"""Run the packaged Tauri app through its real WebView2 and SQLite on an isolated Windows CI runner."""
import json,os,subprocess,sys,tempfile,time,urllib.request,shutil,hashlib,sqlite3
from pathlib import Path
from contextlib import contextmanager, closing
from playwright.sync_api import expect, sync_playwright, Error as PlaywrightError, TimeoutError as PlaywrightTimeoutError
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx
from test_import_controls import daily_workbook_bytes
sys.path.insert(0,str(ROOT/'tauri'/'examples'))
from make_metric_xlsx import create_example

def diagnostics(error):
    output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
    if (output/'windows-runtime.txt').exists():return
    (output/'windows-error.txt').write_text(f'{type(error).__name__}: {error}',encoding='utf-8')
    # Record software/runtime facts only; never dump process environments or full command lines.
    script=r"""
    [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $ErrorActionPreference='Continue'
    Get-Process analiz-rabot-tauri,msedgewebview2 -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,MainWindowTitle,MainWindowHandle,Responding | ConvertTo-Json
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

def record_secondary_failure(error, description, action):
    """Keep the original test failure when diagnostic capture or cleanup fails."""
    try:
        action()
    except Exception as followup:
        error.add_note(f'{description}: {type(followup).__name__}: {followup}')


class NativeConfirmations:
    """Drive only the installed test process's real, parented Win32 task dialogs."""

    automation_script=r"""
    [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $ErrorActionPreference='Stop'
    Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
    $processId=[int]$env:ANALIZ_SMOKE_DIALOG_PID
    $dialog=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr][long]$env:ANALIZ_SMOKE_DIALOG_HWND)
    if ($null -eq $dialog -or $dialog.Current.ProcessId -ne $processId) {
      throw 'UI Automation returned an unexpected dialog process'
    }
    $tree=[System.Windows.Automation.TreeScope]::Descendants
    $nodes=$dialog.FindAll($tree,[System.Windows.Automation.Condition]::TrueCondition)
    $inventory=@()
    $body=@()
    foreach ($node in $nodes) {
      $info=$node.Current
      if ($info.ProcessId -ne $processId) {continue}
      $type=$info.ControlType.ProgrammaticName
      $inventory += [PSCustomObject]@{type=$type;name=$info.Name;automationId=$info.AutomationId;enabled=$info.IsEnabled}
      if ($type -ne 'ControlType.Button' -and $info.AutomationId -notlike 'CommandButton_*') {
        $body += $info.Name
        $body += $info.HelpText
      }
    }
    $text=[regex]::Replace(($body -join ' '),'\s+',' ').Trim()
    [PSCustomObject]@{matched=$text.Contains($env:ANALIZ_SMOKE_DIALOG_TEXT); controls=$inventory} |
      ConvertTo-Json -Depth 4 -Compress
    """

    def __init__(self, pid, database):
        import ctypes
        from ctypes import wintypes

        self.pid=pid
        self.database=database
        self.ctypes=ctypes
        self.user32=ctypes.WinDLL('user32',use_last_error=True)
        self.enum_callback=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
        signatures={
            'EnumWindows':(wintypes.BOOL,self.enum_callback,wintypes.LPARAM),
            'GetWindow':(wintypes.HWND,wintypes.HWND,wintypes.UINT),
            'GetWindowThreadProcessId':(wintypes.DWORD,wintypes.HWND,ctypes.POINTER(wintypes.DWORD)),
            'GetClassNameW':(ctypes.c_int,wintypes.HWND,wintypes.LPWSTR,ctypes.c_int),
            'IsWindowVisible':(wintypes.BOOL,wintypes.HWND),
            'IsWindowEnabled':(wintypes.BOOL,wintypes.HWND),
            'SendMessageTimeoutW':(ctypes.c_ssize_t,wintypes.HWND,wintypes.UINT,wintypes.WPARAM,
                                    wintypes.LPARAM,wintypes.UINT,wintypes.UINT,ctypes.POINTER(ctypes.c_size_t)),
        }
        for name,(result,*arguments) in signatures.items():
            function=getattr(self.user32,name)
            function.restype=result
            function.argtypes=arguments
        self.pid_type=wintypes.DWORD

    def windows(self):
        windows=[]
        @self.enum_callback
        def collect(hwnd, _):
            windows.append(hwnd)
            return True
        if not self.user32.EnumWindows(collect,0):
            raise self.ctypes.WinError(self.ctypes.get_last_error())
        return windows

    def process(self, hwnd):
        if not hwnd:return 0
        pid=self.pid_type()
        self.user32.GetWindowThreadProcessId(hwnd,self.ctypes.byref(pid))
        return pid.value

    def window_class(self, hwnd):
        buffer=self.ctypes.create_unicode_buffer(256)
        self.user32.GetClassNameW(hwnd,buffer,len(buffer))
        return buffer.value

    def inspect(self, dialog, prompt):
        # The dialog plugin enables rfd common-controls-v6. TaskDialogIndirect
        # exposes its content and buttons through UI Automation.
        result=subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',self.automation_script],
            env=dict(os.environ,ANALIZ_SMOKE_DIALOG_PID=str(self.pid),ANALIZ_SMOKE_DIALOG_HWND=str(dialog),
                     ANALIZ_SMOKE_DIALOG_TEXT=prompt),
            capture_output=True,encoding='utf-8',errors='replace',timeout=15)
        if result.returncode:
            raise RuntimeError(f'Native dialog UI Automation failed ({result.returncode}): {result.stderr.strip()}')
        return json.loads(result.stdout)

    def find_dialog(self, expected):
        deadline=time.monotonic()+15
        owned=[]
        while time.monotonic()<deadline:
            owned=[];matches=[]
            for hwnd in self.windows():
                if self.process(hwnd)!=self.pid:
                    continue
                owner=self.user32.GetWindow(hwnd,4)  # GW_OWNER
                facts={'hwnd':hwnd,'class':self.window_class(hwnd),'owner':owner,'owner_pid':self.process(owner),
                       'visible':bool(self.user32.IsWindowVisible(hwnd)),'enabled':bool(self.user32.IsWindowEnabled(hwnd))}
                owned.append(facts)
                if facts['class']!='#32770' or not facts['visible'] or facts['owner_pid']!=self.pid:
                    continue
                facts['controls']=self.inspect(hwnd,expected)
                if facts['controls']['matched']:
                    matches.append((hwnd,owner))
            if matches:
                assert len(matches)==1,('Multiple owned dialogs match',expected,owned)
                return matches[0]
            time.sleep(.05)
        raise AssertionError(f'Native confirmation did not appear for {expected!r}; owned windows and controls: {owned}')

    def answer(self, dialog, owner, prompt, button_id):
        expected_label={1:'OK',2:'Cancel'}[button_id]
        result=self.inspect(dialog,prompt)
        commands=[control for control in result['controls']
                  if control['automationId']==f'CommandButton_{button_id}']
        assert (result['matched'] and len(commands)==1 and commands[0]['name']==expected_label
                and commands[0]['enabled'] is True),('Unexpected native dialog command',expected_label,result)
        assert self.process(dialog)==self.pid and self.user32.GetWindow(dialog,4)==owner
        assert self.process(owner)==self.pid and not self.user32.IsWindowEnabled(owner)
        assert (self.window_class(dialog)=='#32770' and self.user32.IsWindowVisible(dialog)
                and self.user32.IsWindowEnabled(dialog)),result
        # Documented Task Dialog command with a numeric button ID and no pointer data.
        tdm_click_button=0x0400+102  # WM_USER + 102
        message_result=self.ctypes.c_size_t()
        if not self.user32.SendMessageTimeoutW(dialog,tdm_click_button,button_id,0,0x0002,3_000,
                                               self.ctypes.byref(message_result)):
            raise RuntimeError(f'Task Dialog did not accept {expected_label}: {result}') from self.ctypes.WinError(
                self.ctypes.get_last_error())
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            if not self.user32.IsWindowVisible(dialog) and self.user32.IsWindowEnabled(owner):
                return
            time.sleep(.05)
        raise RuntimeError(f'Native confirmation stayed open after {expected_label}: {result}')

    def revision(self):
        with closing(sqlite3.connect(self.database.as_uri()+'?mode=ro',uri=True)) as database:
            return database.execute('SELECT revision FROM storage_revision WHERE singleton=1').fetchone()[0]

    def cancel_then_accept(self, trigger, prompt):
        revision=self.revision()
        for button_id in (2,1):  # IDCANCEL then IDOK
            trigger.click()
            dialog,owner=self.find_dialog(prompt)
            assert self.revision()==revision,('Storage changed before native confirmation',prompt)
            self.answer(dialog,owner,prompt,button_id)
            if button_id==2:
                expect(trigger).to_be_enabled(timeout=30_000)
                assert self.revision()==revision,('Cancelled native confirmation changed storage',prompt)


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

