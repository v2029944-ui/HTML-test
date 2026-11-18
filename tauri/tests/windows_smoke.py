"""Run the packaged Tauri app through its real WebView2 and SQLite on an isolated Windows CI runner."""
import json,os,subprocess,sys,tempfile,time,urllib.request,shutil,hashlib
from pathlib import Path
from contextlib import contextmanager
from playwright.sync_api import sync_playwright, Error as PlaywrightError, TimeoutError as PlaywrightTimeoutError
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx

def diagnostics(error):
    output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
    if (output/'windows-runtime.txt').exists():return
    (output/'windows-error.txt').write_text(str(error),encoding='utf-8')
    # Record software/runtime facts only; never dump process environments or full command lines.
    script=r"""
    [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $ErrorActionPreference='Continue'
    Get-Process analiz-rabot-tauri,msedgewebview2 -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,MainWindowTitle | ConvertTo-Json
    Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" | ForEach-Object {
      $port='';if($_.CommandLine -match '--remote-debugging-port=(\d+)'){$port=$Matches[1]}
      [PSCustomObject]@{ProcessId=$_.ProcessId;RemoteDebuggingPort=$port}
    } | ConvertTo-Json
    Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application" -ErrorAction SilentlyContinue | Select-Object Name | ConvertTo-Json
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $area=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap=New-Object System.Drawing.Bitmap($area.Width,$area.Height)
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($area.Location,[System.Drawing.Point]::Empty,$area.Size)
    $bitmap.Save($env:ANALIZ_SMOKE_SCREENSHOT)
    $graphics.Dispose();$bitmap.Dispose()
    """
    env=dict(os.environ,ANALIZ_SMOKE_SCREENSHOT=str(output/'windows-desktop-diagnostic.png'))
    result=subprocess.run(['powershell','-NoProfile','-Command',script],env=env,capture_output=True,encoding='utf-8',errors='replace',timeout=30)
    (output/'windows-runtime.txt').write_text((result.stdout or '')+'\n'+(result.stderr or ''),encoding='utf-8')

def wait_for_webview_page(browser, timeout_ms=60_000):
    """Wait for the installed WebView2 app page, not just the DevTools endpoint.

    Connecting to CDP can succeed before the application has created its page.
    Use the existing page if one is present; otherwise wait for the context's
    page event. This is used for both the first launch and the restart.
    """
    if not browser.contexts:
        raise RuntimeError('WebView2 DevTools connected without a browser context')
    context=browser.contexts[0]
    for page in context.pages:
        if not page.is_closed():
            return page
    print('WebView2 DevTools connected; waiting for application page...',flush=True)
    try:
        return context.wait_for_event('page',timeout=timeout_ms)
    except PlaywrightTimeoutError as error:
        raise RuntimeError('WebView2 application page did not appear within '+str(timeout_ms)+' ms') from error

def wait_for_app_state(page, expression, description, timeout_ms=30_000):
    """Poll through CDP evaluation without page-side eval blocked by the app CSP."""
    deadline=time.monotonic()+timeout_ms/1000
    while True:
        try:
            if page.evaluate(expression):return
        except PlaywrightError as error:
            # A newly created WebView2 page can navigate while startup is being observed.
            if 'Execution context was destroyed' not in str(error) and 'Cannot find context with specified id' not in str(error):
                raise
        remaining=deadline-time.monotonic()
        if remaining<=0:raise RuntimeError(description+' did not complete within '+str(timeout_ms)+' ms')
        page.wait_for_timeout(min(100,remaining*1000))

