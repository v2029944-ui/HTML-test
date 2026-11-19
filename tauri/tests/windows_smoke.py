"""Run the packaged Tauri app through its real WebView2 and SQLite on an isolated Windows CI runner."""
import json,os,subprocess,sys,tempfile,time,urllib.request,shutil,hashlib,sqlite3
from pathlib import Path
from contextlib import contextmanager, closing
from playwright.sync_api import sync_playwright, Error as PlaywrightError, TimeoutError as PlaywrightTimeoutError
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx
from test_import_controls import daily_workbook_bytes
sys.path.insert(0,str(ROOT/'tauri'/'examples'))
from make_metric_xlsx import create_example

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

def reveal_table(page, selector):
    """Read visible exact values only after opening their details disclosure."""
    for details in page.locator('details').filter(has=page.locator(selector)).all():
        if not details.evaluate('(el) => el.open'):
            details.locator(':scope > summary').click()

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
                # P1.6: measure the first screen, including real SQL KPI
                # and filter layout, at desktop and phone CSS viewport sizes.
                # This is an isolated test WebView, not the released installer.
                previous_viewport=page.evaluate('({width:innerWidth,height:innerHeight})')
                try:
                    for width,height in ((1280,720),(1366,768),(390,844),(320,568)):
                        page.set_viewport_size({'width':width,'height':height})
                        geometry=page.evaluate('''() => {
                          const rect = id => {
                            const box = document.getElementById(id).getBoundingClientRect();
                            return {top:box.top,bottom:box.bottom,width:box.width};
                          };
                          return {
                            viewport:innerWidth,
                            scrollWidth:document.documentElement.scrollWidth,
                            filters:rect('analysisType'),
                            kpi:rect('metricReports'),
                            nextBlock:rect('sqlAnalysis'),
                            active:document.querySelector('[data-nav-link="analysis"]')?.getAttribute('aria-current')
                          };
                        }''')
                        if width>=1280:
                            if not (geometry['filters']['bottom']<height and
                                    geometry['kpi']['bottom']<height and
                                    geometry['nextBlock']['top']<height):
                                raise RuntimeError(f'SQL first viewport {width}x{height} hides filters, KPI or the next result panel: {geometry}')
                        elif geometry['scrollWidth']>width+1:
                            raise RuntimeError(f'SQL phone viewport has page-level horizontal scrolling at {width}x{height}: {geometry}')
                        page.screenshot(path=str(output/f'windows-sql-{width}x{height}.png'),full_page=False)
                    report['checks'].append('Measured SQL first screen at 1280x720, 1366x768, 390x844, 320x568')
                    # Issue #64: large UI scaling must not force the whole
                    # SQL screen to scroll horizontally. Tables may scroll
                    # internally by design, but the page itself must fit.
                    page.set_viewport_size({'width':1280,'height':720})
                    for scale in ('1.25','1.5','2'):
                        zoom=page.evaluate('''scale => {
                          const html=document.documentElement;
                          const before=html.style.zoom;
                          html.style.zoom=scale;
                          const output={
                            scale,
                            viewport:innerWidth,
                            page:html.scrollWidth,
                            toolbar:document.querySelector('.analytics-toolbar').getBoundingClientRect().right,
                            panel:document.querySelector('#sqlOverview').getBoundingClientRect().right,
                            chart:document.querySelector('#timelineChart').getBoundingClientRect().right,
                            controls:document.querySelector('.analytics-toolbar .controls').getBoundingClientRect().right
                          };
                          html.style.zoom=before;
                          return output;
                        }''',scale)
                        if zoom['page']>zoom['viewport']+2 or zoom['panel']>zoom['viewport']+2 or zoom['controls']>zoom['viewport']+2:
                            raise RuntimeError(f'SQL zoom {scale} produces horizontal overflow: {zoom}')
                        if scale=='2':
                            page.screenshot(path=str(output/'windows-sql-zoom-200.png'),full_page=False)
                    report['checks'].append('SQL zoomed cards and calendar controls remain within page width at 125, 150 and 200 percent')
                finally:
                    page.set_viewport_size(previous_viewport)
                # The native SQL workspace is now one application with sections.
                # Switching sections must not initialize the full-history UI.
                wait_for_app_state(page,'Boolean(window.nativeWorkspaceNavigation)',
                    'Native workspace navigation startup')
                assert page.locator('[data-nav-link="analysis"]').get_attribute('aria-current') == 'page'
                page.locator('[data-nav-link="excel"]').click()
                wait_for_app_state(page,
                    'document.querySelector("#sqlSources")?.offsetParent !== null && document.querySelector("#sqlAnalysis")?.offsetParent === null',
                    'SQL source section after tab navigation')
                assert page.locator('#sqlSources').is_visible()
                assert not page.locator('#sqlAnalysis').is_visible()
                assert page.evaluate("typeof window.appReady === 'undefined'")
                report['checks'].append('Unified SQL navigation preserves bounded loading and Excel controls')
                # SQL source panel must only enumerate Excel metadata via Rust.
                wait_for_app_state(page, 'Boolean(window.nativeFolderReady)', 'SQL source folder startup')
                page.evaluate('window.nativeFolderReady')
                page.evaluate('async folder => window.__TAURI__.core.invoke("set_folder", {folder})', str(tmp))
                page.locator('#sourceFolderRefresh').click()
                wait_for_app_state(page,
                    'document.querySelector("#sourceFolderRows tr td")?.textContent === "synthetic-year.xlsx"',
                    'SQL source folder metadata')
                assert page.locator('#sourceFolderRows tr').count()==1
                assert '1' in page.locator('#sourceFolderCount').inner_text()
                # set_folder changed the SQLite config revision; reload the bounded
                # read model so subsequent range queries do not use a stale revision.
                assert page.evaluate('window.nativeWorkspaceRefresh()') is True
                assert page.evaluate("typeof window.appReady === 'undefined'")
                report['checks'].append('SQL-only folder metadata, no full-history bootstrap or implicit import')
                # Read-only end-to-end native ordinary Excel preview; tests real Rust parser
                # without adding or replacing user reports or loading XLSX bytes in JS.
                legacy_source=tmp/'legacy-preview.xlsx'
                legacy_source.write_bytes(daily_workbook_bytes())
                sheets=page.evaluate('source => window.desktopAPI.nativeLegacyImportSheets(source)',str(legacy_source))
                assert 'xl/worksheets/sheet1.xml' in sheets,sheets
                legacy_options={'sheet':'xl/worksheets/sheet1.xml','kind':'week',
                                'start':'2026-11-09','organizations':['СМУ16','АВТО','ТРАК','ЦЕНТР']}
                legacy_preview=page.evaluate(
                    'args => window.desktopAPI.nativeLegacyImportPreview(args.source,args.options)',
                    {'source':str(legacy_source),'options':legacy_options})
                assert legacy_preview['reportCount']>=1
                assert len(legacy_preview['reviewToken'])==64
                assert legacy_preview['sampleReports']
                assert page.evaluate("typeof window.appReady === 'undefined'")
                report['checks'].append('read-only streamed Rust legacy XLSX preview with real cells')
                # Opt in to SQL-first startup; the setting must survive closing WebView2.
                page.locator('#sqlStartupPreference').check()
                assert page.evaluate("localStorage.getItem('analiz-sql-startup-v1')") == 'sql'
                if page.evaluate("typeof window.appReady !== 'undefined'"):
                    raise RuntimeError('Native workspace unexpectedly imported legacy full-history bootstrap')
                # Declarative report families are separate from classic reports.
                wait_for_app_state(page,'Boolean(window.metricKindWorkspaceReady)',
                    'Declarative metric-kind workspace startup')
                page.evaluate('window.metricKindWorkspaceReady')
                assert page.locator('#metricKindSelect option').count()==2
                example=Path(__file__).resolve().parents[1]/'examples'/'labor-report.json'
                metric_preview=page.evaluate(
                    'async source => window.__TAURI__.core.invoke("native_metric_preview",{source})',
                    str(example))
                assert metric_preview['kind']=='labor'
                assert metric_preview['rowCount']==2
                assert metric_preview['values']['hours']==12
                assert abs(metric_preview['values']['units_per_hour']-19/12)<1e-8
                metric_receipt=page.evaluate(
                    'async data => window.__TAURI__.core.invoke("native_metric_apply",data)',
                    {'source':str(example),'reviewToken':metric_preview['reviewToken']})
                assert metric_receipt['rowCount']==2
                page.locator('[data-nav-link="metrics"]').click()
                assert page.locator('#sqlMetricKinds').is_visible()
                page.locator('#metricKindCalculate').click()
                wait_for_app_state(page,
                    'document.querySelector("#metricKindCount")?.textContent.includes("Отчётов: 1")',
                    'Declarative metrics in installed SQL workspace')
                assert page.locator('#metricKindTotals .metric').count()==3
                try:
                    page.evaluate(
                        'async data => window.__TAURI__.core.invoke("native_metric_apply",data)',
                        {'source':str(example),'reviewToken':metric_preview['reviewToken']})
                except Exception:
                    pass
                else:
                    raise RuntimeError('Generic metric report duplicate was accepted')
                report['checks'].append('Declarative report families, ratios of sums, IPC, no-overwrite')
                assert page.locator('#metricXlsxProfile option').count()==2
                metric_xlsx=tmp/'metric-labor-demo.xlsx'
                create_example(metric_xlsx)
                mapped=page.evaluate(
                    'async args => window.__TAURI__.core.invoke("native_metric_xlsx_preview",args)',
                    {'source':str(metric_xlsx),'profile':'labor_simple',
                     'start':'2026-11-18','end':'2026-11-18'})
                assert mapped['kind']=='labor'
                assert mapped['rowCount']==2
                assert mapped['values']['hours']==12
                assert mapped['values']['units']==19
                assert abs(mapped['values']['units_per_hour']-19/12)<1e-8
                saved_xlsx=page.evaluate(
                    'async args => window.__TAURI__.core.invoke("native_metric_xlsx_apply",args)',
                    {'source':str(metric_xlsx),'profile':'labor_simple','start':'2026-11-18',
                     'end':'2026-11-18','reviewToken':mapped['reviewToken']})
                assert saved_xlsx['rowCount']==2
                calculated=page.evaluate(
                    'async args => window.__TAURI__.core.invoke("native_metric_summary",args)',
                    {'kind':'labor','start':'2026-11-18','end':'2026-11-18'})
                assert calculated['reportCount']==2
                assert calculated['values']['hours']==24
                assert calculated['values']['units']==38
                try:
                    page.evaluate(
                        'async args => window.__TAURI__.core.invoke("native_metric_xlsx_apply",args)',
                        {'source':str(metric_xlsx),'profile':'labor_simple','start':'2026-11-18',
                         'end':'2026-11-18','reviewToken':mapped['reviewToken']})
                except Exception:
                    pass
                else:
                    raise RuntimeError('XLSX declarative mapping accepted repeated source')
                report['checks'].append('Profile-driven XLSX import, ratio of sums, no double-counting')
                assert page.locator('#metricReports').inner_text()=='366'
                assert page.locator('#reportRows tr').count()==100
                assert page.locator('#metricGroups').inner_text()=='0'
                first_native=page.locator('#reportRows tr').first.locator('td').nth(4).inner_text()
                page.locator('[data-nav-link="reports"]').click()
                assert page.locator('#sqlReports').is_visible()
                page.locator('#reportNext').click()
                wait_for_app_state(page,
                    'document.querySelector("#reportRows tr td:nth-child(5)")?.textContent !== '+json.dumps(first_native),
                    'Native SQL next page')
                assert page.locator('#reportRows tr').count()==100
                assert page.locator('#reportPageInfo').inner_text()=='Страница 2'
                page.locator('#reportBack').click()
                wait_for_app_state(page,
                    'document.querySelector("#reportRows tr td:nth-child(5)")?.textContent === '+json.dumps(first_native),
                    'Native SQL previous page')
                assert page.locator('#reportPageInfo').inner_text()=='Страница 1'
                assert page.locator('#sqlDailyRollups').is_visible()
                page.locator('#dailyRollupGranularity').select_option('year')
                assert page.locator('#dailyRollupFrom').get_attribute('type')=='number'
                page.locator('#dailyRollupFrom').fill('2024')
                page.locator('#dailyRollupTo').fill('2024')
                page.locator('#dailyRollupPrepare').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailyRollupRun")?.disabled === false',
                    'Daily report index preparation for calendar rollup')
                page.locator('#dailyRollupRun').click()
                wait_for_app_state(page,
                    'document.querySelectorAll("#dailyRollupRows tr").length === 1',
                    '2024 calendar rollup from indexed day sources')
                assert page.locator('#dailyRollupRows tr').first.locator('td').first.inner_text()=='2024'
                assert page.locator('#dailyRollupRows tr').first.locator('td').nth(1).inner_text()=='366'
                assert page.locator('#dailyRollupRows tr').count()<=60
                wait_for_app_state(page,
                    'document.querySelectorAll("#dailySourceKpis .comparison-metric").length === 5 && document.querySelector("#dailySourceChart svg") !== null',
                    'KPI and monthly chart derived only from 366 daily reports')
                # innerText is empty inside a closed details element in WebView2.
                # Open the exact-values table as a user would before reading cells.
                source_details=page.locator('details').filter(has=page.locator('#dailySourceRows'))
                if not source_details.evaluate('(el) => el.open'):
                    source_details.locator('summary').click()
                assert page.locator('#dailySourceRows tr').count()==12
                assert page.locator('#dailySourceRows tr').first.locator('td').first.inner_text()=='2024-01'
                assert page.locator('#dailySourceKpis .comparison-metric').first.locator('strong').inner_text()!='—'
                assert 'календарные месяцы' in page.locator('#dailySourceStatus').inner_text()
                assert 'дневные' in page.locator('#dailyRollupScope').inner_text().lower()
                report['checks'].append('SQL day-source KPIs and chart match the calendar scope, without weekly or monthly imports')
                # Issue #64: the same exact work-name filter must affect the
                # SQL calendar, KPI cards and graph, then restore the baseline.
                baseline_amount=page.locator('#dailySourceKpis .comparison-metric').nth(1).locator('strong').inner_text()
                page.locator('[data-nav-link="analysis"]').click()
                assert page.locator('#sqlAnalysis').is_visible()
                if not page.locator('#analysisMoreFilters').evaluate('(el) => el.open'):
                    page.locator('#analysisMoreFilters summary').click()
                page.locator('#analysisNameSearch').fill('Альфа')
                page.locator('#analysisNameSearchRun').click()
                wait_for_app_state(page,
                    'Array.from(document.querySelector("#analysisName")?.options || []).some(o => o.value === "Альфа")',
                    'Bounded SQLite work-name search includes synthetic daily work')
                page.locator('#analysisName').select_option('Альфа')
                page.locator('#analysisNameAdd').click()
                assert 'Альфа' in page.locator('#analysisFilterScope').inner_text()
                page.locator('[data-nav-link="reports"]').click()
                assert 'Альфа' in page.locator('#dailyRollupScope').inner_text()
                page.locator('#dailyRollupRun').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailySourceStatus")?.textContent.includes("календарные месяцы") && document.querySelectorAll("#dailySourceKpis .comparison-metric").length === 5',
                    'Day-source KPI and graph recomputed for the chosen work')
                filtered_amount=page.locator('#dailySourceKpis .comparison-metric').nth(1).locator('strong').inner_text()
                if filtered_amount == '—':
                    raise RuntimeError('Exact work filter lost the valid daily-source amount')
                # This workbook can contain only one raw work name. The true
                # coverage distinction is a selected *absent* work, which must
                # produce verified zero instead of a fabricated missing day.
                absent_work=page.evaluate('''async () => {
                  const metadata=await window.desktopAPI.nativeSettingsBootstrap();
                  return window.desktopAPI.nativeAnalysisRange(
                    metadata.revision,'day','2024-01-01','2024-12-31',
                    metadata.org_names,'__sql_smoke_absent_work__');
                }''')
                assert absent_work['complete'] is True
                assert absent_work['missing']==0
                assert absent_work['total']['amount']==0
                assert absent_work['total']['count']==0
                assert page.locator('#dailySourceRows tr').count()==12
                assert page.locator('#dailySourceChart svg').count()==1
                page.locator('[data-nav-link="analysis"]').click()
                page.locator('#analysisNameReset').click()
                assert 'все' in page.locator('#analysisFilterScope').inner_text()
                page.locator('[data-nav-link="reports"]').click()
                page.locator('#dailyRollupRun').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailySourceStatus")?.textContent.includes("календарные месяцы") && document.querySelectorAll("#dailySourceKpis .comparison-metric").length === 5',
                    'Day-source KPI restores original scope after filter reset')
                assert page.locator('#dailySourceKpis .comparison-metric').nth(1).locator('strong').inner_text()==baseline_amount
                report['checks'].append('SQL exact work-name selection preserves coverage and resets cleanly')

                assert page.evaluate("typeof window.appReady === 'undefined'")
                report['checks'].append('SQL 2024 annual calendar is computed from daily reports only')
                page.locator('#dailyRollupRows tr').first.locator('button').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailyRollupDetailPanel")?.hidden === false && document.querySelectorAll("#dailyRollupDetailRows tr").length === 60',
                    'SQL annual calendar shows first 60 real daily periods')
                assert page.locator('#dailyRollupDetailRows tr').first.locator('td').first.inner_text()=='2024-01-01'
                assert page.locator('#dailyRollupDetailNext').is_enabled()
                page.locator('#dailyRollupDetailNext').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailyRollupDetailPage")?.textContent === "Страница 2"',
                    'SQL daily calendar details next keyset page')
                assert page.locator('#dailyRollupDetailRows tr').first.locator('td').first.inner_text()=='2024-03-01'
                page.locator('#dailyRollupDetailBack').click()
                wait_for_app_state(page,
                    'document.querySelector("#dailyRollupDetailPage")?.textContent === "Страница 1"',
                    'SQL daily calendar details previous keyset page')
                assert page.locator('#dailyRollupDetailRows tr').first.locator('td').first.inner_text()=='2024-01-01'
                page.locator('#dailyRollupDetailClose').click()
                assert page.locator('#dailyRollupDetailPanel').is_hidden()
                assert page.locator('#dailyRollupRows tr').count()==1
                report['checks'].append('SQL annual-to-day detail pages bound to exact SQLite dates with explicit close')
                page.locator('#nativeReportType').select_option('week')
                wait_for_app_state(page,
                    'document.querySelector("#reportStatus")?.textContent.includes("На странице: 0")',
                    'Native SQLite filter without week reports')
                assert page.locator('#reportRows tr').count()==0
                # Exercise the new SQL settings UI on synthetic CI data only.
                page.evaluate('window.confirm = () => true')
                page.locator('[data-nav-link="groups"]').click()
                page.locator('#managementGroupName').fill('CI SQL group')
                page.locator('#managementGroupCreate').click()
                group_open=page.get_by_role('button',name='Открыть состав группы CI SQL group',exact=True)
                group_open.wait_for(state='visible')
                wait_for_app_state(page,'!document.querySelector("#managementGroupCreate").disabled','SQL group publication finished')
                group_open.click()
                page.locator('#managementMemberName').fill('CI SQL work')
                page.locator('#managementMemberAdd').click()
                wait_for_app_state(page,
                    'document.querySelector("#managementMemberRows")?.textContent.includes("CI SQL work")',
                    'SQL group member committed and reread through bounded IPC')
                page.get_by_role('button',name='Исключить CI SQL work',exact=True).click()
                wait_for_app_state(page,
                    'document.querySelectorAll("#managementMemberRows tr").length === 0 && !document.querySelector("#managementMemberAdd").disabled',
                    'SQL group member removal')
                page.get_by_role('button',name='Удалить группу CI SQL group',exact=True).click()
                wait_for_app_state(page,
                    '!Array.from(document.querySelectorAll("#groupRows button")).some(b => b.getAttribute("aria-label") === "Открыть состав группы CI SQL group") && !document.querySelector("#managementGroupCreate").disabled',
                    'SQL group removal')
                report['checks'].append('Installed SQL group create/member add/member remove/group delete with confirmation')
                page.locator('[data-nav-link="analysis"]').click()
                page.locator('#analysisSource').select_option('daily')
                page.locator('#analysisType').select_option('month')
                page.locator('#analysisFrom').fill('2024-02')
                page.locator('#analysisTo').fill('2024-02')
                page.locator('#analysisPrepare').click()
                wait_for_app_state(page,'!document.querySelector("#analysisRun").disabled','Daily-source month index')
                page.locator('#analysisRun').click()
                wait_for_app_state(page,
                    'document.querySelectorAll("#timelineRows tr").length === 1 && document.querySelector("#timelineRows tr td")?.textContent === "2024-02" && !document.querySelector("#analysisRun").disabled && document.querySelector("#analysisResultScope")?.textContent.includes("только дневные")',
                    'Daily-source selected month produces one SQL month bucket')
                assert 'только дневные' in page.locator('#analysisResultScope').inner_text()
                assert page.locator('#analysisValues').inner_text().find('Полная')>=0
                assert page.locator('#timelineRows tr td').nth(1).text_content()=='29'
                page.locator('#hideEmptyPeriods').check()
                reveal_table(page, '#timelineRows')
                assert page.locator('#timelineRows tr').count()==1
                page.locator('#hideEmptyPeriods').uncheck()
                report['checks'].append('Daily-source KPI and monthly SQL bucket cover all leap-February days')
                # Exercise the real native commands for every specialized view
                # against one captured complete daily-source calendar month.
                page.locator('#comparisonBeforeFrom').fill('2024-01')
                page.locator('#comparisonBeforeTo').fill('2024-01')
                for mode in ['changes', 'usual', 'regularity', 'structure', 'contributions']:
                    page.locator('#advancedMode').select_option(mode)
                    wait_for_app_state(page,
                        '!document.querySelector("#advancedRun").disabled',
                        'Advanced SQL view is ready: '+mode)
                    page.locator('#advancedRun').click()
                    wait_for_app_state(page,
                        '!document.querySelector("#advancedRun").disabled && document.querySelectorAll("#advancedRows tr").length > 0',
                        'Advanced native SQL result: '+mode)
                    assert 'Ошибка' not in page.locator('#advancedStatus').inner_text()
                    assert 'дневные отчёты' in page.locator('#advancedContext').inner_text()
                    assert '2024-02-01' in page.locator('#advancedContext').inner_text()
                    assert 'из ' in page.locator('#advancedPage').inner_text()
                page.emulate_media(media='print')
                assert page.locator('#advancedPage').is_visible()
                assert page.locator('#advancedContext').is_visible()
                assert page.locator('#advancedRows').is_visible()
                page.emulate_media(media='screen')
                report['checks'].append('All five advanced SQL analyses use the daily-source scope; printed page retains row bounds and context')
                page.locator('#analysisSource').select_option('imported')
                # D4c: a type with no reports can be indexed without loading
                # history, then its incomplete SQL range must have NULL totals.
                page.locator('[data-nav-link="analysis"]').click()
                assert page.locator('#sqlAnalysis').is_visible()
                page.locator('#analysisType').select_option('week')
                assert page.locator('#analysisFrom').get_attribute('type')=='week'
                assert page.locator('#analysisTo').get_attribute('type')=='week'
                page.locator('#analysisFrom').fill('2026-W45')
                page.locator('#analysisTo').fill('2026-W46')
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
                assert page.locator('#analysisCoverageNotice').is_visible()
                assert 'Подтверждение скрывает' in page.locator('#analysisCoverageText').inner_text()
                page.locator('#analysisAcknowledge').click()
                assert page.locator('#analysisCoverageNotice').is_hidden()
                assert 'Замечание подтверждено' in page.locator('#analysisStatus').inner_text()
                # Acknowledging the warning must not materialize missing values.
                assert page.locator('#analysisValues .metric').nth(4).locator('strong').inner_text()=='—'
                report['checks'].append('SQL acknowledgement suppresses only the known warning, not absent totals')
                # D4d comparison derives complete/incomplete per-division rows
                # through bounded D2 SQL, not through JS history data.
                wait_for_app_state(page,
                    'document.querySelectorAll("#divisionRows tr").length > 0',
                    'Native organization summary table')
                assert all(row.locator('td').nth(4).inner_text()=='—'
                    for row in page.locator('#divisionRows tr').all())
                assert all(row.locator('td').nth(2).inner_text()!='0'
                    for row in page.locator('#divisionRows tr').all())
                # The graphical SQL layer must use the same missing-period evidence.
                wait_for_app_state(page,
                    'document.querySelector("#timelineStatus")?.textContent.includes("Ни одной полной точки")',
                    'SQL timeline has no fabricated zero observations')
                reveal_table(page, '#timelineRows')
                assert page.locator('#timelineRows tr').count()==2
                assert page.locator('#timelineChart svg').count()==0
                assert all(row.locator('td').nth(4).inner_text()=='—'
                    for row in page.locator('#timelineRows tr').all())
                report['checks'].append('Native timeline preserves missing weekly periods')
                wait_for_app_state(page,
                    'document.querySelector("#rankingStatus")?.textContent.includes("Рейтинг скрыт")',
                    'Native ranking excludes incomplete week ranges')
                assert page.locator('#rankingBars li').count()==0
                assert page.locator('#rankingMetric').is_enabled()
                report['checks'].append('SQL ranking excludes incomplete weekly ranges')
                wait_for_app_state(page,
                    'document.querySelector("#organizationTrendsStatus")?.textContent.includes("Нет подтверждённых точек")',
                    'Native division trends hide missing weekly values')
                reveal_table(page, '#organizationTrendsRows')
                assert page.locator('#organizationTrendsRows tr').count()>0
                assert page.locator('#organizationTrendsRows tr').count()<=992
                assert page.locator('#organizationTrendsChart svg').count()==0
                assert all(row.locator('td').nth(5).inner_text()=='—'
                    for row in page.locator('#organizationTrendsRows tr').all())
                report['checks'].append('Division trend lines are missing when selected weeks are absent')


                report['checks'].append('D2 incomplete SQL range keeps missing totals null')
                report['checks'].append('Native division comparison preserves unknown incomplete amounts')
                # D4e: the group/work keyset API must also run on the real
                # installed app; an empty weekly range produces no fake zeros.
                wait_for_app_state(page,
                    'document.querySelector("#workStatus")?.textContent.includes("На странице: 0")',
                    'Native SQL group/work breakdown for absent weekly reports')
                assert page.locator('#workRows tr').count()==0
                assert page.locator('#workNext').is_disabled()
                assert page.locator('#sqlWorkTrends').is_visible()
                assert page.locator('#workTrendMetric').is_disabled()
                assert page.locator('#workTrendChart svg').count()==0
                reveal_table(page, '#workTrendRows')
                assert page.locator('#workTrendRows tr').count()==0
                assert page.locator('#workTrendSelected').inner_text()=='Позиция не выбрана.'
                report['checks'].append('SQL work/group trend controls start empty without fabricated data')
                page.locator('#comparisonBeforeFrom').fill('2026-W45')
                page.locator('#comparisonBeforeTo').fill('2026-W46')
                wait_for_app_state(page,
                    'document.querySelector("#comparisonRun")?.disabled === false',
                    'Native comparison becomes available after choosing baseline dates')
                page.locator('#comparisonRun').click()
                wait_for_app_state(page,
                    'document.querySelector("#comparisonStatus")?.textContent.includes("есть пропуски")',
                    'Both incomplete SQL ranges keep comparison changes unknown')
                assert page.locator('#comparisonMetrics .comparison-metric').count()==2
                assert page.locator('#comparisonDivisionRows tr').count()==4
                assert all(row.locator('td').nth(3).inner_text()=='—'
                    for row in page.locator('#comparisonDivisionRows tr').all())
                assert page.locator('#comparisonRankRows li').count()==0
                assert 'скрыта' in page.locator('#comparisonRankStatus').inner_text()
                report['checks'].append('SQL comparison refuses fabricated deltas for incomplete baseline')

                report['checks'].append('Native work/group keyset breakdown is empty for missing weekly data')
                # D4f: export uses an on-disk CSV from SQLite, not report JSON,
                # and must reject an existing output path without replacement.
                export_path=tmp/'analysis-empty-week.csv'
                if export_path.exists():
                    raise RuntimeError('CSV export test destination unexpectedly exists')
                export_call="""
                  async path => {
                    const snapshot=await window.desktopAPI.nativeSettingsBootstrap();
                    return window.__TAURI__.core.invoke('native_export_range_csv',{
                      revision:snapshot.revision, reportType:'week',
                      from:'2026-11-02', to:'2026-11-09',
                      organizations:snapshot.org_names, organization:null,
                      destination:path
                    });
                  }
                """
                page.evaluate(export_call,str(export_path))
                exported=export_path.read_bytes()
                assert exported.startswith(b'\xef\xbb\xbf"key";"name";"kind";"count";"amount";"average";"active"')
                assert b'\r\n' in exported
                try:
                    page.evaluate(export_call,str(export_path))
                except Exception as error:
                    assert 'already exists' in str(error)
                else:
                    raise RuntimeError('Native SQL export overwrote an existing CSV')
                assert export_path.read_bytes()==exported
                report['checks'].append('Native SQL CSV export with UTF-8 BOM and no-overwrite guarantee')
                snapshot=tmp/'online-backup.sqlite3'
                backup_call="async path=>window.__TAURI__.core.invoke('native_backup_sqlite',{destination:path})"
                stored_rev=page.evaluate(backup_call,str(snapshot))
                assert isinstance(stored_rev,int)
                assert snapshot.is_file()
                with closing(sqlite3.connect(str(snapshot))) as backed_up:
                    assert backed_up.execute('PRAGMA quick_check').fetchone()[0]=='ok'
                    assert backed_up.execute('SELECT COUNT(*) FROM reports').fetchone()[0]==366
                    assert backed_up.execute('SELECT COUNT(*) FROM metric_report_docs').fetchone()[0]==2
                    assert backed_up.execute("SELECT json FROM kv WHERE name='settings'").fetchone() is not None
                previous=snapshot.read_bytes()
                try:
                    page.evaluate(backup_call,str(snapshot))
                except Exception as error:
                    assert 'already exists' in str(error)
                else:
                    raise RuntimeError('Online backup overwrote existing SQLite')
                assert snapshot.read_bytes()==previous
                report['checks'].append('Online SQLite backup includes WAL and refuses overwrites')
                # E1: native wide import must be inspected and explicitly
                # approved; repeat preparation validates same source/DB content
                # before publishing new report documents atomically.
                native_options={'year':2024,'sheets':[],'mappings':{},
                    'organizations':['СМУ16','АВТО','ТРАК','ЦЕНТР']}
                native_preview=page.evaluate(
                    'async args => window.__TAURI__.core.invoke("native_wide_import_preview",args)',
                    {'source':str(excel),'options':native_options})
                assert native_preview['reportCount']==366
                assert native_preview['rowCount']>0
                assert native_preview['conflicts']['replacements']==366
                token=native_preview['reviewToken']
                assert len(token)==64
                direct_apply='async args => window.__TAURI__.core.invoke("native_wide_import_apply",args)'
                refused={'source':str(excel),'options':native_options,'reviewToken':token,
                         'confirmation':{'replaceExact':True,'acceptCrossType':True,'acceptQuality':True}}
                try:
                    page.evaluate(direct_apply,refused)
                except Exception as error:
                    assert 'backup' in str(error).lower(),str(error)
                else:
                    raise RuntimeError('Native IPC replaced reports without a backup destination')
                before_native=tmp/'before-native-import.sqlite3'
                approved={'source':str(excel),'options':native_options,'reviewToken':token,
                          'confirmation':{'replaceExact':True,'acceptCrossType':True,
                                          'acceptQuality':True,'backupDestination':str(before_native)}}
                receipt=page.evaluate(direct_apply,approved)
                assert before_native.is_file()
                with closing(sqlite3.connect(str(snapshot))) as original_copy:
                    expected_rows=original_copy.execute(
                        'SELECT id,json FROM reports ORDER BY id').fetchall()
                    expected_kv=original_copy.execute(
                        'SELECT name,json FROM kv ORDER BY name').fetchall()
                assert len(expected_rows)==366
                with closing(sqlite3.connect(str(before_native))) as restored_copy:
                    assert restored_copy.execute('PRAGMA quick_check').fetchone()[0]=='ok'
                    assert restored_copy.execute('SELECT COUNT(*) FROM reports').fetchone()[0]==366
                    actual_rows=restored_copy.execute(
                        'SELECT id,json FROM reports ORDER BY id').fetchall()
                    actual_kv=restored_copy.execute(
                        'SELECT name,json FROM kv ORDER BY name').fetchall()
                    assert actual_rows==expected_rows, 'Backup differs from pre-import reports'
                    assert actual_kv==expected_kv, 'Backup differs from pre-import settings'
                report['checks'].append('Native import IPC refuses replacement without verified SQLite backup')
                assert receipt['reports']==366
                assert receipt['replaced']==366
                assert receipt['revision']>native_preview['revision']
                updated=page.evaluate('window.desktopAPI.load()')
                assert len(updated['reports'])==366
                assert all(r.get('sourceFormat')=='daily-wide' for r in updated['reports'])
                report['checks'].append('reviewed native wide XLSX preview and atomic 366-report replacement')
                assert page.evaluate('window.nativeWorkspaceRefresh()')
                page.locator('[data-nav-link="reports"]').click()
                source_anchor=next(row for row in updated['reports'] if 'sourceIssues' in row)
                row=page.locator('#reportRows tr').filter(has_not_text=source_anchor['id']).first
                quality=row.get_by_role('button',name='Замечания к отчёту',exact=False)
                quality.click()
                wait_for_app_state(page,
                    '!document.querySelector("#reportQualitySource")?.hidden',
                    'Saved report issues load the native source link')
                assert page.locator('#reportQualityDialog').is_visible()
                page.locator('#reportQualitySource').click()
                wait_for_app_state(page,
                    'document.querySelector("#reportQualityStatus")?.textContent.startsWith("На странице:") || '
                    'document.querySelector("#reportQualityStatus")?.textContent.startsWith("Сохранённых замечаний нет")',
                    'Saved shared source issues through Tauri IPC')
                assert source_anchor['id'] in page.locator('#reportQualityAnchor').inner_text()
                assert page.locator('#reportQualityRows tr').count()==min(50,len(source_anchor['sourceIssues']))
                page.keyboard.press('Escape')
                assert not page.locator('#reportQualityDialog').is_visible()
                assert quality.evaluate('(button) => document.activeElement === button')
                report['checks'].append('Saved report quality, shared SQLite anchor, bounded rows and keyboard focus')
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
                # The lightweight launcher must go straight to SQLite on restart:
                # no full-history window.appReady and no legacy frontend bootstrap.
                wait_for_app_state(page, 'Boolean(window.nativeWorkspaceReady)', 'SQL-first application restart')
                page.evaluate('window.nativeWorkspaceReady')
                if page.evaluate("typeof window.appReady !== 'undefined'"):
                    raise RuntimeError('SQL-first startup loaded the classic history')
                assert page.locator('#metricReports').inner_text()=='366'
                assert page.locator('#sqlStartupPreference').is_checked()
                report['checks'].append('opt-in SQL-first startup avoids full report history')
                # Switch the next launch back to classic; imports must remain accessible.
                page.locator('#sqlStartupPreference').uncheck()
                assert page.evaluate("localStorage.getItem('analiz-sql-startup-v1')") is None
                page.locator('#legacyWorkspaceLink').click()
                wait_for_app_state(page, 'Boolean(window.appReady)', 'Classic interface after SQL-first restart')
                page.evaluate('window.appReady')
                assert len(page.evaluate('window.desktopAPI.load()')['reports'])==366
                restored=page.evaluate("({view:document.querySelector('.view.active').id,saved:localStorage.getItem('work-reports-view-v1')})")
                report['navigation_after_restart']=restored
                (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
                assert restored=={'view':'upload','saved':'upload'},restored
                report['checks'].append('SQLite and last view survive application restart')
                # Save an Excel rule through real WebView2 + Rust IPC, after all
                # classic autoimport assertions so the new rule cannot alter them.
                page.locator('#openNativeWorkspace').click()
                # Finish both independent SQL startup tasks before editing a
                # rule. Folder listing can finish before the bounded SQLite
                # bootstrap; a save during bootstrap cannot refresh the page.
                wait_for_app_state(page,'Boolean(window.nativeWorkspaceReady)',
                    'SQL workspace ready before rule edit')
                page.evaluate('window.nativeWorkspaceReady')
                wait_for_app_state(page,'Boolean(window.nativeFolderReady)', 'SQL folder rule editor startup')
                page.evaluate('window.nativeFolderReady')
                page.locator('[data-nav-link="excel"]').click()
                assert page.locator('#sqlSources').is_visible()
                # Re-establish the isolated test folder after the classic UI
                # round-trip, then await metadata instead of assuming it survived.
                page.evaluate('async folder => window.__TAURI__.core.invoke("set_folder", {folder})', str(tmp))
                page.locator('#sourceFolderRefresh').click()
                # Both synthetic-year.xlsx and metric-labor-demo.xlsx remain
                # in the test folder; target the intended source by name rather
                # than assuming the yearly workbook sorts first.
                wait_for_app_state(page,
                    'Array.from(document.querySelectorAll("#sourceFolderRows tr td:first-child")).some(td => td.textContent === "synthetic-year.xlsx")',
                    'SQL editor source metadata')
                year_row=page.locator('#sourceFolderRows tr').filter(has_text='synthetic-year.xlsx')
                assert year_row.count()==1
                year_row.locator('button').click()
                page.locator('#sourceRuleFormat').select_option('daily-wide')
                page.locator('#sourceRuleYear').fill('2024')
                page.evaluate('window.confirm = () => true')  # Test only; real UI requires confirmation.
                page.locator('#sourceRuleSave').click()
                wait_for_app_state(page,
                    'document.querySelector("#sourceRuleStatus")?.textContent.includes("Правило сохранено")',
                    'SQLite rule saved with revision check')
                listing=page.evaluate('window.desktopAPI.list()')
                assert listing['rules']['synthetic-year.xlsx']['format']=='daily-wide'
                assert listing['rules']['synthetic-year.xlsx']['year']==2024
                # Verify the persisted report count in SQLite, then await its
                # propagation to the SQL view. Do not silently accept a smaller
                # number or an incomplete asynchronous refresh.
                actual_reports=page.evaluate('window.desktopAPI.nativeSettingsBootstrap()')['reports']
                assert actual_reports==366,('SQLite report count changed after rule edit',actual_reports)
                wait_for_app_state(page,
                    'document.querySelector("#metricReports")?.textContent === "366"',
                    'SQL report count after editing rule')
                assert page.evaluate("typeof window.appReady === 'undefined'")
                report['checks'].append('SQL-only Excel rule write with native revision and file validation')
                # Validate Windows handle/path authorization without committing
                # any extra reports into the 366-report recovery fixture.
                auto_probe=tmp/'auto-probe.xlsx'
                auto_probe.write_bytes(daily_workbook_bytes())
                auto_request=page.evaluate('''async () => {
                  const listing=await window.desktopAPI.list();
                  const file=listing.files.find(f => f.name==='auto-probe.xlsx');
                  if(!file?.nativeSignature)throw Error('Native file signature is missing');
                  await window.desktopAPI.nativeSetRuleChecked(file.name,
                    {type:'day',start:'2025-01-01',sheet:'',format:'legacy'},
                    listing.revision,listing.folder,file.nativeSignature);
                  const current=await window.desktopAPI.list();
                  const source=current.files.find(f => f.name==='auto-probe.xlsx');
                  const session=await window.desktopAPI.nativeFolderImportSession(true);
                  return {session,folder:current.folder,revision:current.revision,
                    name:source.name,signature:source.nativeSignature};
                }''')
                try:
                    auto_review=page.evaluate('request => window.desktopAPI.nativeFolderImportPreview(request)',auto_request)
                    assert auto_review['reportCount']==6,auto_review
                    assert auto_review['conflicts']['new_reports']==6,auto_review
                finally:
                    page.evaluate('window.desktopAPI.nativeFolderImportSession(false)')
                try:
                    page.evaluate('''args => window.desktopAPI.nativeFolderImportApply(args.request,args.token)''',
                        {'request':auto_request,'token':auto_review['reviewToken']})
                except Exception as error:
                    assert 'выключен' in str(error) or 'сеанс' in str(error),str(error)
                else:
                    raise RuntimeError('Revoked folder session allowed a native commit')
                assert page.evaluate('window.desktopAPI.nativeSettingsBootstrap()')['reports']==366
                report['checks'].append('Checked native folder preview follows Windows handles; revoked session cannot commit')

                # P0.3: schedule recovery from a real Online Backup while the
                # installed app is running. It must not change the active DB.
                reviewed=page.evaluate(
                    'async source => window.__TAURI__.core.invoke("native_restore_preview",{source})',
                    str(snapshot))
                assert reviewed['reportCount']==366
                assert len(reviewed['verification'])==64
                requested=page.evaluate(
                    'async args => window.__TAURI__.core.invoke("native_restore_schedule",args)',
                    {'source':str(snapshot),'verification':reviewed['verification']})
                assert requested=={'reportCount':366,'restartRequired':True},requested
                still_live=page.evaluate('window.desktopAPI.load()')
                assert len(still_live['reports'])==366
                assert all(row.get('sourceFormat')=='daily-wide' for row in still_live['reports'])
                report['checks'].append('Restore scheduled without mutating running SQLite')
            except BaseException as error:
                diagnostics(error);raise
            finally:
                try:stop(process)
                finally:
                    if browser and browser.is_connected():browser.close()
            # The next real process starts with no SQLite handles from the
            # previous instance. The staged copy must be activated before
            # the WebView can read reports.
            process,_=start();browser=None
            try:
                browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227')
                page=wait_for_webview_page(browser)
                wait_for_app_state(page,'Boolean(window.appReady)','Restore startup after full close')
                page.evaluate('window.appReady')
                actual=page.evaluate('window.desktopAPI.load()')
                with closing(sqlite3.connect(str(snapshot))) as expected_db:
                    expected_reports=[
                        json.loads(row[0]) for row in expected_db.execute(
                            'SELECT json FROM reports ORDER BY period_start,id')
                    ]
                assert actual['reports']==expected_reports, 'Recovery did not restore backup reports'
                info=page.evaluate(
                    'window.__TAURI__.core.invoke("native_restore_launch_info")')
                assert info and info['restored'] is True,info
                assert 'Восстановление выполнено' in info['message']
                page.locator('#reportsShortcut').click()
                page.locator('#openNativeWorkspace').click()
                wait_for_app_state(page,'Boolean(window.nativeWorkspaceReady)',
                    'SQL workspace after recovered startup')
                page.locator('[data-nav-link="backup"]').click()
                assert page.locator('#sqlBackup').is_visible()
                wait_for_app_state(page,
                    "() => document.getElementById('restoreStatus')?.textContent.includes('Восстановление выполнено')",
                    'User-facing recovery confirmation')
                report['checks'].append(
                    'Installed Windows app restores SQLite on startup with retained backup and UI status')
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