def navigate(page, section):
    """Open a canonical tab, using the same menu as an installed-app user."""
    tab=page.locator(f'.tabs .tab[data-view="{section}"]')
    if not tab.is_visible():
        page.locator('#managementToggle').click()
    tab.click()
    assert page.locator(f'#{section}.view.active').is_visible()
    assert tab.get_attribute('aria-current')=='page'
    assert not page.evaluate('document.querySelector("#managementNav").open')


def api(page, method, *args):
    return page.evaluate('({method,args}) => window.desktopAPI[method](...args)',
                         {'method':method,'args':args})


def invoke(page, command, args=None):
    return page.evaluate('({command,args}) => window.__TAURI__.core.invoke(command,args)',
                         {'command':command,'args':args or {}})


def assert_rejected(action, description, *messages):
    try:
        action()
    except PlaywrightError as error:
        if messages:
            assert any(message in str(error).lower() for message in messages),str(error)
    else:
        raise AssertionError(description)


def wait_for_text(page, selector, text, timeout_ms=30_000):
    wait_for_app_state(page,
        'document.querySelector('+json.dumps(selector)+')?.textContent.includes('+json.dumps(text)+')',
        f'{selector} contains {text}',timeout_ms)


def ready(page, count):
    wait_for_app_state(page,'Boolean(window.appReady)', 'Unified SQLite startup',90_000)
    assert page.evaluate('window.appReady') is True
    assert page.evaluate('''() => {
      return ['/','/index.html'].includes(location.pathname)
        && document.querySelector('meta[name="analiz-unified-sql"]')?.content==='true'
        && typeof window.__TAURI__.core.invoke==='function'
        && !('load' in window.desktopAPI) && !('save' in window.desktopAPI)
        && Object.isFrozen(window.unifiedOriginal.state.reports)
        && window.unifiedOriginal.state.reports.length===0
        && !document.querySelector('#legacyWorkspaceLink,#openNativeWorkspace,#sqlStartupPreference');
    }''')
    assert page.locator('#dashboard.view.active').is_visible()
    assert api(page,'nativeSettingsBootstrap')['reports']==count
    wait_for_text(page,'#unifiedIndexStatus',f'Отчётов: {count} · Локальная база SQLite',90_000)