@contextmanager
def windows_test_workspace():
    """Release only this test's WebView2 workers before deleting their temporary profile."""
    workspace=Path(tempfile.mkdtemp(prefix='analiz-webview2-'))
    try:
        yield workspace
    finally:
        cleanup_script=r"""
        $token=$env:ANALIZ_SMOKE_PROFILE_TOKEN
        if(-not $token.StartsWith('analiz-webview2-')){throw 'Invalid test profile token'}
        $owned=@(Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" | Where-Object {$_.CommandLine -like "*$token*"})
        foreach($item in $owned){
          # A parent worker may already have terminated a child in this snapshot.
          Stop-Process -Id $item.ProcessId -Force -ErrorAction SilentlyContinue
          $remaining=Get-Process -Id $item.ProcessId -ErrorAction SilentlyContinue
          if($remaining -and -not $remaining.WaitForExit(10000)){throw 'A test WebView2 worker did not exit'}
        }
        $left=@(Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" | Where-Object {$_.CommandLine -like "*$token*"})
        if($left.Count){throw 'Test WebView2 workers remain after cleanup'}
        """
        subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',cleanup_script],
            env=dict(os.environ,ANALIZ_SMOKE_PROFILE_TOKEN=workspace.name),check=True,capture_output=True,timeout=30)
        deadline=time.monotonic()+10
        while workspace.exists():
            try:shutil.rmtree(workspace)
            except PermissionError as error:
                if getattr(error,'winerror',None) not in (5,32,33) or time.monotonic()>=deadline:raise
                time.sleep(.2)

