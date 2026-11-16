"""Run the packaged Tauri app through its real WebView2 and SQLite on an isolated Windows CI runner."""
import json,os,subprocess,sys,tempfile,time,urllib.request,shutil,hashlib
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx

def diagnostics(error):
    output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
    if (output/'windows-runtime.txt').exists():return
    (output/'windows-error.txt').write_text(str(error),encoding='utf-8')
    # Record software/runtime facts only; never dump process environments or full command lines.
    script=r"""
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
    result=subprocess.run(['powershell','-NoProfile','-Command',script],env=env,capture_output=True,text=True,timeout=30)
    (output/'windows-runtime.txt').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')

def run():
    if sys.platform!='win32' or os.environ.get('GITHUB_ACTIONS')!='true':raise RuntimeError('Run only on an isolated Windows GitHub Actions runner')
    installers=list((ROOT/'tauri/src-tauri/target/release/bundle/nsis').glob('*.exe'))
    if len(installers)!=1:raise RuntimeError('Expected one freshly built installer')
    output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
    installer=output/'setup.exe';shutil.copy2(installers[0],installer)
    signature=Path(str(installers[0])+'.sig')
    if not signature.exists():raise RuntimeError('Signed preview installer is required')
    shutil.copy2(signature,output/'setup.exe.sig')
    report={'installer':str(installer.relative_to(ROOT)),'bytes':installer.stat().st_size,'sha256':hashlib.sha256(installer.read_bytes()).hexdigest(),'checks':[]}
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print('INSTALLER_PATH='+report['installer'],flush=True);print('INSTALLER_BYTES='+str(report['bytes']),flush=True)
    with tempfile.TemporaryDirectory(prefix='analiz-webview2-') as tmp:
        tmp=Path(tmp);destination=tmp/'installed'
        subprocess.run([str(installer),'/S','/D='+str(destination)],check=True,timeout=240)
        executables=list(destination.rglob('analiz-rabot-tauri.exe'))
        if len(executables)!=1:raise RuntimeError('Installed application executable not found')
        env=dict(os.environ,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9227',WEBVIEW2_USER_DATA_FOLDER=str(tmp/'webview'))
        env['NO_PROXY']=env.get('NO_PROXY','')+',127.0.0.1,localhost'
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
                diagnostics(error)
                if process.poll() is None:subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],check=False,capture_output=True)
                raise
        def stop(process):
            if process.poll() is None:subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],check=True,capture_output=True)
            process.wait(timeout=30)
        with sync_playwright() as pw:
            process,engine=start()
            try:
                browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227')
                page=browser.contexts[0].pages[0];errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
                page.wait_for_function('!!window.appReady');page.evaluate('window.appReady')
                assert page.evaluate('!!window.__TAURI__.core.invoke')
                data=page.evaluate('window.desktopAPI.load()')
                if data['reports']:raise RuntimeError('CI data folder is not empty; refusing to overwrite reports')
                report['webview2']=engine.get('Browser');report['customizableSelect']=page.evaluate("CSS.supports('appearance','base-select')");report['checks'].append('real WebView2 launch')
                page.locator('#reportsShortcut').click()
                excel=tmp/'synthetic-year.xlsx';excel.write_bytes(wide_xlsx(2024))
                page.locator('#excelFile').set_input_files(str(excel));page.wait_for_function('state.workbook!==null')
                page.locator('#inspectFile').click();page.wait_for_function('state.pending.length===366')
                page.locator('#saveImport').click();page.wait_for_function('state.reports.length===366')
                loaded=page.evaluate('window.desktopAPI.load()');assert len(loaded['reports'])==366
                assert abs(sum(r['amount'] for r in loaded['reports'])-7320)<.001
                report['checks'].append('Excel import and 366 reports persisted through native SQLite')
                page.locator('[data-view=dashboard]').click()
                page.locator('#periodType').click();page.keyboard.press('End');page.keyboard.press('Enter')
                assert page.locator('#periodType').input_value()=='day'
                page.locator('#orgFilter summary').click();checkbox=page.locator('#orgFilter input').first;was=checkbox.is_checked()
                checkbox.focus();page.keyboard.press('Space');assert page.locator('#orgFilter input').first.is_checked()!=was
                page.keyboard.press('Escape');report['checks'].append('native and multiple dropdown keyboard input')
                page.locator('#reportsShortcut').click()
                page.locator('#calendarYear').select_option('2024');page.locator('#calendarType').select_option('day')
                page.locator('[data-month="2024-02"]').click();assert page.locator('#calendarDetails button[data-date]').count()==29
                report['checks'].append('leap February calendar')
                page.screenshot(path=str(output/'windows-webview2.png'))
                assert not errors,errors
            except BaseException as error:
                diagnostics(error);raise
            finally:stop(process)
            process,_=start()
            try:
                browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227');page=browser.contexts[0].pages[0]
                page.wait_for_function('!!window.appReady');page.evaluate('window.appReady')
                assert len(page.evaluate('window.desktopAPI.load()')['reports'])==366
                assert page.locator('.view.active').get_attribute('id')=='upload'
                report['checks'].append('SQLite and last view survive application restart')
            except BaseException as error:
                diagnostics(error);raise
            finally:stop(process)
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False),flush=True)

if __name__=='__main__':
    try:run()
    except BaseException as error:
        diagnostics(error)
        raise