def check_runtime(page):
    expected_version=json.loads((ROOT/'tauri/package.json').read_text(encoding='utf-8'))['version']
    wait_for_text(page,'#appVersion','Версия '+expected_version)
    navigate(page,'storage')
    page.locator('#appearanceTheme').select_option('dark')
    page.locator('#appearancePalette').select_option('paper')
    page.locator('#appearanceDesign').select_option('compact')
    page.locator('#appearanceSize').select_option('large')
    assert page.evaluate('''() => {
      const {theme,palette,design,uiSize}=document.documentElement.dataset;
      return [theme,palette,design,uiSize].join('/')==='dark/paper/compact/large';
    }''')
    page.locator('#appearanceTheme').select_option('system')
    page.locator('#appearancePreset').select_option('minimal')
    page.locator('#appearanceSize').select_option('standard')
    navigate(page,'dashboard')
    menu=page.locator('#managementToggle')
    menu.focus();menu.press('Enter')
    assert page.locator('#managementNav').evaluate('el => el.open')
    menu.press('Escape')
    assert not page.locator('#managementNav').evaluate('el => el.open')
    assert menu.evaluate('el => document.activeElement===el')
    select=page.locator('#periodType')
    select.focus();select.press('Enter');select.press('Escape')
    assert select.evaluate('el => document.activeElement===el')
    dropdown=page.locator('#categoryFilter > summary')
    dropdown.focus();dropdown.press('Enter')
    assert page.locator('#categoryFilter').evaluate('el => el.open')
    dropdown.press('Enter')
    assert not page.locator('#categoryFilter').evaluate('el => el.open')


def set_folder(page, folder, filenames):
    invoke(page,'set_folder',{'folder':str(folder)})
    page.locator('#sourceFolderRefresh').click()
    expected=json.dumps(sorted(filenames),ensure_ascii=False,separators=(',',':'))
    wait_for_app_state(page,
        'JSON.stringify(Array.from(document.querySelectorAll("#sourceFolderRows tr td:first-child"),x=>x.textContent).sort()) === '+json.dumps(expected),
        'Source metadata follows the connected Excel folder')
    wait_for_text(page,'#sourceFolderLocation',folder.name)
    listing=api(page,'list')
    assert set(file['name'] for file in listing['files'])==set(filenames)
    return listing


def import_year(page, workbook, confirmations):
    page.get_by_role('button',name='Показать файлы папки',exact=True).click()
    page.locator('#sourceManualFile').select_option(label=workbook.name)
    page.get_by_role('button',name='Открыть для проверки',exact=True).click()
    wait_for_text(page,'#excelFileStatus',workbook.name)
    page.locator('#importFormat').select_option('daily-wide')
    page.locator('#dailyYear').fill('2024')
    page.locator('#inspectFile').click()
    wait_for_text(page,'#unifiedImportStatus','Проверка завершена',90_000)
    assert page.locator('#importPreview .kpi').first.locator('strong').inner_text()=='366'
    assert page.locator('#importPreview tbody tr').count()<=50
    assert api(page,'nativeSettingsBootstrap')['reports']==0
    page.locator('#unifiedAcceptQuality').check()
    confirmations.cancel_then_accept(page.locator('#saveImport'),
        'Повторно проверить Excel и атомарно сохранить 366 отчётов')
    wait_for_text(page,'#unifiedImportStatus','Сохранено: 366 отчётов',120_000)
    wait_for_text(page,'#unifiedIndexStatus','Отчётов: 366',90_000)
    assert api(page,'nativeSettingsBootstrap')['reports']==366
    assert page.evaluate('window.unifiedOriginal.state.reports.length===0')


def check_report_views(page):
    page.locator('#uploadedReports > summary').click()
    wait_for_text(page,'#unifiedReportsPage','1–50 из 366')
    rows=page.locator('#reports .calendar-source')
    assert rows.count()==50
    first=rows.first.locator('strong').inner_text()
    assert '2024-12-31' in first
    page.locator('#unifiedReportsNext').click()
    wait_for_text(page,'#unifiedReportsPage','51–100 из 366')
    assert rows.count()==50
    assert rows.first.locator('strong').inner_text()!=first
    page.locator('#unifiedReportsPrev').click()
    wait_for_text(page,'#unifiedReportsPage','1–50 из 366')
    assert rows.first.locator('strong').inner_text()==first
    snapshot=api(page,'nativeSettingsBootstrap')
    daily=api(page,'managementReportSummary',snapshot['revision'],{'report_type':'day'})
    assert daily['reports']==366 and abs(daily['amount']-7320)<.001,daily
    assert api(page,'managementReportPage',snapshot['revision'],{'report_type':'week'},None,50)==[]
    wait_for_app_state(page,'document.querySelectorAll("#reportCalendar > button").length===12','Calendar loaded')
    year=int(page.locator('#calendarRange').inner_text().split()[0])
    for current in range(year,2024,-1):
        page.locator('#calendarPrev').click()
        wait_for_app_state(page,
            f'document.querySelector("#calendarRange").textContent.startsWith("{current-1} год") && '
            'document.querySelectorAll("#reportCalendar > button").length===12', 'Previous calendar year',60_000)
    assert page.locator('#reportCalendar > button').count()==12
    month=page.locator('#reportCalendar [data-month="2024-02"]')
    month.click()
    leap=page.locator('#calendarDetails .calendar-day[aria-label^="2024-02-29:"]')
    assert page.locator('#calendarDetails .calendar-day').count()==29
    assert leap.count()==1
    assert 'Дней: 29 · Пропусков: 0' in month.inner_text()
    leap.click()
    detail=page.locator('#calendarDetails .calendar-day-details')
    assert detail.get_by_role('heading',name='2024-02-29',exact=True).is_visible()
    detail.locator('.calendar-source').wait_for()
    assert detail.locator('.calendar-source').count()==1
    assert '2024-02-29' in detail.locator('.calendar-source strong').inner_text()
    detail.get_by_role('button',name='Закрыть подробности дня',exact=True).click()
    assert leap.evaluate('el => document.activeElement===el')
    page.get_by_role('button',name='Закрыть подробности месяца',exact=True).click()
    assert month.evaluate('el => document.activeElement===el')