def run():
    if sys.platform!='win32' or os.environ.get('GITHUB_ACTIONS')!='true':raise RuntimeError('Run only on an isolated Windows GitHub Actions runner')
    installers=list((ROOT/'tauri/src-tauri/target/release/bundle/nsis').glob('*.exe'))
    if len(installers)!=1:raise RuntimeError('Expected one freshly built installer')
    output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
    # Production installer is preserved before building the isolated CDP-enabled bundle.
    # For PRs this is unsigned; the release-preview branch retains its signed artifact.
    unsigned_ci=os.environ.get('ANALIZ_SMOKE_UNSIGNED_CI')=='true'
    installer=output/'setup.exe';signature=output/'setup.exe.sig'
    if not installer.is_file() or (not unsigned_ci and not signature.is_file()):
        raise RuntimeError('Original installer (and signature for signed preview) must be preserved')
    if unsigned_ci and signature.exists():
        raise RuntimeError('Unsigned PR smoke must not reuse a signed-release artifact')
    gui_installer=installers[0]
    if hashlib.sha256(gui_installer.read_bytes()).digest()==hashlib.sha256(installer.read_bytes()).digest():
        raise RuntimeError('GUI test bundle must differ from the distributable installer')
    report={'installer':str(installer.relative_to(ROOT)),'bytes':installer.stat().st_size,
            'sha256':hashlib.sha256(installer.read_bytes()).hexdigest(),
            'mode':'unsigned PR' if unsigned_ci else 'signed preview',
            'gui_check':'separately compiled non-distributable WebView2 CDP build','checks':[]}
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print('INSTALLER_PATH='+report['installer'],flush=True);print('INSTALLER_BYTES='+str(report['bytes']),flush=True)
    with windows_test_workspace() as tmp:
        tmp=Path(tmp);destination=tmp/'installed'
        # CDP comes from the test-only Tauri configuration's WebView2 API options,
        # not from an environment flag ignored by elevated modern WebView2.
        env=dict(os.environ,WEBVIEW2_USER_DATA_FOLDER=str(tmp/'webview'))
        env.pop('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS',None)
        env['NO_PROXY']=env.get('NO_PROXY','')+',127.0.0.1,localhost'
        subprocess.run([str(gui_installer),'/S','/D='+str(destination)],check=True,timeout=240,env=env)
        executables=list(destination.rglob('analiz-rabot-tauri.exe'))
        if len(executables)!=1:raise RuntimeError('Installed application executable not found')
        # NSIS may launch the application before the test starts its own instance.
        # Stop only instances installed in our disposable test directory, together with their WebView2 children.
        find_owned="@(Get-Process analiz-rabot-tauri -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $env:ANALIZ_SMOKE_BINARY} | ForEach-Object {$_.Id}) | ConvertTo-Json -Compress"
        own_env=dict(env,ANALIZ_SMOKE_BINARY=str(executables[0]))
        result=subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',find_owned],env=own_env,capture_output=True,check=True)
        owned=json.loads(result.stdout.decode('utf-8').strip() or '[]')
        if isinstance(owned,int):owned=[owned]
        print('INSTALLER_AUTOSTART_INSTANCES='+str(len(owned)),flush=True)
        for pid in owned:subprocess.run(['taskkill','/PID',str(pid),'/T','/F'],check=True,capture_output=True)
        direct=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def start():
            process=subprocess.Popen([str(executables[0])],env=env)
            try:
                deadline=time.monotonic()+90
                while time.monotonic()<deadline:
                    if process.poll() is not None:raise RuntimeError('Tauri exited before creating its WebView2')
                    try:
                        with direct.open('http://127.0.0.1:9227/json/version',timeout=2) as response:
                            data=json.load(response)
                        return process,data
                    except (OSError,ValueError):time.sleep(.5)
                raise RuntimeError('WebView2 DevTools endpoint did not become available')
            except BaseException as error:
                try:diagnostics(error)
                finally:
                    if process.poll() is None:subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],check=True,capture_output=True)
                raise
        def stop(process):
            if process.poll() is not None:return
            # Simulate closing the application window, not crashing its WebView2 storage process.
            close_script="$app=Get-Process -Id $env:ANALIZ_SMOKE_PID -ErrorAction Stop; if(-not $app.CloseMainWindow()){throw 'Application window refused a normal close request'}"
            try:
                subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',close_script],
                    env=dict(env,ANALIZ_SMOKE_PID=str(process.pid)),check=True,capture_output=True,timeout=15)
                process.wait(timeout=30)
            finally:
                # This is failure cleanup only; an unsuccessful normal close still fails the test.
                if process.poll() is None:subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],check=True,capture_output=True)
        with sync_playwright() as pw:
            process,engine=start();browser=None
            try:
                browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227')
                page=wait_for_webview_page(browser);errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
                wait_for_app_state(page, 'Boolean(window.appReady)', 'Application initialization');page.evaluate('window.appReady')
                assert page.evaluate('!!window.__TAURI__.core.invoke')
                data=page.evaluate('window.desktopAPI.load()')
                if data['reports']:raise RuntimeError('CI data folder is not empty; refusing to overwrite reports')
                report['webview2']=engine.get('Browser');report['customizableSelect']=page.evaluate("CSS.supports('appearance','base-select')");report['checks'].append('real WebView2 launch')
                page.locator('#reportsShortcut').click()
                excel=tmp/'synthetic-year.xlsx';excel.write_bytes(wide_xlsx(2024))
                page.locator('#excelFile').set_input_files(str(excel));wait_for_app_state(page, 'state.workbook!==null', 'Excel workbook loading')
                page.locator('#inspectFile').click();wait_for_app_state(page, 'state.pending.length===366', 'Preview of 366 daily reports')
                page.locator('#saveImport').click();wait_for_app_state(page, 'state.reports.length===366', 'Saving 366 daily reports')
                loaded=page.evaluate('window.desktopAPI.load()');assert len(loaded['reports'])==366
                assert abs(sum(r['amount'] for r in loaded['reports'])-7320)<.001
                report['checks'].append('Excel import and 366 reports persisted through native SQLite')
                # D4a read-only SQLite browsing must operate through the actual
                # installed Tauri IPC, not an in-memory JS model or mocked bridge.
                native=page.locator('section[aria-label="Постраничный просмотр SQLite"]')
                native.get_by_role('button',name='Открыть список отчётов').click()
                inspector_css='section[aria-label="Постраничный просмотр SQLite"]'
                wait_for_app_state(page,
                    'document.querySelectorAll('+json.dumps(inspector_css+' tbody tr')+').length === 100',
                    'First bounded SQLite metadata page')
                assert native.locator('tbody tr').count()==100
                original_id=native.locator('tbody tr').first.locator('td').nth(4).inner_text()
                native.get_by_role('button',name='Далее').click()
                wait_for_app_state(page,
                    'document.querySelector('+json.dumps(inspector_css+' tbody tr td:last-child')+')?.textContent !== '+json.dumps(original_id),
                    'Second bounded SQLite metadata page')
                assert native.locator('tbody tr').count()==100
                native.get_by_role('button',name='Назад').click()
                wait_for_app_state(page,
                    'document.querySelector('+json.dumps(inspector_css+' tbody tr td:last-child')+')?.textContent === '+json.dumps(original_id),
                    'SQLite keyset back navigation')
                native.get_by_label('Фильтр типа отчёта в SQLite').select_option('week')
                wait_for_app_state(page,
                    'document.querySelector('+json.dumps(inspector_css+' [role="status"]')+')?.textContent.includes("записей на странице: 0")',
                    'SQLite type-filtered empty report page')
                assert native.locator('tbody tr').count()==0
                native.get_by_label('Фильтр типа отчёта в SQLite').select_option('day')
                wait_for_app_state(page,
                    'document.querySelectorAll('+json.dumps(inspector_css+' tbody tr')+').length === 100',
                    'SQLite daily reports re-filtered page')
                report['checks'].append('native SQLite revision-checked pagination and period filter')
                page.locator('[data-view=dashboard]').click()
                page.locator('#periodType').click();page.keyboard.press('End');page.keyboard.press('Enter')
                assert page.locator('#periodType').input_value()=='day'
                page.locator('#orgFilter summary').click();checkbox=page.locator('#orgFilter input').first;was=checkbox.is_checked()
                checkbox.focus();page.keyboard.press('Space');assert page.locator('#orgFilter input').first.is_checked()!=was
                page.keyboard.press('Escape');report['checks'].append('native and multiple dropdown keyboard input')
                page.locator('#reportsShortcut').click()
                page.locator('#calendarYearSelect').select_option('2024');page.locator('#calendarType').select_option('day')
                page.locator('[data-month="2024-02"]').click();assert page.locator('#calendarDetails button[data-date]').count()==29
                report['checks'].append('leap February calendar')
                # D4b: the alternate SQL-only page must render without starting
                # the original in-memory report model. Exercise real Tauri IPC.
                page.locator('#reportsShortcut').click()
                page.locator('#openNativeWorkspace').click()
                wait_for_app_state(page,'Boolean(window.nativeWorkspaceReady)','Native SQLite workspace startup')
                page.evaluate('window.nativeWorkspaceReady')
                if page.evaluate("typeof window.appReady !== 'undefined'"):
                    raise RuntimeError('Native workspace unexpectedly imported legacy full-history bootstrap')
                assert page.locator('#metricReports').inner_text()=='366'
                assert page.locator('#reportRows tr').count()==100
                assert page.locator('#metricGroups').inner_text()=='0'
                first_native=page.locator('#reportRows tr').first.locator('td').nth(4).inner_text()
                page.locator('#reportNext').click()
                wait_for_app_state(page,
                    'document.querySelector("#reportRows tr td:last-child")?.textContent !== '+json.dumps(first_native),
                    'Native SQL next page')
                assert page.locator('#reportRows tr').count()==100
                page.locator('#reportBack').click()
                wait_for_app_state(page,
                    'document.querySelector("#reportRows tr td:last-child")?.textContent === '+json.dumps(first_native),
                    'Native SQL previous page')
                page.locator('#nativeReportType').select_option('week')
                wait_for_app_state(page,
                    'document.querySelector("#reportStatus")?.textContent.includes("На странице: 0")',
                    'Native SQLite filter without week reports')
                assert page.locator('#reportRows tr').count()==0
                # D4c: a type with no reports can be indexed without loading
                # history, then its incomplete SQL range must have NULL totals.
                page.locator('#analysisType').select_option('week')
                page.locator('#analysisFrom').fill('2026-11-02')
                page.locator('#analysisTo').fill('2026-11-09')
                page.locator('#analysisPrepare').click()
                wait_for_app_state(page,
                    'document.querySelector("#analysisRun")?.disabled === false',
                    'Bounded SQL analysis index preparation')
                page.locator('#analysisRun').click()
                wait_for_app_state(page,
                    'document.querySelector("#analysisStatus")?.textContent.includes("не все периоды")',
                    'SQL incomplete-range detection')
                assert page.locator('#analysisValues .metric').count()==9
                assert page.locator('#analysisValues .metric').nth(3).locator('strong').inner_text()=='—'
                assert page.locator('#analysisValues .metric').nth(4).locator('strong').inner_text()=='—'
                # D4d comparison derives complete/incomplete per-division rows
                # through bounded D2 SQL, not through JS history data.
                wait_for_app_state(page,
                    'document.querySelectorAll("#divisionRows tr").length > 0',
                    'Native organization summary table')
                assert all(row.locator('td').nth(4).inner_text()=='—'
                    for row in page.locator('#divisionRows tr').all())
                assert all(row.locator('td').nth(2).inner_text()!='0'
                    for row in page.locator('#divisionRows tr').all())
                report['checks'].append('D2 incomplete SQL range keeps missing totals null')
                report['checks'].append('Native division comparison preserves unknown incomplete amounts')
                # D4e: the group/work keyset API must also run on the real
                # installed app; an empty weekly range produces no fake zeros.
                wait_for_app_state(page,
                    'document.querySelector("#workStatus")?.textContent.includes("На странице: 0")',
                    'Native SQL group/work breakdown for absent weekly reports')
                assert page.locator('#workRows tr').count()==0
                assert page.locator('#workNext').is_disabled()
                report['checks'].append('Native work/group keyset breakdown is empty for missing weekly data')
                report['checks'].append('SQL-only native page, bounded report navigation, settings bootstrap')
                page.locator('#legacyWorkspaceLink').click()
                wait_for_app_state(page,'Boolean(window.appReady)','Legacy workspace after native route')
                page.evaluate('window.appReady')
                assert page.evaluate('state.reports.length')==366
                page.locator('#reportsShortcut').click()
                page.screenshot(path=str(output/'windows-webview2.png'))
                assert not errors,errors
                before_restart=page.evaluate("({view:document.querySelector('.view.active').id,saved:localStorage.getItem('work-reports-view-v1')})")
                report['navigation_before_restart']=before_restart
                (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
                assert before_restart=={'view':'upload','saved':'upload'},before_restart
            except BaseException as error:
                diagnostics(error);raise
            finally:
                try:stop(process)
                finally:
                    if browser and browser.is_connected():browser.close()
            process,_=start();browser=None
            try:
                browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227');page=wait_for_webview_page(browser)
                wait_for_app_state(page, 'Boolean(window.appReady)', 'Application initialization');page.evaluate('window.appReady')
                assert len(page.evaluate('window.desktopAPI.load()')['reports'])==366
                restored=page.evaluate("({view:document.querySelector('.view.active').id,saved:localStorage.getItem('work-reports-view-v1')})")
                report['navigation_after_restart']=restored
                (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
                assert restored=={'view':'upload','saved':'upload'},restored
                report['checks'].append('SQLite and last view survive application restart')
            except BaseException as error:
                diagnostics(error);raise
            finally:
                try:stop(process)
                finally:
                    if browser and browser.is_connected():browser.close()
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False),flush=True)

if __name__=='__main__':
    try:run()
    except BaseException as error:
        diagnostics(error)
        raise