def check_metrics(page, temporary):
    navigate(page,'upload')
    page.locator('#additionalMetrics > summary').click()
    page.locator('#metricKindSelect option').first.wait_for(state='attached')
    assert page.locator('#metricKindSelect option').count()==2
    expect(page.locator('#metricKindChooseXlsx')).to_be_enabled()
    assert len(api(page,'nativeMetricKinds'))==2
    example=ROOT/'tauri/examples/labor-report.json'
    metric=api(page,'nativeMetricPreview',str(example))
    assert metric['kind']=='labor' and metric['rowCount']==2
    assert metric['values']['hours']==12 and abs(metric['values']['units_per_hour']-19/12)<1e-8
    assert api(page,'nativeMetricApply',str(example),metric['reviewToken'])['rowCount']==2
    assert_rejected(lambda:api(page,'nativeMetricApply',str(example),metric['reviewToken']),
                    'Generic metric report duplicate was accepted')
    metric_xlsx=temporary/'metric-labor-demo.xlsx'
    create_example(metric_xlsx)
    day='2026-11-18'
    arguments=[str(metric_xlsx),'labor_simple',day,day]
    mapped=api(page,'nativeMetricXlsxPreview',*arguments)
    assert mapped['rowCount']==2 and mapped['values']['hours']==12 and mapped['values']['units']==19
    assert api(page,'nativeMetricXlsxApply',*arguments,mapped['reviewToken'])['rowCount']==2
    summary=api(page,'nativeMetricSummary','labor',day,day)
    assert summary['reportCount']==2 and summary['values']['hours']==24 and summary['values']['units']==38
    assert abs(summary['values']['units_per_hour']-38/24)<1e-8
    assert_rejected(lambda:api(page,'nativeMetricXlsxApply',*arguments,mapped['reviewToken']),
                    'Metric XLSX mapping accepted a repeated source')


def dashboard_month(page):
    navigate(page,'dashboard')
    page.locator('#unifiedSource').select_option('daily')
    page.locator('#periodType').select_option('month')
    wait_for_app_state(page,'document.getElementById("overviewPeriod").type==="month"','Monthly period input')
    page.locator('#overviewPeriod').fill('2024-02')
    wait_for_app_state(page,
        'window.unifiedUI.scope?.from==="2024-02-01" && window.unifiedUI.scope?.to==="2024-02-29" '
        '&& document.querySelectorAll("#overviewKpis .kpi").length===4','February SQLite dashboard')
    wait_for_text(page,'#overviewKpis',page.evaluate('window.unifiedUI.fmt(580,"amount")'))


def check_analytics(page, temporary, output):
    dashboard_month(page)
    snapshot=api(page,'nativeSettingsBootstrap')
    revision,orgs=snapshot['revision'],snapshot['org_names']
    assert len(orgs)==4,snapshot
    for start,end,unit,days,amount in (
        ('2024-01-01','2024-12-31','year',366,7320),
        ('2024-02-01','2024-02-29','month',29,580),
    ):
        rollup=api(page,'nativeDailyRollup',revision,start,end,unit,orgs,None,50)
        assert rollup['granularity']==unit and rollup['next_cursor'] is None,rollup
        assert len(rollup['points'])==1,rollup
        point=rollup['points'][0]
        # The observed CTE counts an_valid rows, keyed by (organization, day).
        assert point['start']==start and point['days']==days and point['available_reports']==days*len(orgs),rollup
        assert point['complete'] and point['missing']==0 and point['total']['amount']==amount,rollup
    absent=api(page,'nativeDailyRollup',revision,'2024-02-01','2024-02-29','month',orgs,None,50,'Отсутствующая работа')
    assert len(absent['points'])==1,absent
    point=absent['points'][0]
    assert point['days']==29 and point['available_reports']==29*len(orgs),absent
    assert point['complete'] and point['missing']==0 and point['total']['amount']==0,absent

    page.locator('#advancedAnalytics > summary').click()
    page.locator('#chartBreakdown').select_option('organizations')
    assert page.locator('[data-chart="line"]').count()==0
    page.locator('[data-chart="bar"]').click()
    wait_for_app_state(page,'document.querySelector("#chartsContainer svg")!==null','SQLite single-period department bar chart')
    page.locator('#chartOrientation').select_option('horizontal')
    wait_for_app_state(page,'document.querySelector("#chartsContainer .horizontal-bar-scroll svg")!==null','SQLite single-period horizontal bar chart')
    page.locator('#chartOrientation').select_option('vertical')
    wait_for_app_state(page,'document.querySelector("#chartsContainer .horizontal-bar-scroll")===null','SQLite restored vertical bar chart')
    organization=page.locator('#orgFilter input')
    assert organization.count()==4
    if not organization.first.is_visible():
        page.locator('#orgFilter summary').click()
    organization.nth(1).uncheck();organization.nth(3).uncheck()
    wait_for_app_state(page,
        'window.unifiedUI.scope?.organizations.length===2 && document.querySelectorAll("#overviewOrganizations .overview-unit").length===2',
        'Two-organization SQLite dashboard')
    wait_for_text(page,'#overviewKpis',page.evaluate('window.unifiedUI.fmt(290,"amount")'))
    organization.nth(1).check();organization.nth(3).check()
    wait_for_app_state(page,'window.unifiedUI.scope?.organizations.length===4','Restore all organizations')
    if page.locator('#orgFilter summary').count() and page.locator('#orgFilter summary').is_visible():
        page.locator('#orgFilter summary').click()
    page.locator('#advancedAnalytics > summary').click()

    # Verify the canonical dashboard at both desktop and phone CSS viewports.
    original=page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
    try:
        for width,height in ((1280,720),(1366,768),(390,844),(320,568)):
            page.set_viewport_size({'width':width,'height':height})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),(width,height)
            assert page.locator('#overviewKpis .kpi').count()==4
            page.screenshot(path=str(output/f'windows-sql-{width}x{height}.png'))
        page.set_viewport_size({'width':1280,'height':720})
        for scale in ('1.25','1.5','2'):
            geometry=page.evaluate('''scale => {
              const html=document.documentElement;
              html.style.zoom=scale;
              return {viewport:innerWidth,page:html.scrollWidth,scale,
                overview:document.querySelector('#overviewPanel').getBoundingClientRect().right,
                periods:document.querySelector('.period-toolbar').getBoundingClientRect().right};
            }''',scale)
            assert max(geometry['page'],geometry['overview'],geometry['periods'])<=geometry['viewport']+2,geometry
            if scale=='2':
                page.screenshot(path=str(output/'windows-sql-zoom-200.png'))
    finally:
        page.evaluate("document.documentElement.style.removeProperty('zoom')")
        page.set_viewport_size(original)

    # The real analysis commands must keep missing weeks out of all totals.
    start,end='2025-01-06','2025-01-13'
    summary=api(page,'nativeAnalysisRange',revision,'week',start,end,orgs)
    assert not summary['complete'] and summary['missing']>0 and summary['total']['amount'] is None
    timeline=api(page,'nativeAnalysisTimeline',revision,'week',start,end,orgs)
    assert len(timeline['points'])==2 and all(p['total']['amount'] is None for p in timeline['points'])
    trends=api(page,'nativeOrganizationTrends',revision,'week',start,end,orgs)
    assert len(trends['series'])==4 and all(p['total']['amount'] is None for s in trends['series'] for p in s['points'])
    rank=api(page,'nativeAnalysisRanking',revision,'week',start,end,orgs,None,'amount','all')
    assert not rank['complete'] and rank['rows']==[]
    range_page=api(page,'nativeAnalysisRangePage',revision,'week',start,end,orgs)
    assert range_page==[]
    comparison=api(page,'nativeAnalysisCompare',revision,'week',start,end,start,end,orgs)
    assert comparison['summary']['after']['total']['amount'] is None
    assert comparison['summary']['before']['total']['amount'] is None
    export_path=temporary/'analysis-empty-week.csv'
    args=[revision,'week',start,end,orgs,None,str(export_path)]
    api(page,'nativeExportRangeCsv',*args)
    exported=export_path.read_bytes()
    assert exported.startswith(b'\xef\xbb\xbf"key";"name";"kind";"count";"amount";"average";"active"')
    assert b'\r\n' in exported
    assert_rejected(lambda:api(page,'nativeExportRangeCsv',*args),'Native CSV export overwrote a file','already exists')
    assert export_path.read_bytes()==exported

    page.locator('#periodType').select_option('week')
    wait_for_app_state(page,'document.getElementById("overviewPeriod").type==="week"','Weekly period input')
    page.locator('#overviewPeriod').fill('2025-W02')
    wait_for_app_state(page,'window.unifiedUI.scope?.from==="2025-01-06"','Missing weekly range is selected')
    alerts=page.locator('#overviewAlerts')
    expect(alerts).to_contain_text('Неполные данные: 28 пропусков')
    expect(alerts.get_by_role('button',name='Понятно, замечание известно',exact=True)).to_have_count(0)
    amount=page.locator('#overviewKpis .kpi').first.locator('strong')
    expect(amount).to_have_text('—')
    details=alerts.locator('.coverage-details')
    details.locator('summary').focus()
    page.keyboard.press('Enter')
    expect(details.locator('.coverage-matrix tbody tr')).to_have_count(4)
    expect(details.locator('.coverage-matrix thead th')).to_have_count(8)
    expect(details.locator('td.coverage-missing')).to_have_count(28)
    expect(details.locator('td.coverage-ok')).to_have_count(0)
    expect(details.locator('.coverage-matrix thead')).to_contain_text('06.01.2025')
    expect(details.locator('.coverage-matrix thead')).to_contain_text('12.01.2025')
    details.locator('summary').press('Enter')
    expect(alerts).to_contain_text('Неполные данные: 28 пропусков')
    expect(amount).to_have_text('—')
    alerts.get_by_role('button',name='Проверить данные',exact=True).click()
    expect(page.locator('#upload')).to_have_class('view active')
    navigate(page,'dashboard')
    expect(alerts).to_contain_text('Неполные данные: 28 пропусков')
    expect(amount).to_have_text('—')
    page.screenshot(path=str(output/'windows-sql-persistent-coverage.png'))
    dashboard_month(page)


def check_extended(page):
    assert page.locator('#printSummary').is_enabled()
    page.emulate_media(media='print')
    try:
        assert page.evaluate('getComputedStyle(document.body).backgroundColor')=='rgb(255, 255, 255)'
        assert not page.locator('main').is_visible()
    finally:
        page.emulate_media(media='screen')
    page.locator('#advancedAnalytics > summary').click()
    wait_for_text(page,'#comparisonCoverage','Второй интервал: Полные данные',90_000)
    page.locator('#contributions > summary').click()
    wait_for_text(page,'#contributionCheck','совпадает',90_000)
    page.locator('#workViewMode').select_option('composition')
    expect(page.locator('#chartPanel')).to_be_hidden()
    expect(page.locator('#structurePanel')).to_be_visible()
    wait_for_text(page,'#structureResults','Итог:',90_000)
    page.locator('#workViewMode').focus()
    page.keyboard.press('ArrowUp')
    expect(page.locator('#workViewMode')).to_have_value('trend')
    expect(page.locator('#chartPanel')).to_be_visible()
    expect(page.locator('#structurePanel')).to_be_hidden()
    for control,result in (('usualPanel','usualCoverage'),('regularityPanel','regularityCoverage')):
        page.locator(f'#{control} > summary').click()
        wait_for_text(page,f'#{result}','Полных периодов:',90_000)
    navigate(page,'changes')
    page.locator('#changesType').select_option('month')
    page.locator('#changesTo').fill('2024-02')
    wait_for_text(page,'#changesPage','Страница 1',90_000)
    assert page.locator('#changesSummary .kpi').count()>=4
    assert page.locator('#changesResults tbody tr').count()<=100


def check_groups(page, confirmations):
    navigate(page,'groups')
    def create():
        page.locator('#createGroup').click()
        page.locator('#groupName').fill('Группа CI')
        work=page.locator('#groupEditor label.work-row').filter(has_text='Альфа')
        work.wait_for()
        assert work.count()==1
        work.locator('input').check()
        page.locator('#saveGroup').click()
        group=page.locator('#groupList details').filter(has_text='Группа CI')
        group.locator('summary').click(timeout=90_000)
        assert api(page,'nativeSettingsBootstrap')['memberships']==1
        return group
    group=create()
    member=group.locator('.group-member').filter(has_text='Альфа')
    member.locator('select').select_option('',timeout=90_000)
    wait_for_text(page,'#flash','Работа перенесена.',90_000)
    snapshot=api(page,'nativeSettingsBootstrap')
    assert snapshot['memberships']==0 and snapshot['groups']==0,snapshot
    assert group.count()==0
    # Removing the final member intentionally removes its source group.
    # Recreate it to verify the separate explicit-delete confirmation path.
    group=create()
    confirmations.cancel_then_accept(group.get_by_role('button',name='Удалить группу',exact=True),
        'Удалить группу «Группа CI»? Исходные работы останутся.')
    wait_for_text(page,'#flash','Группа удалена.',90_000)
    assert api(page,'nativeSettingsBootstrap')['groups']==0


def backup_contents(path):
    with closing(sqlite3.connect(path)) as database:
        assert database.execute('PRAGMA quick_check').fetchone()==('ok',)
        reports=database.execute('SELECT id,json FROM reports ORDER BY id').fetchall()
        settings=database.execute('SELECT json FROM kv WHERE name=\'settings\'').fetchone()
        metrics=database.execute('SELECT COUNT(*) FROM metric_report_docs').fetchone()[0]
    return reports,settings,metrics


def check_backup_and_replace(page, temporary, snapshot):
    revision=api(page,'nativeBackupSqlite',str(snapshot))
    assert isinstance(revision,int) and snapshot.is_file()
    original=backup_contents(snapshot)
    assert len(original[0])==366 and original[1] is not None and original[2]==2
    before=snapshot.read_bytes()
    assert_rejected(lambda:api(page,'nativeBackupSqlite',str(snapshot)),
                    'Native backup overwrote an existing SQLite file','already exists')
    assert snapshot.read_bytes()==before

    replacement=temporary/'replacement.xlsx'
    replacement.write_bytes(wide_xlsx(2024,works_per_org=2))
    original_file=replacement.read_bytes()
    orgs=api(page,'nativeSettingsBootstrap')['org_names']
    options={'year':2024,'sheets':[],'mappings':{},'organizations':orgs}
    preview=api(page,'nativeWideImportPreview',str(replacement),options)
    assert preview['reportCount']==366 and preview['rowCount']>0 and preview['conflicts']['replacements']==366
    token=preview['reviewToken']
    assert len(token)==64
    flags={'replaceExact':True,'acceptCrossType':True,'acceptQuality':True}
    assert_rejected(lambda:api(page,'nativeWideImportApply',str(replacement),options,token,flags),
                    'Native import replaced reports without a backup destination','backup')
    assert api(page,'nativeRevision')==revision
    before_replace=temporary/'before-native-import.sqlite3'
    receipt=api(page,'nativeWideImportApply',str(replacement),options,token,flags|{'backupDestination':str(before_replace)})
    assert backup_contents(before_replace)==original
    assert receipt['replaced']==366 and receipt['reports']==366 and receipt['revision']>preview['revision']
    assert replacement.read_bytes()==original_file
    assert page.evaluate('window.unifiedUI.refresh()')['reports']==366
    updated=api(page,'managementReportSummary',receipt['revision'],{'report_type':'day'})
    assert updated['reports']==366 and abs(updated['amount']-14640)<.001,updated
    return original


def check_folder_rule_and_revocation(page, workbook, confirmations):
    navigate(page,'upload')
    wait_for_app_state(page,'typeof window.nativeFolderReady!=="undefined"','Native folder controller startup')
    page.evaluate('window.nativeFolderReady')
    assert not page.locator('#sourceAutoImport').is_checked()
    listing=api(page,'list')
    assert {file['name'] for file in listing['files']}=={workbook.name}
    row=page.locator('#sourceFolderRows tr').filter(has_text=workbook.name)
    row.get_by_role('button',name='Настроить правило для '+workbook.name,exact=True).click()
    page.locator('#sourceRuleFormat').select_option('daily-wide')
    page.locator('#sourceRuleYear').fill('2024')
    confirmations.cancel_then_accept(page.locator('#sourceRuleSave'),
        f'Сохранить правило для «{workbook.name}»?')
    wait_for_text(page,'#sourceRuleStatus','Правило сохранено',90_000)
    rule=api(page,'list')['rules'][workbook.name]
    assert rule['format']=='daily-wide' and rule['year']==2024
    assert api(page,'nativeSettingsBootstrap')['reports']==366

    auto_probe=workbook.with_name('auto-probe.xlsx')
    auto_probe.write_bytes(daily_workbook_bytes())
    listing=api(page,'list')
    file=next(item for item in listing['files'] if item['name']==auto_probe.name)
    assert file['nativeSignature']
    api(page,'nativeSetRuleChecked',file['name'],{'type':'day','start':'2025-01-01','sheet':'','format':'legacy'},
        listing['revision'],listing['folder'],file['nativeSignature'])
    current=api(page,'list')
    source=next(item for item in current['files'] if item['name']==auto_probe.name)
    session=api(page,'nativeFolderImportSession',True)
    request={'session':session,'folder':current['folder'],'revision':current['revision'],
             'name':source['name'],'signature':source['nativeSignature']}
    try:
        review=api(page,'nativeFolderImportPreview',request)
        assert review['reportCount']==6 and review['conflicts']['new_reports']==6,review
    finally:
        api(page,'nativeFolderImportSession',False)
    assert_rejected(lambda:api(page,'nativeFolderImportApply',request,review['reviewToken']),
                    'A revoked folder session allowed a native commit','выключен','сеанс')
    assert api(page,'nativeSettingsBootstrap')['reports']==366
    auto_probe.unlink()

@contextmanager
def windows_test_workspace():
    """Release only this test's WebView2 workers before deleting their temporary profile."""
    workspace=Path(tempfile.mkdtemp(prefix='analiz-webview2-'))
    def cleanup_workspace():
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

    try:
        yield workspace
    except BaseException as error:
        record_secondary_failure(error,'Temporary WebView2 workspace cleanup failed',cleanup_workspace)
        raise
    else:
        cleanup_workspace()


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
                record_secondary_failure(error,'Runtime diagnostics failed',lambda:diagnostics(error))
                record_secondary_failure(error,'Installed process cleanup failed',lambda:force_stop(process))
                raise
        def force_stop(process):
            if process.poll() is not None:return
            stopped=subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],capture_output=True,timeout=15)
            if stopped.returncode!=0 and process.poll() is None:
                stopped.check_returncode()
            process.wait(timeout=15)
        def stop(process):
            if process.poll() is not None:
                raise RuntimeError(f'Application exited before a normal close could be requested: {process.returncode}')
            # Retry only a normal window close while Windows locates the main HWND.
            close_script=r"""
            [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
            $ErrorActionPreference='Stop'
            $deadline=(Get-Date).AddSeconds(8)
            do {
              $app=Get-Process -Id $env:ANALIZ_SMOKE_PID
              if($app.CloseMainWindow()){exit 0}
              Start-Sleep -Milliseconds 250
            } while((Get-Date) -lt $deadline)
            throw ('Application refused a normal close request (HWND {0}, responding {1})' -f $app.MainWindowHandle,$app.Responding)
            """
            closed=subprocess.run(['powershell','-NoProfile','-NonInteractive','-Command',close_script],
                env=dict(env,ANALIZ_SMOKE_PID=str(process.pid)),capture_output=True,encoding='utf-8',errors='replace',timeout=15)
            if closed.returncode:
                raise RuntimeError(f'Application window close failed ({closed.returncode}): {closed.stderr.strip()}')
            exit_code=process.wait(timeout=30)
            if exit_code!=0:
                raise RuntimeError(f'Application exited with status {exit_code} after the normal close request')
        with sync_playwright() as pw:
            @contextmanager
            def installed_page():
                process,engine=start();browser=page=None
                try:
                    browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227')
                    page=wait_for_webview_page(browser)
                    errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
                    yield page,engine,process.pid
                    assert not errors,errors
                    stop(process)
                    if browser.is_connected():browser.close()
                except BaseException as error:
                    if page and not page.is_closed():
                        record_secondary_failure(error,'WebView2 screenshot failed',lambda:page.screenshot(
                            path=str(output/'windows-webview2.png'),timeout=10_000))
                    record_secondary_failure(error,'Build progress report failed',lambda:(output/'build-info.json').write_text(
                        json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8'))
                    record_secondary_failure(error,'Runtime diagnostics failed',lambda:diagnostics(error))
                    record_secondary_failure(error,'Installed process cleanup failed',lambda:force_stop(process))
                    if browser and browser.is_connected():
                        record_secondary_failure(error,'DevTools disconnect failed',browser.close)
                    raise

            source_folder=tmp/'excel-sources';source_folder.mkdir()
            workbook=source_folder/'synthetic-year.xlsx';workbook.write_bytes(wide_xlsx(2024))
            original_xlsx=workbook.read_bytes()
            other_folder=tmp/'other-source';other_folder.mkdir()
            legacy_source=other_folder/'ordinary.xlsx';legacy_source.write_bytes(daily_workbook_bytes())
            snapshot=tmp/'online-backup.sqlite3'
            with installed_page() as (page,engine,pid):
                ready(page,0)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                entry_url=page.url
                check_runtime(page)
                report['webview2']=engine.get('Browser')
                report['customizableSelect']=page.evaluate("CSS.supports('appearance','base-select')")
                report['checks'].append('Installed canonical index starts on SQLite, reports runtime version and keyboard/appearance controls')
                navigate(page,'upload')
                wait_for_app_state(page,'typeof window.nativeFolderReady!=="undefined"','Native Excel folder controller startup')
                page.evaluate('window.nativeFolderReady')
                set_folder(page,source_folder,[workbook.name])
                set_folder(page,other_folder,[legacy_source.name])
                orgs=api(page,'nativeSettingsBootstrap')['org_names']
                sheets=api(page,'nativeLegacyImportSheets',str(legacy_source))
                assert 'xl/worksheets/sheet1.xml' in sheets,sheets
                preview=api(page,'nativeLegacyImportPreview',str(legacy_source),{
                    'sheet':'xl/worksheets/sheet1.xml','kind':'day','start':'2025-01-01','organizations':orgs})
                assert preview['reportCount']==6 and len(preview['reviewToken'])==64 and preview['sampleReports']
                assert api(page,'nativeSettingsBootstrap')['reports']==0
                set_folder(page,source_folder,[workbook.name])
                assert not page.locator('#sourceAutoImport').is_checked()
                assert page.evaluate('window.unifiedUI.refresh()')['reports']==0
                report['checks'].append('Changing the connected folder replaces metadata without importing; ordinary XLSX preview streams through Rust')
                import_year(page,workbook,confirmations)
                assert workbook.read_bytes()==original_xlsx
                check_report_views(page)
                report['checks'].append('Native Excel confirmation blocks writes, Cancel preserves SQLite, and OK imports 366 reports with 50-card pagination and a 29-day calendar')
                check_analytics(page,tmp,output)
                report['checks'].append('Daily-source annual and February totals, exact work and organization filters, missing-week analytics, 200% zoom and responsive widths, and protected UTF-8 CSV')
                check_extended(page)
                report['checks'].append('Canonical changes, usual level, regularity, structure and contribution views use the current SQLite selection; print uses the dedicated layout')
                check_groups(page,confirmations)
                report['checks'].append('Group creation, member assignment and removal, and native Cancel/OK for explicit deletion')
                check_metrics(page,tmp)
                report['checks'].append('Installed native metric imports and ratio-of-sums queries refuse repeated JSON and XLSX sources')
                original=check_backup_and_replace(page,tmp,snapshot)
                assert workbook.read_bytes()==original_xlsx
                report['checks'].append('SQLite online backup quick-check, no overwrite, and atomic 366-report replacement with a verified pre-import backup')
                navigate(page,'upload')
                wait_for_text(page,'#qualitySummary','Найдено:',90_000)
                assert page.locator('#qualityTable tbody tr').count()<=50
                assert page.locator('#uploadedReports > summary').is_visible()
                assert page.url==entry_url
                page.screenshot(path=str(output/'windows-webview2.png'))
                page.evaluate("localStorage.setItem('work-reports-view-v1','upload'); localStorage.setItem('analiz-sql-startup-v1','classic')")
                report['navigation_before_restart']=page.locator('.view.active').get_attribute('id')
                (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')

            with installed_page() as (page,_,pid):
                ready(page,366)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                report['navigation_after_restart']=page.locator('.view.active').get_attribute('id')
                assert report['navigation_before_restart']=='upload' and report['navigation_after_restart']=='dashboard'
                assert api(page,'list')['folder']
                check_folder_rule_and_revocation(page,workbook,confirmations)
                report['checks'].append('Normal restart preserves native SQLite and opens the canonical dashboard despite obsolete preferences; folder rule and revoked session are verified')
                data_file=Path(api(page,'info')['dataFile'])
                safety_before=set(data_file.parent.glob('.analiz-restore-safety-*.sqlite3'))
                reviewed=api(page,'nativeRestorePreview',str(snapshot))
                assert reviewed['reportCount']==366 and len(reviewed['verification'])==64
                requested=api(page,'nativeRestoreSchedule',str(snapshot),reviewed['verification'])
                assert requested=={'reportCount':366,'restartRequired':True},requested
                verify_live=tmp/'still-live.sqlite3'
                api(page,'nativeBackupSqlite',str(verify_live))
                still_live=backup_contents(verify_live)
                assert len(still_live[0])==366 and still_live[0]!=original[0]
                assert api(page,'list')['rules'][workbook.name]['year']==2024
                report['checks'].append('Pending restore leaves the changed live SQLite and its Excel rules in place until restart')

            with installed_page() as (page,_,_):
                ready(page,366)
                restored=tmp/'after-restore.sqlite3'
                api(page,'nativeBackupSqlite',str(restored))
                assert backup_contents(restored)==original,'Recovery did not restore the backed-up reports, settings and metric data'
                assert workbook.name not in api(page,'list')['rules']
                retained=set(data_file.parent.glob('.analiz-restore-safety-*.sqlite3'))-safety_before
                assert len(retained)==1,'No unique protection copy for the replaced SQLite database'
                assert backup_contents(retained.pop())==still_live
                info=api(page,'nativeRestoreLaunchInfo')
                assert info and info['restored'] is True and 'Восстановление выполнено' in info['message'],info
                navigate(page,'storage')
                page.locator('#storage').get_by_text('Восстановление выполнено',exact=False).wait_for()
                assert workbook.read_bytes()==original_xlsx
                report['checks'].append('Installed restart restores exactly the SQLite backup, retains the replaced data and displays confirmation')
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False),flush=True)

if __name__=='__main__':
    try:run()
    except BaseException as error:
        record_secondary_failure(error,'Runtime diagnostics failed',lambda:diagnostics(error))
        raise
