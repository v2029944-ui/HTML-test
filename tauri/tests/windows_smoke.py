"""Run the packaged Tauri app through its real WebView2 and SQLite on an isolated Windows CI runner."""
import json,os,subprocess,sys,tempfile,time,urllib.request,shutil,hashlib,sqlite3,io,re,zipfile
from pathlib import Path
from datetime import date
from contextlib import contextmanager, closing
from playwright.sync_api import expect, sync_playwright, Error as PlaywrightError, TimeoutError as PlaywrightTimeoutError
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx
from test_import_controls import daily_workbook_bytes
sys.path.insert(0,str(ROOT/'tauri'/'examples'))
from make_metric_xlsx import create_example
from shared_period_controls import common_filters, common_period
from reference_control_layout import check_control_layout
from reference_smoke import check_reference
from reference_catalog import check_reference_catalog
from reference_import_flow import check_reference_import_flow
from reference_window_size import set_client_width
from reference_data_smoke import check_reference_data, choose_calendar_date, select_filter_period
from reference_gallery import write_reference_gallery
from reference_print_probe import probe_reference_print
from reference_print_changes import check_print_all_changes
from reference_file_dialogs import OwnedFileDialogs, check_reference_file_dialogs, check_reference_data_file_dialogs

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
        self.confirmation_cycles(trigger,prompt,accept=True)

    def cancel_only(self, trigger, prompt, drafts=None):
        self.confirmation_cycles(trigger,prompt,drafts=drafts)

    def confirmation_cycles(self, trigger, prompt, accept=False, drafts=None):
        revision=self.revision()
        before=drafts() if drafts else None
        closer=OwnedFileDialogs(self)
        output=ROOT/'tauri/test-artifacts';output.mkdir(exist_ok=True)
        digest=hashlib.sha256(prompt.encode('utf-8')).hexdigest()[:12]
        for index,action in enumerate(('cancel','close','escape')+(('accept',) if accept else ())):
            if index:
                expect(trigger).to_be_focused(timeout=30_000)
                trigger.press('Enter')
            else:
                trigger.click()
            dialog,owner=self.find_dialog(prompt)
            assert self.revision()==revision,('Storage changed before native confirmation',prompt)
            title=closer.title(dialog)
            screenshot=output/('native-file-task-'+digest+'-'+action+'.png')
            closer.verify_window(dialog,owner,title)
            closer.inspect(dialog,title,screenshot,require_cancel=False)
            bar=closer.titlebar_info(dialog)
            event={'prompt':prompt,'action':action,'screenshot':screenshot.name,
                   'revisionBefore':revision,'nativeTitlebar':bar}
            if action=='close' and bar['state']&(0x1|0x8000|0x10000):
                # Explicit native state is evidence of an unavailable control.
                # Unknown/zero bounds with an enabled state still fail below.
                event['outcome']='Close unavailable; dismissed using verified Cancel'
                self.answer(dialog,owner,prompt,2)
            elif action in ('close','escape'):
                if action=='close':
                    event['closeTarget']=closer.close_titlebar(dialog,owner,title,screenshot)
                    event['outcome']='Physical click on verified HTCLOSE'
                else:
                    event['escapeFocusHwnd']=closer.escape(dialog)
                    event['outcome']='SendInput VK_ESCAPE down/up'
                deadline=time.monotonic()+10
                while self.user32.IsWindowVisible(dialog) or not self.user32.IsWindowEnabled(owner):
                    if time.monotonic()>=deadline:
                        raise AssertionError('Native confirmation stayed open after '+action+': '+prompt)
                    time.sleep(.05)
            else:
                self.answer(dialog,owner,prompt,2 if action=='cancel' else 1)
                event['outcome']='Verified TaskDialog '+('Cancel' if action=='cancel' else 'OK')
            if action!='accept':
                expect(trigger).to_be_enabled(timeout=30_000)
                expect(trigger).to_be_focused(timeout=30_000)
                assert self.revision()==revision,('Cancelled native confirmation changed storage',prompt)
                if drafts:
                    assert drafts()==before,('Cancelled native confirmation changed drafts',prompt,action)
            event['revisionAfter']=self.revision()
            event['caption']=prompt+' — '+event['outcome']
            record=output/'native-file-dialogs-confirmations.json'
            events=json.loads(record.read_text(encoding='utf-8')) if record.exists() else []
            events.append(event)
            record.write_text(json.dumps(events,ensure_ascii=False,indent=2),encoding='utf-8')


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
    """Open a desktop section by the same visible buttons as the installed user."""
    key = 'overviewPanel' if section == 'dashboard' else section
    # The detailed toolbar no longer duplicates Summary and Adjacent History.
    # Keep the legacy summary SQL route available for deeper regression checks
    # while entering the preserved history through its real visible button.
    if key == 'overviewPanel':
        page.locator('#sharedHeader .main-nav button').first.click()
        page.evaluate("""() => window.referenceDesktop.openPanel(
            'overviewPanel', document.getElementById('refAnalytics'))""")
        expect(page.locator('#refNative')).to_have_attribute('data-panel','overviewPanel')
        expect(page.locator('#overviewPanel')).to_be_visible()
        return
    if key == 'changes':
        page.locator('#sharedHeader .main-nav button').nth(1).click()
        page.locator('#referenceAdjacentHistory').click()
        expect(page.locator('#refNative')).to_have_attribute('data-panel','changes')
        expect(page.locator('#changes')).to_be_visible()
        return
    tab = page.locator(f'.tool-tabs [data-section="{key}"]')
    if not tab.is_visible() and page.locator('#toolsPage').is_visible() and page.locator('#toolsPage .tool-tabs').is_hidden():
        # Old internal comparison/summary routes hide the five-tab strip.
        # Close that route before reopening the genuine navigation bar.
        page.locator('#backBtn').click()
    if not tab.is_visible():
        if key in ('upload', 'groups', 'databases', 'storage'):
            page.locator('#refImport' if key == 'upload' else '#refSettings').click()
        else:
            if not page.locator('#refAnalytics').is_visible():
                page.locator('#sharedHeader .main-nav button').first.click()
            page.locator('#refAnalytics').click()
    if tab.get_attribute('aria-pressed') != 'true':
        tab.click()
    expect(tab).to_have_attribute('aria-pressed', 'true')
    expect(page.locator('#refNative')).to_have_attribute('data-panel', key)
    expect(page.locator('#' + key)).to_be_visible()


def api(page, method, *args):
    return page.evaluate('({method,args}) => window.desktopAPI[method](...args)',
                         {'method':method,'args':args})


def invoke(page, command, args=None):
    # Raw fixture commands still need the identity enforced by Rust. Preserve an
    # explicit stale/invalid token so rejection tests cannot silently refresh it.
    payload=dict(args or {})
    if not command.startswith('shared_workspace_') and 'workspaceToken' not in payload:
        status=page.evaluate("window.__TAURI__.core.invoke('shared_workspace_status')")
        payload['workspaceToken']=status['workspace_token']
    return page.evaluate('({command,args}) => window.__TAURI__.core.invoke(command,args)',
                         {'command':command,'args':payload})


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


def ready(page, count, *, shared=False):
    wait_for_app_state(page,'Boolean(window.appReady)', 'Unified SQLite startup',90_000)
    assert page.evaluate('window.appReady') is True
    assert page.evaluate('''() => {
      return ['/','/index.html'].includes(location.pathname)
        && document.querySelector('meta[name="analiz-unified-sql"]')?.content==='true'
        && typeof window.__TAURI__.core.invoke==='function'
        && !('load' in window.desktopAPI) && !('save' in window.desktopAPI)
        && Object.isFrozen(window.unifiedOriginal.state.reports)
        && window.unifiedOriginal.state.reports.length===0
        && !document.querySelector('#legacyWorkspaceLink,#openNativeWorkspace,#sqlStartupPreference,#refData,#refWorkspaceDialog');
    }''')
    expect(page.locator('#s1')).to_be_visible()
    assert page.locator('#dashboard').evaluate("el=>el.classList.contains('active')")
    assert api(page,'nativeSettingsBootstrap')['reports']==count
    mode='Общая' if shared else 'Локальная'
    wait_for_text(page,'#unifiedIndexStatus',f'{mode} база SQLite. Отчётов: {count}',90_000)


def check_runtime(page):
    expected_version=json.loads((ROOT/'tauri/package.json').read_text(encoding='utf-8'))['version']
    wait_for_text(page,'#appVersion','Версия '+expected_version)
    navigate(page,'storage')
    expect(page.locator('#onlineUpdates #appVersion')).to_have_text('Версия '+expected_version)
    scale = page.locator('#appearanceSize')
    scale.select_option('standard')
    targets = page.locator('#refSettings, #checkAppUpdate')
    assert targets.count() == 2
    heights = 'nodes => nodes.map(node => node.getBoundingClientRect().height)'
    standard = targets.evaluate_all(heights)
    assert min(standard) > 0
    for percent in (90, 80):
        scale.select_option(str(percent))
        expect(page.locator('#refRoot')).to_have_attribute('data-ui-size', str(percent))
        assert page.evaluate("localStorage.getItem('work-reports-ui-size-v1')") == str(percent)
        for actual, baseline in zip(targets.evaluate_all(heights), standard):
            assert abs(actual - baseline * percent / 100) <= 1, (percent, actual, baseline)
    scale.select_option('standard')
    page.locator('#appearanceTheme').select_option('dark')
    page.locator('#appearancePalette').select_option('paper')
    page.locator('#appearanceDesign').select_option('compact')
    page.locator('#appearanceSize').select_option('large')
    assert page.evaluate('''() => {
      const {theme,palette,design,uiSize}=document.documentElement.dataset;
      return [theme,palette,design,uiSize].join('/')==='dark/paper/compact/large';
    }''')
    check_caption_colors(page)
    check_palette_contrast(page)
    page.locator('#appearanceTheme').select_option('system')
    page.locator('#appearancePreset').select_option('minimal')
    page.locator('#appearanceSize').select_option('standard')
    page.locator('#sharedHeader .main-nav button').first.click()
    navigate(page,'chartPanel')
    expect(page.locator('#backBtn')).to_be_hidden()
    analytics=page.locator('#toolsPage .tool-tabs [data-section="chartPanel"]')
    analytics.focus();analytics.press('Escape')
    expect(page.locator('#toolsPage')).to_be_visible()
    expect(analytics).to_be_focused()
    navigate(page,'dashboard')
    fold=common_filters(page)
    select=fold.locator('#referenceSource')
    select.focus();select.press('Enter');select.press('Escape')
    assert select.evaluate('el => document.activeElement===el')
    fold.locator(':scope > summary').click()
    dropdown=page.locator('#categoryFilter > summary')
    dropdown.focus();dropdown.press('Enter')
    assert page.locator('#categoryFilter').evaluate('el => el.open')
    dropdown.press('Enter')
    assert not page.locator('#categoryFilter').evaluate('el => el.open')


def check_caption_colors(page):
    if sys.getwindowsversion().build < 22000:
        return
    for theme, palette in (('light', 'neon'), ('dark', 'paper')):
        page.locator('#appearanceTheme').select_option(theme)
        page.locator('#appearancePalette').select_option(palette)
        expect(page.locator('#refRoot')).to_have_attribute('data-palette', palette)
        fill = page.locator('#sharedHeader .app-header').evaluate(
            "node => getComputedStyle(node).getPropertyValue('--window-caption-background').trim()")
        assert re.fullmatch(r'#[0-9a-f]{6}', fill, re.IGNORECASE), fill
        background = list(bytes.fromhex(fill[1:]))
        assert page.evaluate("""args => window.__TAURI__.core.invoke('set_window_caption', args)""",
                             {'background': background, 'dark': theme == 'dark'}) is True


def check_palette_contrast(page):
    pairs = [('--text', '--surface', 4.5), ('--muted', '--subtle', 4.5),
             ('--on-accent', '--accent-fill', 4.5), ('--on-accent', '--accent-hover', 4.5),
             ('--axis', '--surface', 3), ('--selected-border', '--selected', 3)]
    pairs.extend((f'--plot-{index}', '--surface', 3) for index in range(8))
    themed_pairs = [('--field-border', '--field-bg', 3), ('--warning', '--subtle', 4.5),
                    ('--text', '--selected', 4.5), ('--focus', '--surface', 3)]
    names = sorted({name for foreground, background, _ in pairs + themed_pairs for name in (foreground, background)})

    def luminance(color):
        assert re.fullmatch(r'#[0-9a-f]{6}', color, re.IGNORECASE), color
        channels = [int(color[index:index+2], 16) / 255 for index in (1, 3, 5)]
        linear = [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in channels]
        return sum(value * weight for value, weight in zip(linear, (0.2126, 0.7152, 0.0722)))

    for theme in ('light', 'dark'):
        page.locator('#appearanceTheme').select_option(theme)
        for palette in ('neon', 'glass', 'corporate', 'paper', 'industrial'):
            page.locator('#appearancePalette').select_option(palette)
            colors = page.evaluate('''names => {
              const style = getComputedStyle(document.documentElement);
              return Object.fromEntries(names.map(name => [name, style.getPropertyValue(name).trim()]));
            }''', names)
            checked_pairs = pairs if palette == 'corporate' else pairs + themed_pairs
            for foreground, background, minimum in checked_pairs:
                low, high = sorted([luminance(colors[foreground]), luminance(colors[background])])
                ratio = (high + 0.05) / (low + 0.05)
                assert ratio >= minimum, (palette, theme, foreground, background, ratio, minimum)


def set_folder(page, folder, filenames):
    page.locator('#uploadSourceFolder').click()
    invoke(page,'set_folder',{'folder':str(folder)})
    page.locator('#sourceFolderRefresh').click()
    expected=json.dumps(sorted(filenames)[:50],ensure_ascii=False,separators=(',',':'))
    wait_for_app_state(page,
        'JSON.stringify(Array.from(document.querySelectorAll("#sourceFolderRows tr td:first-child"),x=>x.textContent).sort()) === '+json.dumps(expected),
        'Source metadata follows the connected Excel folder')
    wait_for_text(page,'#sourceFolderLocation',folder.name)
    listing=api(page,'list')
    assert set(file['name'] for file in listing['files'])==set(filenames)
    return listing


def set_calendar_field(page, field_id, value):
    if field_id == 'dailyYear' and not page.locator('#unifiedYearDetails').evaluate('node=>node.open'):
        page.locator('#unifiedYearDetails > summary').click()
    field = page.locator('#' + field_id)
    trigger = page.locator('#' + field_id + 'Calendar')
    expect(field).to_be_hidden()
    if '-W' in value:
        year, week = value.split('-W')
        stamp = date.fromisocalendar(int(year), int(week), 1).isoformat()
    elif len(value) == 4:
        stamp = value + '-01-01'
    elif len(value) == 7:
        stamp = value + '-01'
    else:
        stamp = value
    choose_calendar_date(page, trigger, stamp)
    expect(field).to_have_value(value)


def import_year(page, workbook, confirmations):
    expect(page.locator('#unifiedImportAcknowledgements')).not_to_be_visible()
    expect(page.locator('#unifiedAcceptOverlap')).not_to_be_visible()
    expect(page.locator('#unifiedAcceptQuality')).not_to_be_visible()
    page.locator('#sourceFolderRows tr').filter(has_text=workbook.name).get_by_role('button',name='Открыть для проверки '+workbook.name,exact=True).click()
    wait_for_text(page,'#excelFileStatus',workbook.name)
    page.locator('#importType').select_option('day')
    if page.locator('#editImportLayout').is_visible():
        page.locator('#editImportLayout').click()
    page.locator('#importFormat').select_option('daily-wide')
    set_calendar_field(page, 'dailyYear', '2024')
    metadata=api(page,'nativeImportSheets',str(workbook))
    assert [sheet['name'] for sheet in metadata]==['Q1','Q2','Q3','Q4'],metadata
    assert page.locator('#sheetSelect option').all_text_contents()==['Q1','Q2','Q3','Q4']
    for i in range(4):
        page.locator(f'#unifiedWidePeriod{i}').select_option('q'+str(i+1))
        assert page.locator(f'#unifiedWideSheet{i}').is_checked()
    assert page.locator('#unifiedWideSheetMode').input_value()=='selected'
    summary = page.locator('#unifiedWideSheets')
    expect(summary).to_be_visible()
    expect(summary).to_have_value('Q1; Q2; Q3; Q4')
    expect(summary).to_have_attribute('readonly', '')
    assert summary.evaluate("node => getComputedStyle(node).borderRadius === getComputedStyle(document.querySelector('#unifiedWideSheetMode')).borderRadius")
    page.locator('#inspectFile').click()
    wait_for_text(page,'#unifiedImportStatus','Проверка завершена',90_000)
    assert page.locator('#importPreview .kpi').first.locator('strong').inner_text()=='366'
    assert page.locator('#importPreview tbody tr').count()<=50
    assert api(page,'nativeSettingsBootstrap')['reports']==0
    assert not page.locator('#saveDailyFolderRule').is_visible()
    check_control_layout(page)
    offered=[]
    for field,reason in [('unifiedAcceptOverlap','Проверьте даты и типы'),
                         ('unifiedAcceptQuality','замечаний качества')]:
        acknowledgement=page.locator('#'+field)
        expect(acknowledgement).not_to_be_checked()
        if acknowledgement.is_visible():
            label=acknowledgement.locator('xpath=parent::label')
            expect(label).to_contain_text(reason)
            assert re.search(r'\d+',label.inner_text()), 'Consent must explain the actual preview count'
            offered.append(field)
            acknowledgement.check()
    assert page.locator('#unifiedImportAcknowledgements').is_visible()==bool(offered)
    confirmations.cancel_then_accept(page.locator('#saveImport'),
        'Повторно проверить Excel и атомарно сохранить 366 отчётов')
    wait_for_text(page,'#unifiedImportStatus','Сохранено отчётов: 366',120_000)
    wait_for_text(page,'#unifiedIndexStatus','Отчётов: 366',90_000)
    assert api(page,'nativeSettingsBootstrap')['reports']==366
    assert page.evaluate('window.unifiedOriginal.state.reports.length===0')


def undated_quarters_xlsx():
    """Keep every daily quantity/amount pair; remove only its ISO date cell."""
    source=io.BytesIO(wide_xlsx(2024,quarter_sheets=True));output=io.BytesIO()
    with zipfile.ZipFile(source) as original,zipfile.ZipFile(output,'w',zipfile.ZIP_DEFLATED) as target:
        for entry in original.infolist():
            data=original.read(entry.filename)
            if entry.filename.startswith('xl/worksheets/'):
                xml=data.decode('utf-8')
                xml=re.sub(r'<c\b[^>]*><is><t>\d{4}-\d{2}-\d{2}</t></is></c>','',xml)
                data=xml.encode('utf-8')
            target.writestr(entry.filename,data)
    return output.getvalue()


def check_dated_quarters_preview(page,temporary):
    source=temporary/'dated-quarters.xlsx';source.write_bytes(wide_xlsx(2023,quarter_sheets=True))
    before=api(page,'nativeRevision');orgs=api(page,'nativeSettingsBootstrap')['org_names']
    options={'year':2023,'sheets':[],'mappings':{},'organizations':orgs}
    all_quarters=api(page,'nativeWideImportPreview',str(source),options)
    assert all_quarters['reportCount']==365,all_quarters
    second=api(page,'nativeWideImportPreview',str(source),options|{'sheets':['Q2']})
    assert second['reportCount']==91 and second['sampleReports'][0]['start']=='2023-04-01',second
    assert second['processedCellCount']==all_quarters['processedCellCount']>all_quarters['rowCount']
    assert api(page,'nativeRevision')==before


def clear_report_confirmation(page, confirmations, count, *, accept=False, drafts=None):
    trigger = page.locator('#clearReports')
    dialog = page.get_by_role('dialog', name='Удалить все импортированные отчёты?', exact=True)
    cancel = dialog.get_by_role('button', name='Отмена', exact=True)
    revision = confirmations.revision()
    before = drafts() if drafts else None
    actions = ['enter', 'cancel', 'close', 'escape']
    if accept:
        actions.append('delete')
    for index, action in enumerate(actions):
        if index:
            trigger.press('Enter')
        else:
            trigger.click()
        expect(dialog).to_be_visible()
        expect(cancel).to_be_focused()
        expect(dialog).to_contain_text('Личная база на этом компьютере.')
        expect(dialog).to_contain_text(f'Отчётов к удалению: {count}.')
        expect(dialog).to_contain_text('Группы останутся.')
        assert confirmations.revision() == revision
        if action in ('enter', 'escape'):
            page.keyboard.press('Enter' if action == 'enter' else 'Escape')
        elif action == 'cancel':
            cancel.click()
        else:
            dialog.get_by_role('button', name='Удалить отчёты' if action == 'delete' else 'Закрыть', exact=True).click()
        expect(dialog).to_have_count(0)
        if action != 'delete':
            expect(trigger).to_be_focused()
            assert confirmations.revision() == revision
            if drafts:
                assert drafts() == before


def check_cancelled_management_confirmations(page, confirmations):
    """Exercise only cancellation on the runner's populated synthetic database."""
    before=api(page,'nativeSettingsBootstrap')
    assert before['reports']>0
    revision=confirmations.revision()
    navigate(page,'storage')
    fields=page.locator('#organizationFields input')
    expect(fields).to_have_count(4)
    original=fields.first.input_value()
    fields.first.fill(original+' · отменяемый черновик CI')

    def drafts():
        return page.evaluate('''() => ({organizations:[...document.querySelectorAll('#organizationFields input')]
          .map(node=>node.value),restore:document.querySelector('#databaseRestoreStatus').textContent})''')

    confirmations.cancel_only(page.locator('#saveOrganizations'),
        'Изменить названия подразделений во всех отчётах и справочнике? '
        'Сначала будет создана резервная копия SQLite.',drafts=drafts)
    fields.first.fill(original)
    clear_report_confirmation(page,confirmations,before['reports'],drafts=drafts)
    for removed in ('Создать отдельную восстановленную копию',
                    'Восстановить при следующем запуске',
                    'Отменить запланированное восстановление'):
        expect(page.get_by_role('button', name=removed, exact=True)).to_have_count(0)
    navigate(page,'upload')
    page.locator('#unifiedUploadList').click()
    loaded=page.locator('#uploadedReports')
    if not loaded.evaluate('node=>node.open'):
        loaded.locator(':scope > summary').click()
    card=page.locator('#reports .calendar-source').first
    expect(card).to_be_visible(timeout=90_000)
    dates=re.search(r'· (\d{4}-\d{2}-\d{2}) — (\d{4}-\d{2}-\d{2})',card.locator('strong').inner_text())
    assert dates, 'Saved report card must expose its real period'
    confirmations.cancel_only(card.get_by_role('button',name='Удалить отчёт',exact=True),
        'Удалить отчёт '+dates[1]+' — '+dates[2]+'?')
    assert confirmations.revision()==revision
    after=api(page,'nativeSettingsBootstrap')
    assert after==before,('Cancelled management confirmations changed SQLite snapshot',before,after)


def check_report_views(page):
    page.locator('#unifiedUploadList').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','1–50 из 366')
    rows=page.locator('#reports .calendar-source')
    assert rows.count()==50
    first=rows.first.locator('strong').inner_text()
    assert '2024-12-31' in first
    pager=page.locator('[data-sql-pager="reports"]')
    expect(pager).to_be_visible()
    expect(page.locator('#unifiedReportsNext')).to_be_hidden()
    expect(page.locator('#unifiedReportsPrev')).to_be_hidden()
    names=lambda: rows.locator('strong').all_text_contents()
    first_names=names()
    pager.locator('[data-page-action="next"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','51–100 из 366')
    second_names=names()
    assert len(second_names)==50 and not set(first_names).intersection(second_names)
    pager.locator('[data-page-action="next"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','101–150 из 366')
    third_names=names()
    assert len(third_names)==50 and not set(first_names+second_names).intersection(third_names)
    pager.locator('[data-page-action="previous"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','51–100 из 366')
    assert names()==second_names
    pager.locator('[data-page-action="previous"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','1–50 из 366')
    assert names()==first_names and rows.first.locator('strong').inner_text()==first
    pager.locator('[data-page-action="choose"]').click()
    pager.locator('[data-page-number="8"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','351–366 из 366')
    assert rows.count()==16 and '2024-01-01' in rows.last.locator('strong').inner_text()
    pager.locator('[data-page-action="choose"]').click()
    pager.locator('[data-page-number="1"]').click()
    wait_for_text(page,'[data-sql-pager="reports"] [data-page-range]','1–50 из 366')
    assert names()==first_names
    snapshot=api(page,'nativeSettingsBootstrap')
    daily=api(page,'managementReportSummary',snapshot['revision'],{'report_type':'day'})
    assert daily['reports']==366 and abs(daily['amount']-7320)<.001,daily
    assert api(page,'managementReportPage',snapshot['revision'],{'report_type':'week'},None,50)==[]
    page.locator('#unifiedUploadCalendar').click()
    wait_for_app_state(page,'document.querySelectorAll("#reportCalendar > button").length===12','Calendar loaded')
    expect(page.locator('#calendarType')).to_be_hidden()
    expect(page.locator('#calendarYearSelect')).to_be_hidden()
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
    expect(detail.get_by_role('heading',name='Подразделения',exact=True)).to_be_visible()
    for organization in snapshot['org_names']:
        expect(detail.get_by_text(organization,exact=True)).to_be_visible()
    expect(detail.locator('summary').filter(has_text='Подразделения')).to_have_count(0)
    detail.get_by_role('button',name='Закрыть подробности дня',exact=True).click()
    assert leap.evaluate('el => document.activeElement===el')
    page.get_by_role('button',name='Закрыть подробности месяца',exact=True).click()
    assert month.evaluate('el => document.activeElement===el')
    month.click()
    leap.click()
    detail.get_by_role('button',name='Закрыть подробности дня',exact=True).focus()
    page.keyboard.press('Escape')
    expect(leap).to_be_focused()
    expect(detail.get_by_role('heading',name='2024-02-29',exact=True)).to_have_count(0)
    page.keyboard.press('Escape')
    expect(month).to_be_focused()
    expect(page.locator('#refNative')).to_have_attribute('data-panel','upload')
    expect(leap).to_have_count(0)


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
    common_period(page,'Месяц','2024-02-01',base='prevMonth')
    wait_for_app_state(page,
        'window.unifiedUI.scope?.from==="2024-02-01" && window.unifiedUI.scope?.to==="2024-02-29" '
        '&& document.querySelectorAll("#overviewKpis .kpi").length===4','February SQLite dashboard')
    wait_for_text(page,'#overviewKpis',page.evaluate('window.unifiedUI.fmt(580,"amount")'))



def source_evidence_xlsx():
    """One proven zero day and one blank leap day; no real user input."""
    rows=[['Работа','2020-02-28','2020-02-28','2020-02-29','2020-02-29','Итого за период','Итого за период'],
          ['', 'Количество','Сумма','Количество','Сумма','Количество','Сумма'],
          ['СМУ16',0,0,None,None,0,0],['Итого',0,0,None,None,0,0]]
    return quality_evidence_xlsx(rows)


def quality_evidence_xlsx(rows):
    xml=[]
    for row,values in enumerate(rows,1):
        cells=[]
        for col,value in enumerate(values):
            if value is None or value=='':continue
            address=chr(65+col)+str(row)
            cells.append(f'<c r="{address}"><v>{value}</v></c>' if isinstance(value,int) else f'<c r="{address}" t="inlineStr"><is><t>{value}</t></is></c>')
        xml.append(f'<row r="{row}">'+''.join(cells)+'</row>')
    files={'xl/worksheets/sheet1.xml':'<worksheet><sheetData>'+''.join(xml)+'</sheetData></worksheet>',
           'xl/workbook.xml':'<workbook xmlns:r="r"><sheets><sheet name="Проверка источника" r:id="r1"/></sheets></workbook>',
           'xl/_rels/workbook.xml.rels':'<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml" Type="worksheet"/></Relationships>'}
    output=io.BytesIO()
    with zipfile.ZipFile(output,'w',zipfile.ZIP_DEFLATED) as archive:
        for name,value in files.items():archive.writestr(name,value)
    return output.getvalue()


def open_daily_overview(page, stamp):
    # The shared period is reapplied when a new SQLite revision is published.
    page.locator('#sharedHeader .main-nav').get_by_role('button',name='Сводка',exact=True).click()
    filters=page.locator('#reportShell .filter-fold')
    if not filters.evaluate('node => node.open'):
        filters.locator(':scope > summary').click()
    select_filter_period(filters, 'День', 'single')
    choose_calendar_date(page,filters.locator('[data-date="start"]'),stamp)
    wait_for_app_state(page,'window.referenceFilters.scope?.displayFrom==='+json.dumps(stamp),'Shared day selected')
    navigate(page,'dashboard')
    wait_for_app_state(page,'window.unifiedUI.scope?.displayType==="day" && window.unifiedUI.scope?.from==='+json.dumps(stamp),'Daily SQLite overview')


def check_source_evidence(page, temporary, output):
    source=temporary/'zero-and-missing-day.xlsx';original=source_evidence_xlsx();source.write_bytes(original)
    snapshot=api(page,'nativeSettingsBootstrap');before=snapshot['reports']
    options={'year':2020,'sheets':[],'mappings':{},'organizations':snapshot['org_names']}
    preview=api(page,'nativeWideImportPreview',str(source),options)
    assert preview['reportCount']==1 and preview['rowCount']==0 and preview['conflicts']['new_reports']==1,preview
    assert preview['processedCellCount']==23,preview
    flags={'replaceExact':False,'acceptCrossType':False,'acceptQuality':False}
    assert_rejected(lambda:api(page,'nativeWideImportApply',str(source),options,preview['reviewToken'],flags),
                    'Source gaps bypassed explicit quality acknowledgment','quality')
    assert api(page,'nativeSettingsBootstrap')['reports']==before
    receipt=api(page,'nativeWideImportApply',str(source),options,preview['reviewToken'],flags|{'acceptQuality':True})
    assert receipt['reports']==1 and receipt['rows']==0 and source.read_bytes()==original,receipt
    assert page.evaluate('window.unifiedUI.refresh()')['reports']==before+1
    revision=receipt['revision']
    matrix=api(page,'nativeQualityMatrixPage',revision,'day','2020-02-28','2020-02-28',['СМУ16'])
    cell=matrix['rows'][0]['cells'][0]
    assert cell['available'] and cell['numeric_zero'] and not cell['has_activity'] and cell['warned_reports']==1,cell
    rows=api(page,'nativeQualityMatrixReports',revision,'day','2020-02-28','СМУ16',None,20)['rows']
    assert len(rows)==1,rows
    report_id=rows[0]['id']
    local=api(page,'nativeReportQualityPage',revision,report_id,'report',0,50)
    assert local['evidence']['explicit_zero'] is True and local['evidence']['sheet_name']=='Проверка источника',local
    assert local['evidence']['coverage']=={'start':'2020-02-28','end':'2020-02-29'},local
    shared=api(page,'nativeReportQualityPage',revision,report_id,'source',0,50)
    assert shared['evidence']['document_id']==report_id and shared['evidence']['missing_day_entries']==1,shared
    assert shared['evidence']['deferred_day_entries']==0 and shared['evidence']['omitted_row_entries']==0,shared
    gap=next(issue for issue in shared['issues'] if issue['kind']=='missing-day')
    assert gap['date']=='2020-02-29' and gap['organization'] is None and gap['relevant'] is True and gap['scope']=='source-period',gap
    open_daily_overview(page,'2020-02-28')
    details=page.locator('#overviewAlerts .coverage-details');details.locator(':scope > summary').click()
    expect(details.locator('td.coverage-warning')).to_have_count(1)
    expect(details.locator('td.coverage-warning')).to_contain_text('Нулевые значения')
    details.locator('td.coverage-warning .coverage-cell-action').click()
    inspection=details.locator('.coverage-inspection');inspection.locator('.coverage-report > details > summary').click()
    expect(inspection).to_contain_text('Сохранён признак явного нуля')
    expect(inspection).to_contain_text('Заявленный охват листа: 28.02.2020 — 29.02.2020')
    inspection.get_by_role('button',name='Замечания источника',exact=True).click()
    evidence=inspection.locator('.coverage-source-evidence')
    expect(evidence).to_contain_text('Записи о пропусках дней источника: 1')
    expect(evidence).to_contain_text('не устанавливает пропуск у выбранного подразделения')
    expect(evidence).not_to_contain_text('Сохранён признак явного нуля')
    expect(inspection.locator('.coverage-issue')).to_contain_text('Дата: 29.02.2020')
    inspection.screenshot(path=str(output/'windows-sql-source-evidence.png'))
    assert api(page,'nativeSettingsBootstrap')['revision']==revision,'Read-only evidence changed storage'
    assert source.read_bytes()==original,'Evidence flow rewrote the source XLSX'



def omitted_detail_xlsx():
    return quality_evidence_xlsx([
        ['Работа','Количество','Сумма'],['СМУ16',3,8],['Included',1,5],
        ['Rejected','bad',99],['',2,3],['АВТО','bad',7],
        ['Unknown organization',4,9],['Итого',3,8]])


def trend_zero_workbook(organizations):
    dates=['2019-01-01','2019-01-02','2019-01-04']
    rows=[['Работа',*[date for date in dates for _ in range(2)],'Итого за период','Итого за период'],
          ['',*(['Количество','Сумма']*4)]]
    for index,org in enumerate(organizations):
        values=[10,10,0,0,20,20,30,30] if index==0 else [0]*8
        rows.append([org,*values])
        if index==0:
            rows.append(['000 Проверка нуля',*values])
    rows.append(['Итого',10,10,0,0,20,20,30,30])
    return quality_evidence_xlsx(rows)


def check_filtered_zero_chart(page, temporary, output):
    snapshot=api(page,'nativeSettingsBootstrap')
    source=temporary/'trend-zero.xlsx'
    source.write_bytes(trend_zero_workbook(snapshot['org_names']))
    options={'year':2019,'sheets':[],'mappings':{},'organizations':snapshot['org_names']}
    preview=api(page,'nativeWideImportPreview',str(source),options)
    assert preview['reportCount']==3 and preview['conflicts']['new_reports']==3,preview
    # A date skipped by the Excel header is kept as a source-quality notice.
    assert preview['quality']['relevant_source_issues']==1,preview
    saved=api(page,'nativeWideImportApply',str(source),options,preview['reviewToken'],
              {'replaceExact':False,'acceptCrossType':False,'acceptQuality':True})
    assert saved['reports']==3,saved
    page.evaluate('window.unifiedUI.refresh()')
    open_daily_overview(page,'2019-01-01')
    page.locator('#sharedHeader .main-nav').get_by_role('button',name='Сводка',exact=True).click()
    filters=page.locator('#reportShell .filter-fold')
    if not filters.evaluate('n=>n.open'):
        filters.locator(':scope > summary').click()
    select_filter_period(filters, 'День', 'range')
    choose_calendar_date(page,filters.locator('[data-date="start"]'),'2019-01-01')
    choose_calendar_date(page,filters.locator('[data-date="end"]'),'2019-01-04')
    navigate(page,'chartPanel')
    page.locator('#chartBreakdown').select_option('works')
    page.locator('[data-reference-line]').click()
    toggle=page.locator('#hideEmptyPeriods')
    toggle.uncheck()
    chart=page.locator('#chartsContainer')
    expect(chart.locator('svg [data-chart-tooltip*="000 Проверка нуля"]').first).to_be_visible()
    expect(chart.locator('svg [data-chart-tooltip*="02.01.2019"]').first).to_be_visible()
    expect(chart.locator('svg [data-chart-tooltip*="03.01.2019"]').first).to_be_visible()
    toggle.check()
    gap=chart.locator('svg text').filter(has_text='Пропуск').first
    expect(gap).to_be_visible()
    expect(gap.locator('title')).to_contain_text('Нет данных за 03.01.2019')
    expect(gap.locator('title')).to_contain_text('Скрыты подтверждённые нулевые периоды: 02.01.2019')
    expect(chart.locator('svg [data-chart-tooltip*="02.01.2019"]')).to_have_count(0)
    expect(chart.locator('svg [data-chart-tooltip*="03.01.2019"]')).to_have_count(0)
    expect(chart.locator('svg polyline.chart-series')).to_have_count(0)
    chart.screenshot(path=str(output/'windows-sql-filtered-zero.png'))
    page.locator('#chartPanel [data-chart="table"]').click()
    table=page.locator('#dataTable table').first
    expect(table).to_contain_text('01.01.2019')
    expect(table).to_contain_text('04.01.2019')
    expect(table).not_to_contain_text('02.01.2019')
    expect(table).not_to_contain_text('03.01.2019')

    request=page.evaluate('window.unifiedUI.request(window.referenceFilters.scope)')
    target=temporary/'trend-filtered-zeros-and-absent.csv'
    api(page,'referenceTrendCsv',request,{'table':'works','metrics':['count'],'hideEmpty':True,'matchChart':True},str(target))
    assert target.read_text(encoding='utf-8-sig').splitlines()[0].endswith(';"2019-01-01";"2019-01-04"')

    # A single selected period cannot form a line, and responsive chart
    # controls must remain usable in every theme and compact UI size.
    page.locator('#chartPanel [data-reference-line]').click()
    if not filters.evaluate('node => node.open'):
        filters.locator(':scope > summary').click()
    select_filter_period(filters, 'День', 'single')
    page.wait_for_function("() => window.referenceFilters.scope?.overviewMode === 'single'", timeout=90_000)
    line=page.locator('#chartPanel [data-reference-line]')
    expect(line).to_be_hidden(timeout=90_000)
    expect(page.locator('#chartPanel [data-chart="bar"]')).to_have_attribute('aria-pressed', 'true')
    assert page.evaluate("window.unifiedUI.state.chart === 'bar'")

    # CDP-attached installed WebView2 can expose no Playwright viewport.
    viewport=page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
    initial=page.evaluate("""() => ({
        theme:document.documentElement.dataset.theme,size:document.documentElement.dataset.uiSize,
        rootTheme:document.getElementById('refRoot').dataset.theme,
        rootSize:document.getElementById('refRoot').dataset.uiSize
    })""")
    try:
        for width,height in ((390,844),(720,749),(1028,749),(1440,900)):
            page.set_viewport_size({'width':width,'height':height})
            for size in ('standard','90','80'):
                for theme in ('light','dark'):
                    page.evaluate('''options => {
                        const root=document.documentElement;
                        root.dataset.theme=options.theme;
                        root.dataset.uiSize=options.size;
                        const app=document.getElementById('refRoot');
                        app.dataset.theme=options.theme;
                        app.dataset.uiSize=options.size;
                    }''', {'theme':theme,'size':size})
                    geometry=page.evaluate('''() => {
                      const root=document.querySelector('#chartPanel');
                      const groups=['.chart-view-controls','.chart-mode-line',
                          '.chart-mode-controls','#chartsContainer','.chart-legend'];
                      const visible=node=>node&&node.getClientRects().length>0;
                      const overflow=groups.map(query=>{
                        const node=root.querySelector(query);
                        if(!visible(node)) return null;
                        // Horizontal SVG plotting regions intentionally scroll;
                        // their *outer* containers and controls must not.
                        return {query,width:node.clientWidth,scroll:node.scrollWidth,
                          right:node.getBoundingClientRect().right};
                      }).filter(Boolean);
                      return {page:document.documentElement.scrollWidth,
                        viewport:document.documentElement.clientWidth,
                        overflow,hiddenLine:!visible(root.querySelector('[data-reference-line]'))};
                    }''')
                    assert geometry['hiddenLine'] and geometry['page']<=geometry['viewport']+2, (width,size,theme,geometry)
                    for item in geometry['overflow']:
                        if item['query'] in ('.chart-view-controls','.chart-mode-line','.chart-mode-controls','.chart-legend'):
                            assert item['scroll']<=item['width']+2,(width,size,theme,item)
            # Capture the entire visible chart panel, including legend and plot,
            # not just the control strip. The screenshots are attached to CI.
            page.evaluate("""() => {
                document.documentElement.dataset.theme='light';
                document.documentElement.dataset.uiSize='standard';
                document.getElementById('refRoot').dataset.theme='light';
                document.getElementById('refRoot').dataset.uiSize='standard';
            }""")
            if width in (390,720,1440):
                page.locator('#chartPanel').screenshot(path=str(output/f'windows-chart-single-{width}.png'))
    finally:
        page.set_viewport_size(viewport)
        page.evaluate('''previous => {
            document.documentElement.dataset.theme=previous.theme;
            document.documentElement.dataset.uiSize=previous.size;
            const root=document.getElementById('refRoot');
            root.dataset.theme=previous.rootTheme;
            root.dataset.uiSize=previous.rootSize;
        }''',initial)
    select_filter_period(filters, 'День', 'range')
    page.wait_for_function("() => window.referenceFilters.scope?.overviewMode === 'range'", timeout=90_000)
    expect(line).to_be_visible(timeout=90_000)
    expect(line).to_have_attribute('aria-pressed', 'true')


def check_partial_amount_charts(page, temporary, output):
    snapshot=api(page,'nativeSettingsBootstrap')
    first,second,*_=snapshot['org_names']
    for day,first_amount,second_amount in [(1,20,30),(2,40,50)]:
        source=temporary/f'partial-amount-{day}.xlsx'
        source.write_bytes(quality_evidence_xlsx([
            ['Работа','Количество','Сумма'],
            [first,5,first_amount],['Сумма известна',2,first_amount],['Пустая сумма',3,None],
            [second,4,second_amount],['Сумма известна',4,second_amount],
        ]))
        options={'sheet':'xl/worksheets/sheet1.xml','kind':'day','start':f'2018-01-0{day}',
                 'organizations':snapshot['org_names']}
        preview=api(page,'nativeLegacyImportPreview',str(source),options)
        assert preview['reportCount']==1 and preview['rowCount']==3,preview
        result=api(page,'nativeLegacyImportApply',str(source),options,preview['reviewToken'],
                   {'replaceExact':False,'acceptCrossType':False,'acceptQuality':True})
        assert result['reports']==1 and result['rows']==3,result
    page.evaluate('window.unifiedUI.refresh()')
    open_daily_overview(page,'2018-01-01')
    filters=page.locator('#reportShell .filter-fold')
    if not filters.evaluate('node => node.open'):
        filters.locator(':scope > summary').click()
    select_filter_period(filters,'День','range')
    choose_calendar_date(page,filters.locator('[data-date="start"]'),'2018-01-01')
    choose_calendar_date(page,filters.locator('[data-date="end"]'),'2018-01-03')
    filters.locator('[data-measure]').select_option('amount')
    filters.locator('#sum-base').select_option('prev')
    wait_for_app_state(page,
        'window.unifiedUI.scope?.baseFrom==="2017-12-29" && window.unifiedUI.scope?.baseTo==="2017-12-31"',
        'Partial analytics baseline matches the missing three-day interval')
    navigate(page,'chartPanel')
    page.locator('#chartBreakdown').select_option('organizations')
    layout=page.locator('#chartSeriesLayout')
    if layout.is_visible():
        layout.select_option('separate')
    page.locator('#hideEmptyPeriods').uncheck()
    line=page.locator('#chartPanel [data-reference-line]')
    if line.get_attribute('aria-pressed')!='true':
        line.click()
    chart=page.locator('#chartsContainer')
    expect(chart.locator('svg polyline.chart-series').first).to_be_visible()
    fmt=lambda n:page.evaluate('n=>window.unifiedUI.fmt(n,"amount")',n)
    for name,amount in [(first,20),(second,30)]:
        expect(chart.locator(f'svg circle[data-chart-tooltip*="{name}"][data-chart-tooltip*="01.01.2018: {fmt(amount)}"]').first).to_be_visible()
    expect(chart.locator('svg circle[data-chart-tooltip*="03.01.2018"]')).to_have_count(0)
    expect(page.locator('#chartHelp')).to_contain_text('Частичные значения')
    chart.screenshot(path=str(output/'windows-sql-partial-amount-chart.png'))
    page.locator('#chartPanel [data-chart="table"]').click()
    rows=page.locator('#dataTable table tbody tr')
    for name,amount in [(first,20),(second,30)]:
        row=rows.filter(has_text=name).filter(has_text='Сумма').first
        expect(row).to_contain_text(fmt(amount))
        expect(row).to_contain_text('—')

    navigate(page,'comparison')
    wait_for_text(page,'#comparisonCoverage','базового и анализируемого периодов',90_000)
    # Isolate amount so the graph assertions do not depend on the default KPI selection.
    metric_picker=page.locator('#comparisonMetrics details')
    metric_picker.locator('summary').click()
    metric_picker.locator('input[value="amount"]').check()
    for other in metric_picker.locator('input:not([value="amount"]):checked').all():
        other.uncheck()
    metric_picker.locator('summary').click()
    comparison=page.locator('#comparisonTable table').first
    expect(comparison).to_be_visible(timeout=90_000)
    expect(comparison).to_contain_text('Значение сейчас')
    expect(comparison).to_contain_text(fmt(140))
    expect(comparison).not_to_contain_text('Изменение, %')
    page.locator('#comparison').screenshot(path=str(output/'windows-sql-partial-comparison.png'))

    work={'key':'work:Сумма известна','kind':'work','name':'Сумма известна'}
    try:
        page.evaluate('orgs => window.referenceFilters.setOrganizations(orgs)',[first,second])
        page.evaluate('work => window.referenceFilters.setWorks(false,[[work.key,work]])',work)
        page.locator('#refComparisonModes [data-value="organizations"]').click()
        trends=page.locator('#comparisonDynamicsPlots')
        plot = trends.locator(':scope > .metric-plot')
        expect(plot).to_have_count(1)
        expect(trends.locator(':scope > h3')).to_contain_text('Сумма')
        table = trends.locator('.comparison-dynamics-table')
        expect(table.get_by_role('heading', name='Значения по периодам · Сумма', exact=True)).to_be_visible()
        expect(table.get_by_role('table')).to_be_visible()
        expect(plot.locator('.chart-legend span')).to_have_text([first,second])
        expect(plot.locator('svg polyline.chart-series')).to_have_count(2)
        for name,amount in [(first,20),(second,30)]:
            expect(plot.locator(f'svg circle[data-chart-tooltip*="{name}"][data-chart-tooltip*="01.01.2018: {fmt(amount)}"]')).to_be_visible()
        expect(plot.locator('svg circle[data-chart-tooltip*="03.01.2018"]')).to_have_count(0)
        expect(plot.locator('.chart-period-notices summary')).to_be_visible()
        expect(comparison).to_contain_text(work['name'])
        trends.screenshot(path=str(output/'windows-sql-department-range-comparison.png'))
    finally:
        page.locator('#refComparisonModes [data-value="periods"]').click()
        page.evaluate('() => window.referenceFilters.setWorks(true,[])')
        page.evaluate('orgs => window.referenceFilters.setOrganizations(orgs)',snapshot['org_names'])


def check_omitted_detail_evidence(page, temporary, output):
    source=temporary/'omitted-detail.xlsx';original=omitted_detail_xlsx();source.write_bytes(original)
    snapshot=api(page,'nativeSettingsBootstrap');before=snapshot['reports']
    options={'sheet':'xl/worksheets/sheet1.xml','kind':'day','start':'2020-03-01','organizations':snapshot['org_names']}
    preview=api(page,'nativeLegacyImportPreview',str(source),options)
    assert preview['reportCount']==1 and preview['rowCount']==2,preview
    flags={'replaceExact':False,'acceptCrossType':False,'acceptQuality':False}
    assert_rejected(lambda:api(page,'nativeLegacyImportApply',str(source),options,preview['reviewToken'],flags),
                    'Excluded details bypassed explicit quality acknowledgment','quality')
    assert api(page,'nativeSettingsBootstrap')['reports']==before
    receipt=api(page,'nativeLegacyImportApply',str(source),options,preview['reviewToken'],flags|{'acceptQuality':True})
    assert receipt['reports']==1 and receipt['rows']==2 and source.read_bytes()==original,receipt
    assert page.evaluate('window.unifiedUI.refresh()')['reports']==before+1
    revision=receipt['revision']
    matrix=api(page,'nativeQualityMatrixPage',revision,'day','2020-03-01','2020-03-01',['СМУ16'])
    cell=matrix['rows'][0]['cells'][0]
    assert cell['available'] and cell['count']==3 and cell['amount']==8 and cell['warned_reports']==1,cell
    rows=api(page,'nativeQualityMatrixReports',revision,'day','2020-03-01','СМУ16',None,20)['rows'];assert len(rows)==1,rows
    notes=api(page,'nativeReportQualityPage',revision,rows[0]['id'],'report',0,50)
    assert notes['evidence']['omitted_row_entries']==2 and notes['evidence']['explicit_zero'] is None,notes
    by_row={i['source_row']:i for i in notes['issues'] if i['source_row'] is not None}
    assert by_row[4]['kind']=='invalid' and by_row[4]['skipped'] is True and by_row[4]['organization']=='СМУ16',notes
    assert by_row[5]['kind']=='empty-name' and by_row[5]['skipped'] is None,notes
    assert by_row[6]['kind']=='invalid' and by_row[6]['skipped'] is None,notes
    assert by_row[7]['kind']=='skipped' and by_row[7]['skipped'] is True and by_row[7]['organization'] is None,notes
    open_daily_overview(page,'2020-03-01')
    details=page.locator('#overviewAlerts .coverage-details');details.locator(':scope > summary').click()
    expect(details.locator('td.coverage-warning')).to_have_count(1)
    details.locator('td.coverage-warning .coverage-cell-action').click()
    inspection=details.locator('.coverage-inspection');inspection.locator('.coverage-report > details > summary').click()
    expect(inspection).to_contain_text('Явно исключённые строки отчёта: 2')
    expect(inspection.get_by_text('Строка помечена как исключённая.',exact=True)).to_have_count(2)
    expect(inspection).to_contain_text('Признак явного нуля не сохранён')
    inspection.screenshot(path=str(output/'windows-sql-omitted-detail-evidence.png'))
    assert api(page,'nativeSettingsBootstrap')['revision']==revision and source.read_bytes()==original


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

    navigate(page,'chartPanel')
    page.locator('#chartBreakdown').select_option('organizations')
    assert page.locator('[data-chart="line"]').count()==0
    page.locator('[data-chart="bar"]').click()
    wait_for_app_state(page,'document.querySelector("#chartsContainer svg")!==null','SQLite single-period department bar chart')
    page.locator('#chartOrientation').select_option('horizontal')
    wait_for_app_state(page,'document.querySelector("#chartsContainer .horizontal-bar-scroll svg")!==null','SQLite single-period horizontal bar chart')
    geometry=page.locator('#chartsContainer .horizontal-bar-scroll').first.evaluate(
        'node => ({maximum:getComputedStyle(node).maxHeight, overflow:node.scrollHeight-node.clientHeight})')
    assert geometry['maximum']=='none' and geometry['overflow']<=1,geometry
    page.locator('#chartOrientation').select_option('vertical')
    wait_for_app_state(page,'document.querySelector("#chartsContainer .horizontal-bar-scroll")===null','SQLite restored vertical bar chart')
    navigate(page,'dashboard')
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

    # Verify the canonical dashboard at both desktop and phone CSS viewports.
    original=page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
    try:
        for width,height in ((1280,720),(1366,768),(1920,1080),(768,1024),(390,844),(320,568)):
            page.set_viewport_size({'width':width,'height':height})
            assert page.evaluate('document.documentElement.scrollWidth<=document.documentElement.clientWidth+1'),(width,height)
            assert page.locator('#overviewKpis .kpi').count()==4
            page.screenshot(path=str(output/f'windows-sql-{width}x{height}.png'))
        page.set_viewport_size({'width':1280,'height':720})
        for scale in ('1.25','1.5','2'):
            geometry=page.evaluate('''scale => {
              const html=document.documentElement;
              html.style.zoom=scale;
              return {viewport:html.clientWidth,page:html.scrollWidth,scale,
                overview:document.querySelector('#overviewPanel').getBoundingClientRect().right,
                periods:document.querySelector('#reportShell .filter-fold').getBoundingClientRect().right};
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

    common_period(page,'Неделя','2025-01-06')
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
    inspect_reports=alerts.get_by_role('button',name='Проверить данные',exact=True)
    inspect_reports.click()
    expect(page.locator('#upload')).to_have_class('view active')
    page.locator('#backBtn').click()
    expect(page.locator('#refNative')).to_have_attribute('data-panel','overviewPanel')
    expect(inspect_reports).to_be_focused()
    expect(alerts).to_contain_text('Неполные данные: 28 пропусков')
    expect(amount).to_have_text('—')
    page.screenshot(path=str(output/'windows-sql-persistent-coverage.png'))
    # Entirely absent daily ranges must still generate every expected day.
    common_period(page,'Неделя','2025-01-06','2025-02-10')
    expect(alerts).to_contain_text('Неполные данные: 168 пропусков')
    details=alerts.locator('.coverage-details')
    details.locator('summary').click()
    expect(details.locator('.coverage-matrix thead th')).to_have_count(32)
    expect(details.locator('td.coverage-missing')).to_have_count(124)
    expect(details.locator('.coverage-matrix thead th').last).to_contain_text('05.02.2025')
    expect(details.locator('.coverage-matrix thead th').nth(1)).to_contain_text('06.01.2025')
    matrix_pager=details.locator('[data-sql-pager="coverage-periods"]')
    matrix_pager.locator('[data-page-action="next"]').click()
    expect(matrix_pager).to_have_attribute('data-sql-window','1')
    expect(details.locator('.coverage-matrix thead th')).to_have_count(12)
    expect(details.locator('td.coverage-missing')).to_have_count(44)
    expect(details.locator('.coverage-matrix thead th').last).to_contain_text('16.02.2025')
    expect(details.locator('.coverage-matrix thead th').nth(1)).to_contain_text('06.02.2025')
    matrix_pager.locator('[data-page-action="previous"]').click()
    expect(matrix_pager).to_have_attribute('data-sql-window','0')
    expect(details.locator('td.coverage-missing')).to_have_count(124)
    page.screenshot(path=str(output/'windows-sql-coverage-pages.png'))
    # The installed WebView2 must drill into actual indexed report provenance.
    common_period(page,'Неделя','2024-12-30','2025-01-06')
    wait_for_app_state(page,'window.unifiedUI.scope?.from==="2024-12-30" && window.unifiedUI.scope?.to==="2025-01-12"','Mixed complete and missing daily quality range')
    details=alerts.locator('.coverage-details')
    details.locator(':scope > summary').click()
    expect(details.locator('td.coverage-ok')).to_have_count(8)
    expect(details.locator('td.coverage-missing')).to_have_count(48)
    action=details.locator('td.coverage-ok .coverage-cell-action').first
    action.focus();page.keyboard.press('Enter')
    inspection=details.locator('.coverage-inspection')
    expect(inspection).to_be_visible()
    evidence=api(page,'nativeQualityMatrixReports',revision,'day','2024-12-30',orgs[0],None,20)
    assert evidence['revision']==revision and len(evidence['rows'])==1 and evidence['next'] is None,evidence
    saved=evidence['rows'][0]
    assert saved['valid'] and not saved['shifted'] and not saved['duplicate'] and not saved['overlap'],saved
    expect(inspection.locator('.coverage-report strong')).to_have_text(saved['source_name'] or 'Источник не сохранён')
    inspection.locator('.coverage-report > details > summary').click()
    expect(inspection).to_contain_text('Подробные замечания не найдены. Это не подтверждает полноту источника.')
    page.screenshot(path=str(output/'windows-sql-quality-matrix.png'))
    inspection.get_by_role('button',name='Закрыть подробности',exact=True).focus()
    page.keyboard.press('Escape')
    expect(inspection).to_be_hidden()
    expect(action).to_be_focused()
    missing=details.locator('td.coverage-missing .coverage-cell-action').first
    missing.click()
    expect(inspection).to_contain_text('Причина отсутствия неизвестна; значение не заменено нулём.')
    expect(inspection.locator('.coverage-report')).to_have_count(0)
    assert api(page,'nativeSettingsBootstrap')['revision']==revision,'Read-only diagnostics changed stored data'
    dashboard_month(page)


def check_extended(page):
    assert page.locator('#printSummary').is_enabled()
    page.emulate_media(media='print')
    try:
        assert page.evaluate('getComputedStyle(document.body).backgroundColor')=='rgb(255, 255, 255)'
        assert not page.locator('main.container').is_visible()
    finally:
        page.emulate_media(media='screen')
    common_period(page,'Месяц','2024-02-01',base='2023-12-01')
    wait_for_app_state(page,
        'window.unifiedUI.scope?.baseFrom==="2023-12-01" && window.unifiedUI.scope?.baseTo==="2023-12-29"',
        'Shared baseline calendar bounds')
    navigate(page,'comparison')
    wait_for_text(page,'#comparisonCoverage','Второй интервал: Полные данные',90_000)
    date=lambda index: page.locator(f'#refComparisonDates [data-compare-date="{index}"]')
    editor=page.locator('#refComparisonDates [data-edit-base]')
    editor.click()
    expect(date(0)).to_have_text('01.12.2023')
    choose_calendar_date(page,date(0),'2024-01-01')
    choose_calendar_date(page,date(1),'2024-02-29')
    expect(page.locator('#refComparisonDates [data-lengths]')).to_contain_text('Дней в базе: 60 · в анализируемом периоде: 29')
    navigate(page,'dashboard')
    common_period(page,'Месяц','2024-02-01',base='2023-11-01')
    wait_for_app_state(page,'window.unifiedUI.scope?.baseFrom==="2023-11-01"','Changed shared baseline')
    navigate(page,'comparison')
    expect(date(0)).to_have_text('01.01.2024')
    reset=page.locator('#refComparisonDates [data-reset-base]')
    reset.click()
    expect(date(0)).to_be_hidden()
    expect(reset).to_be_hidden()
    expect(editor).to_be_focused()
    expect(page.locator('#comparison .detail-date-scope')).to_contain_text('01.11.2023')
    # Both missing intervals show a reason, with no meaningless pager/CSV.
    common_period(page,'Месяц','2023-12-01',base='prevMonth')
    wait_for_text(page,'#comparisonCoverage','базового и анализируемого периодов',90_000)
    expect(page.locator('#comparisonTable')).to_contain_text('Нет доступных значений для сравнения')
    expect(page.locator('#comparisonTable table')).to_have_count(0)
    expect(page.locator('#comparisonTable .change-pagination')).to_have_count(0)
    expect(page.locator('#comparisonTable button')).to_have_count(0)
    page.locator('#comparison').screenshot(path=str(ROOT/'tauri/test-artifacts/windows-sql-empty-comparison.png'))
    common_period(page,'Месяц','2024-02-01',base='prevMonth')
    wait_for_app_state(page,'window.unifiedUI.scope?.baseFrom==="2024-01-01"','January shared baseline')
    wait_for_text(page,'#comparisonCoverage','Первый интервал: Полные данные',90_000)
    # Imported months are queried by their first day while visible end dates are inclusive.
    common_period(page,'Месяц','2024-02-01',base='prevMonth',source='imported')
    expect(page.locator('#comparison .detail-date-scope')).to_contain_text('29.02.2024')
    editor.click()
    choose_calendar_date(page,date(1),'2024-02-01')
    expect(date(1)).to_have_text('29.02.2024')
    expect(page.locator('#refComparisonDates [data-lengths]')).to_contain_text('Дней в базе: 60 · в анализируемом периоде: 29')
    assert page.evaluate('window.referenceDetail.currentComparison()') == {
        'before':{'from':'2024-01-01','to':'2024-02-01'},'after':{'from':'2024-02-01','to':'2024-02-01'}}
    reset.click()
    common_period(page,'День','2024-02-29',base='year')
    expect(page.locator('#comparison .detail-date-scope')).to_contain_text('28.02.2023')
    common_period(page,'Месяц','2024-02-01',base='prevMonth')
    wait_for_text(page,'#comparisonCoverage','Первый интервал: Полные данные',90_000)
    expect(page.locator('#contributions > h3')).to_be_visible()
    expect(page.locator('#contributions')).to_have_js_property('tagName', 'SECTION')
    wait_for_text(page,'#contributionCheck','совпадает',90_000)
    navigate(page,'structurePanel')
    expect(page.locator('#structurePanel > h2')).to_be_visible()
    expect(page.locator('#chartPanel')).to_be_hidden()
    wait_for_text(page,'#structureResults','Итог:',90_000)
    trend=page.locator('.tool-tabs [data-section="chartPanel"]')
    trend.focus();trend.press('Enter')
    expect(page.locator('#workViewMode')).to_have_value('trend')
    expect(page.locator('#chartPanel')).to_be_visible()
    expect(page.locator('#structurePanel')).to_be_hidden()
    for control,result in (('usualPanel','usualCoverage'),('regularityPanel','regularityCoverage')):
        navigate(page,control)
        expected='Исторических окон с отчётами:' if control=='usualPanel' else 'Периодов с отчётами:'
        wait_for_text(page,f'#{result}',expected,90_000)
    navigate(page,'changes')
    expect(page.locator('#changes .detail-date-scope')).to_contain_text('29.02.2024')
    expect(page.locator('#changesToCalendar')).to_be_hidden()
    # The month-to-month changes are real but smaller than the default strong thresholds.
    more=page.locator('#changesMoreOptions')
    if not more.evaluate('node=>node.open'):
        more.locator(':scope > summary').click()
    page.locator('#changesMode').select_option('all')
    expect(page.locator('#changesResults tbody tr').first).to_be_visible(timeout=90_000)
    expect(page.locator('[data-sql-pager="changesResults"]')).to_have_attribute('data-sql-window','0',timeout=90_000)
    # The pager can remain mounted while a date change reloads the KPIs.
    expect(page.locator('#changesSummary .kpi-action')).to_have_count(4,timeout=90_000)
    expect(page.locator('#changesSummary')).to_have_attribute('aria-busy','false')
    assert page.locator('#changesResults tbody tr').count()<=50
    slider=page.locator('#changesPercent')
    slider.focus();slider.press('End')
    expect(page.locator('#changesSensitivityValue')).to_have_text('200%')
    expect(page.locator('#changesSensitivityExample')).to_contain_text('200')
    slider.press('Home')
    expect(page.locator('#changesSensitivityValue')).to_have_text('0%')
    page.locator('#changesSummary .kpi-action').first.click()
    expect(page.locator('#changesActiveFilter')).to_contain_text('значительный рост')
    expect(page.locator('#changesDirection')).to_have_value('significant-up')
    page.locator('#changesSummary .kpi-action').filter(has_text='Значительно снизилось').click()
    expect(page.locator('#changesActiveFilter')).to_contain_text('значительное снижение')
    expect(page.locator('#changesDirection')).to_have_value('significant-down')

    # The synthetic daily workbook has one work per department, giving 60 adjacent records.
    common_period(page,'День','2024-01-01','2024-01-16')
    page.locator('#changesDirection').select_option('all')
    rows=page.locator('#changesResults tbody tr')
    pager=page.locator('[data-sql-pager="changesResults"]')
    position=pager.locator('[data-page-range]')
    expect(position).to_have_text('Позиции 1–50 из 120',timeout=90_000)
    expect(rows).to_have_count(50)
    expect(rows.nth(0).locator('td').nth(4)).to_have_text('Количество / объём')
    expect(rows.nth(1).locator('td').nth(4)).to_have_text('Сумма')
    pager.locator('[data-page-action="next"]').click()
    expect(position).to_have_text('Позиции 51–100 из 120',timeout=90_000)
    expect(rows).to_have_count(50)
    pager.locator('[data-page-action="choose"]').click()
    pager.locator('[data-page-number="3"]').click()
    expect(position).to_have_text('Позиции 101–120 из 120',timeout=90_000)
    expect(rows).to_have_count(20)


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
    expect(page.locator('#groupSearchCount')).to_contain_text('Найдено групп: 1')
    expect(page.locator('#groupCoverage')).to_contain_text('1')
    expect(group.locator('.group-members .group-assign').first).to_be_visible()
    expect(group.locator('.group-members [data-sql-pager]')).to_have_count(0)
    rename=group.locator('button[data-group-rename]')
    initial_revision=confirmations.revision()
    for how in ('button','escape'):
        rename.click()
        dialog=page.locator('#groupRenameDialog')
        expect(dialog).to_be_visible()
        assert dialog.evaluate("node=>node.matches(':modal')")
        expect(page.locator('#refNative')).to_have_attribute('data-panel','groups')
        name=dialog.locator('#groupRenameName')
        expect(name).to_be_focused()
        expect(name).to_have_value('Группа CI')
        name.fill('Несохранённое имя')
        if how=='button':
            dialog.locator('#groupRenameCancel').click()
        else:
            name.press('Escape')
        expect(dialog).not_to_be_visible()
        expect(page.locator('#refNative')).to_have_attribute('data-panel','groups')
        expect(rename).to_be_focused()
        assert confirmations.revision()==initial_revision
    rename.click()
    page.locator('#groupRenameName').fill('Переименованная группа CI')
    page.locator('#groupRenameName').press('Enter')
    group=page.locator('#groupList details').filter(has_text='Переименованная группа CI')
    # Rename reloads the list, opens the replacement row, then restores focus.
    # Clicking its summary while that restoration is in flight can close the row.
    expect(group.get_by_role('button',name='Переименовать',exact=True)).to_be_focused(timeout=90_000)
    expect(group).to_have_js_property('open',True)
    assert confirmations.revision()>initial_revision
    member=group.locator('.group-member').filter(has_text='Альфа')
    group_button=member.locator('.group-assign')
    expect(group_button).to_have_text('Группа')
    group_button.click(timeout=90_000)
    picker=page.locator('#groupPickerDialog')
    search=picker.locator('#groupPickerSearch')
    expect(search).to_be_focused()
    current_group=picker.get_by_role('button',name='Переименованная группа CI',exact=True)
    search.fill('Несуществующая группа')
    expect(picker.get_by_role('status')).to_have_text('Группы не найдены.')
    expect(current_group).to_have_count(0)
    search.fill('Переименованная')
    expect(current_group).to_have_attribute('aria-current','true')
    search.press('Escape')
    expect(picker).to_have_count(0)
    expect(group_button).to_be_focused()
    group_button.click()
    picker.locator('#groupPickerChoices').get_by_role('button',name='Без группы',exact=True).click()
    wait_for_text(page,'#flash','Работа перенесена.',90_000)
    expect(page.locator('#groupSearch')).to_be_focused(timeout=90_000)
    dismiss_notice=page.locator('#flash').get_by_role('button',name='Закрыть уведомление',exact=True)
    dismiss_notice.click()
    expect(page.locator('#flash')).to_be_hidden()
    snapshot=api(page,'nativeSettingsBootstrap')
    assert snapshot['memberships']==0 and snapshot['groups']==0,snapshot
    assert group.count()==0
    # Removing the final member intentionally removes its source group.
    # Recreate it to verify the separate explicit-delete confirmation path.
    group=create()
    page.get_by_role('button',name='Выбрать группы',exact=True).click()
    group.get_by_role('checkbox',name='Выбрать «Группа CI»',exact=True).check()
    expect(page.locator('.group-removal')).to_contain_text('Выбрано групп: 1')
    confirmations.cancel_only(page.get_by_role('button',name='Удалить выбранные',exact=True),
        'Удаляемых групп: 1.')
    page.get_by_role('button',name='Завершить выбор',exact=True).click()
    confirmations.cancel_then_accept(group.get_by_role('button',name='Удалить группу',exact=True),
        'Удалить группу «Группа CI»? Исходные работы останутся.')
    wait_for_text(page,'#flash','Группа удалена.',90_000)
    dismiss_notice.focus()
    dismiss_notice.press('Enter')
    expect(page.locator('#flash')).to_be_hidden()
    assert api(page,'nativeSettingsBootstrap')['groups']==0


def backup_contents(path):
    with closing(sqlite3.connect(path)) as database:
        assert database.execute('PRAGMA quick_check').fetchone()==('ok',)
        reports=database.execute('SELECT id,json FROM reports ORDER BY id').fetchall()
        settings=database.execute('SELECT json FROM kv WHERE name=\'settings\'').fetchone()
        metrics=database.execute('SELECT COUNT(*) FROM metric_report_docs').fetchone()[0]
    return reports,settings,metrics


def check_delete_without_backup(page, temporary, confirmations):
    """Last destructive fixture: real confirmation and SQLite, no Save dialog."""
    # The earlier trend fixture is a wide Excel report with shared source diagnostics.
    wide_file = temporary/'trend-zero.xlsx'
    original_wide = wide_file.read_bytes()
    snapshot = api(page,'nativeSettingsBootstrap')
    wide = api(page,'managementReportPage',snapshot['revision'],{'from':'2019-01-01','to':'2019-01-04'},None,20)
    assert [row['start'] for row in wide]==['2019-01-04','2019-01-02','2019-01-01'],wide
    anchor=wide[-1]['id']
    assert_rejected(lambda:api(page,'nativeDeleteReport',snapshot['revision'],anchor),
                    'A deletion removed the shared diagnostic anchor','общие замечания excel')
    deleted=api(page,'nativeDeleteReport',snapshot['revision'],wide[0]['id'])
    assert deleted['deleted']==1 and wide_file.read_bytes()==original_wide
    days=api(page,'unifiedCalendar',deleted['revision'],2019)['days']
    assert not any(day['date']=='2019-01-04' for day in days),days
    evidence=api(page,'nativeReportQualityPage',deleted['revision'],wide[1]['id'],'source',0,50)
    assert evidence['evidence']['document_id']==anchor,evidence
    source=temporary/'delete-without-backup.xlsx'
    source.write_bytes(daily_workbook_bytes())
    original=source.read_bytes()
    before=api(page,'nativeSettingsBootstrap')
    options={'sheet':'xl/worksheets/sheet1.xml','kind':'day','start':'2030-01-01',
             'organizations':before['org_names']}
    preview=api(page,'nativeLegacyImportPreview',str(source),options)
    assert preview['reportCount']==6 and preview['conflicts']['replacements']==0
    receipt=api(page,'nativeLegacyImportApply',str(source),options,preview['reviewToken'],
                {'replaceExact':False,'acceptCrossType':False,'acceptQuality':True})
    page.evaluate('window.unifiedUI.refresh()')
    revision=receipt['revision']
    rows=api(page,'managementReportPage',revision,{'from':'2030-01-01','to':'2030-01-06'},None,50)
    assert len(rows)==6 and rows[0]['start']=='2030-01-06'
    database=Path(api(page,'info')['dataFile'])
    files={p.name for p in database.parent.glob('*.sqlite3')}
    stored=backup_contents(database)
    navigate(page,'databases')
    expect(page.locator('#databases #unifiedReportDatabase')).to_have_text('Личная база')
    loaded=page.locator('#uploadedReports')
    if not loaded.evaluate('node=>node.open'):
        loaded.locator(':scope > summary').click()
    card=page.locator('#reports .calendar-source').first
    expect(card.locator('strong')).to_contain_text('2030-01-06',timeout=90_000)
    confirmations.cancel_then_accept(card.get_by_role('button',name='Удалить отчёт',exact=True),
        'Удалить отчёт 2030-01-06 — 2030-01-06 из личной базы?')
    wait_for_app_state(page,
        "typeof window.unifiedUI.snapshot?.reports==='number' && window.unifiedUI.snapshot.reports==="+
        str(before['reports']+5),'Confirmed single-report deletion',90_000)
    remaining=backup_contents(database)
    assert remaining[0]==[row for row in stored[0] if row[0]!=rows[0]['id']]
    assert remaining[1:]==stored[1:]
    assert_rejected(lambda:api(page,'nativeDeleteReport',revision,rows[1]['id']),
                    'An old single-delete selection was accepted','stale storage revision')
    for stamp in ('2030-01-05','2030-01-04'):
        report=page.locator('#reports .calendar-source').filter(has_text=stamp).first
        report.get_by_role('checkbox').check()
    selected=page.locator('#unifiedDeleteSelectedReports')
    selected.click()
    prompt='Удалить 2 отчёта из личной базы?'
    dialog,owner=confirmations.find_dialog(prompt)
    confirmations.answer(dialog,owner,prompt,2)
    expect(selected).to_be_focused()
    expect(selected).to_be_enabled()
    assert backup_contents(database)==remaining
    selected.press('Enter')
    dialog,owner=confirmations.find_dialog(prompt)
    confirmations.answer(dialog,owner,prompt,1)
    wait_for_app_state(page,'window.unifiedUI.snapshot?.reports==='+str(before['reports']+3),
        'Confirmed selected report deletion',90_000)
    after_selection=backup_contents(database)
    assert after_selection[0]==[row for row in stored[0] if row[0] not in {r['id'] for r in rows[:3]}]
    assert after_selection[1:]==stored[1:]

    period=page.locator('#unifiedDeletePeriodReports')
    period.click()
    calendar=page.get_by_role('dialog',name='Период удаления отчётов')
    calendar.get_by_role('button',name='Отмена',exact=True).click()
    expect(calendar).to_have_count(0)
    expect(period).to_be_focused()
    period.press('Enter')
    expect(calendar).to_be_visible()
    page.keyboard.press('Escape')
    expect(calendar).to_have_count(0)
    expect(period).to_be_focused()
    period.press('Enter')
    calendar.get_by_role('button',name='Месяцы',exact=True).click()
    calendar.locator('[data-stamp="2030-01-01"]').click()
    calendar.get_by_role('button',name='Подтвердить',exact=True).click()
    prompt='Удалить 3 отчёта за период 01.01.2030 — 31.01.2030 из личной базы?'
    dialog,owner=confirmations.find_dialog(prompt)
    confirmations.answer(dialog,owner,prompt,1)
    expect(calendar).to_have_count(0)
    wait_for_app_state(page,'window.unifiedUI.snapshot?.reports==='+str(before['reports']),
        'Confirmed calendar-period report deletion',90_000)
    after_period=backup_contents(database)
    assert after_period[0]==[row for row in stored[0] if row[0] not in {r['id'] for r in rows}]
    assert after_period[1:]==stored[1:]

    all_reports=page.locator('#unifiedDeleteAllReports')
    all_reports.click()
    prompt='Удалить все '+str(before['reports'])
    dialog,owner=confirmations.find_dialog(prompt)
    confirmations.answer(dialog,owner,prompt,2)
    expect(all_reports).to_be_focused()
    assert backup_contents(database)==after_period
    navigate(page,'storage')
    clear_snapshot=api(page,'nativeSettingsBootstrap')
    clear_revision=clear_snapshot['revision']
    clear_report_confirmation(page,confirmations,clear_snapshot['reports'],accept=True)
    wait_for_app_state(page,
        "typeof window.unifiedUI.snapshot?.reports==='number' && window.unifiedUI.snapshot.reports===0",
        'Confirmed report clearing',90_000)
    cleared=backup_contents(database)
    assert cleared[0]==[] and cleared[1:]==stored[1:]
    assert {p.name for p in database.parent.glob('*.sqlite3')}==files,'Deletion created an unsolicited SQLite backup'
    assert_rejected(lambda:api(page,'unifiedClearReports',clear_revision),
                    'An old clear-all selection was accepted','stale storage revision')
    with closing(sqlite3.connect(database)) as check:
        assert check.execute('PRAGMA foreign_key_check').fetchall()==[]
        for table in ['report_source_diagnostic_links','report_source_ownership']:
            assert check.execute('SELECT COUNT(*) FROM '+table).fetchone()==(0,)
    assert source.read_bytes()==original


def check_backup_and_replace(page, temporary, snapshot):
    revision=api(page,'nativeBackupSqlite',str(snapshot))
    assert isinstance(revision,int) and snapshot.is_file()
    assert_rejected(lambda:api(page,'nativeOpenBackupFolder',str(temporary)),
                    'Reveal accepted an arbitrary directory','недоступна')
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


def check_scheduled_backup(page):
    navigate(page,'databases')
    control=page.locator('#scheduledBackupEnabled')
    expect(control).to_be_enabled()
    assert not control.is_checked()
    expect(page.locator('#scheduledBackupStatus')).to_contain_text('ещё не создавалась')
    revision=api(page,'nativeRevision')
    control.click()
    wait_for_text(page,'#scheduledBackupStatus','Последняя успешная копия:',90_000)
    assert control.is_checked()
    status=api(page,'nativeScheduledBackup',None)
    assert status['enabled'] and status['lastSuccess']>0 and status['error'] is None,status
    path=Path(status['lastPath'])
    assert path.is_file()
    reviewed=api(page,'nativeRestorePreview',str(path))
    assert reviewed['reportCount']==366
    assert api(page,'nativeRevision')==revision
    control.click()
    expect(control).to_be_enabled()
    assert not control.is_checked()
    stopped=api(page,'nativeScheduledBackup',None)
    assert not stopped['enabled'] and stopped['lastSuccess']==status['lastSuccess']
    assert path.is_file()


def check_folder_rule_and_revocation(page, workbook, confirmations):
    navigate(page,'upload')
    page.locator('#uploadSourceFolder').click()
    wait_for_app_state(page,'typeof window.nativeFolderReady!=="undefined"','Native folder controller startup')
    page.evaluate('window.nativeFolderReady')
    toggle=page.locator('#sourceAutoImport')
    sync_status=page.locator('#sourceFolderLastSync')
    assert not toggle.is_checked()
    expect(sync_status).to_contain_text('ещё не было')
    listing=api(page,'list')
    assert {file['name'] for file in listing['files']}=={workbook.name}
    row=page.locator('#sourceFolderRows tr').filter(has_text=workbook.name)
    row.get_by_role('button',name='Настроить правило для '+workbook.name,exact=True).click()
    page.locator('#sourceRuleType').select_option('day')
    page.locator('#sourceRuleFormat').select_option('period-pairs')
    page.locator('#sourceRuleYear').fill('2024')
    confirmations.cancel_then_accept(page.locator('#sourceRuleSave'),
        f'Сохранить правило для «{workbook.name}»?')
    wait_for_text(page,'#sourceRuleStatus','Правило сохранено',90_000)
    rule=api(page,'list')['rules'][workbook.name]
    assert rule['format']=='daily-wide' and rule['year']==2024
    assert api(page,'nativeSettingsBootstrap')['reports']==366

    def save_rule(source, kind, start):
        listing=api(page,'list')
        file=next(item for item in listing['files'] if item['name']==source.name)
        assert file['nativeSignature']
        return api(page,'nativeSetRuleChecked',file['name'],{'type':kind,'start':start,'sheet':'','format':'legacy'},
                   listing['revision'],listing['folder'],file['nativeSignature'])

    fixture=daily_workbook_bytes()
    auto_probe=workbook.with_name('auto-probe.xlsx')
    auto_probe.write_bytes(fixture)
    save_rule(auto_probe,'day','2025-01-01')
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
    expect(sync_status).to_contain_text('ещё не было')

    toggle.click()
    prompt='Разрешить в этом сеансе автоматически добавлять новые отчёты из настроенной папки?'
    dialog,owner=confirmations.find_dialog(prompt)
    confirmations.answer(dialog,owner,prompt,1)
    file_rows=page.locator('#sourceFolderRows tr')
    expect(file_rows.filter(has_text=auto_probe.name)).to_contain_text('Отчёты добавлены автоимпортом',timeout=90_000)
    expect(row).to_contain_text('Требует ручной проверки',timeout=90_000)
    expect(sync_status).to_contain_text('Последний успешный автоимпорт из папки:')
    sync_time=sync_status.inner_text()
    wait_for_text(page,'#unifiedIndexStatus','Локальная база SQLite. Отчётов: 372',90_000)
    after_import=api(page,'nativeRevision')
    imported=api(page,'managementReportPage',after_import,{'report_type':'day','from':'2025-01-01','to':'2025-01-06'},None,50)
    assert sorted(report['start'] for report in imported)==[f'2025-01-{day:02}' for day in range(1,7)]
    assert api(page,'nativeSettingsBootstrap')['reports']==372
    stored=backup_contents(confirmations.database)[0]

    duplicate=workbook.with_name('auto-duplicate.xlsx')
    overlap=workbook.with_name('auto-overlap.xlsx')
    duplicate.write_bytes(fixture)
    save_rule(duplicate,'day','2025-01-01')
    overlap.write_bytes(fixture)
    after_rules=save_rule(overlap,'week','2025-01-01')
    page.evaluate('window.unifiedReports.refreshFolder()')
    for source in (duplicate,overlap):
        expect(file_rows.filter(has_text=source.name)).to_contain_text('Требует ручной проверки',timeout=90_000)
    assert api(page,'nativeRevision')==after_rules
    assert backup_contents(confirmations.database)[0]==stored
    expect(sync_status).to_have_text(sync_time)

    toggle.click()
    wait_for_text(page,'#sourceAutoStatus','Автоматический импорт выключен.')
    listing=api(page,'list')
    session=api(page,'nativeFolderImportSession',True)
    try:
        def preview(source):
            file=next(item for item in listing['files'] if item['name']==source.name)
            return api(page,'nativeFolderImportPreview',{
                'session':session,'folder':listing['folder'],'revision':listing['revision'],
                'name':source.name,'signature':file['nativeSignature']})

        assert_rejected(lambda:preview(duplicate),'A duplicate file can replace another source',
                        'another source\'s overlapping period')
        assert preview(overlap)['conflicts']['cross_type_overlaps']>0
    finally:
        api(page,'nativeFolderImportSession',False)
    assert api(page,'nativeRevision')==after_rules
    for source in (auto_probe,duplicate,overlap):
        assert source.read_bytes()==fixture
        source.unlink()

@contextmanager
def windows_test_workspace(parent=None):
    """Release only this test's WebView2 workers before deleting their temporary profile."""
    workspace=Path(tempfile.mkdtemp(prefix='analiz-webview2-',dir=parent))
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
            def installed_page(*, print_probe_phase=False):
                process,engine=start();browser=page=None
                try:
                    browser=pw.chromium.connect_over_cdp('http://127.0.0.1:9227')
                    page=wait_for_webview_page(browser)
                    errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
                    yield page,engine,process.pid
                    assert not errors,errors
                    try:
                        stop(process)
                    except (RuntimeError,subprocess.SubprocessError) as close_error:
                        # A system print preview can block a normal window close.
                        # Only this read-only fixture process is disposable.
                        if not print_probe_phase or not report.get('printProbeStatus','').startswith('unavailable-'):
                            raise
                        report['printProbeTeardown']=type(close_error).__name__+': '+str(close_error)
                        force_stop(process)
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
            workbook=source_folder/'synthetic-year.xlsx';workbook.write_bytes(undated_quarters_xlsx())
            original_xlsx=workbook.read_bytes()
            other_folder=tmp/'other-source';other_folder.mkdir()
            legacy_source=other_folder/'ordinary.xlsx';legacy_source.write_bytes(daily_workbook_bytes())
            snapshot=tmp/'online-backup.sqlite3'
            independent_failures=[]
            @contextmanager
            def independent(name):
                # Each read-only walkthrough gets a freshly started WebView2.
                # Keep every failed assertion and fail the job after unrelated checks.
                try:
                    yield
                except Exception as error:
                    error.add_note(name)
                    independent_failures.append(error)
                    report.setdefault('failedWalkthroughs',{})[name]=str(error)
                    print('FAILED '+name+': '+str(error),flush=True)

            with independent('File dialogs on an empty database'), installed_page() as (page,_,pid):
                ready(page,0)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_reference_file_dialogs(page,confirmations,output)
                report['checks'].append('Empty database file pickers cancel without modifying SQLite and return focus')
            with independent('Reference shell on an empty database'), installed_page() as (page,_,_):
                ready(page,0)
                check_reference(page,output,False)
                report['checks'].append('Reference interface walkthrough and close/cancel/focus checks on an empty database')
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
                check_reference_import_flow(page,tmp,confirmations,output)
                report['checks'].append('101st folder file, saved second-sheet parity, cancellation and exact weekly post-import scope')
                check_dated_quarters_preview(page,tmp)
                import_year(page,workbook,confirmations)
                assert workbook.read_bytes()==original_xlsx
                check_report_views(page)
                report['checks'].append('Native Excel confirmation blocks writes, Cancel preserves SQLite, and OK imports 366 reports with 50-card pagination and a 29-day calendar')
            with independent('Reference analytics on populated SQLite'), installed_page() as (page,_,_):
                ready(page,366)
                check_reference_data(page,output)
            with independent('File dialogs on populated SQLite'), installed_page() as (page,_,pid):
                ready(page,366)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_reference_data_file_dialogs(page,confirmations,output)
                report['checks'].append('Summary CSV action is hidden and read-only backup verification preserves data; retained restore IPC copies without overwrite and schedules/cancels only the synthetic backup')
            with independent('Read-only management confirmations'), installed_page() as (page,_,pid):
                ready(page,366)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_cancelled_management_confirmations(page,confirmations)
                report['checks'].append('Native management confirmation Cancel, available Close, and Escape preserve reports and drafts')
            with independent('System print preview'), installed_page(print_probe_phase=True) as (page,_,pid):
                wait_for_app_state(page,'Boolean(window.appReady && window.unifiedUI?.snapshot?.reports)',
                                   'Print probe on the isolated populated SQLite database',90_000)
                expect(page.locator('#s1')).to_be_visible()
                page.wait_for_function("""() => document.querySelector('#s1').getAttribute('aria-busy')!=='true'
                  && !document.querySelector('#s1 .data-card button').disabled""",timeout=90_000)
                probe=probe_reference_print(page,output,pid)
                report['printProbeStatus']=probe['status']
                if probe['status']!='observed-cancel-reopen-escape-and-focus':
                    raise AssertionError('Installed print preview: '+probe['status'])
            with installed_page() as (page,_,pid):
                ready(page,366)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_analytics(page,tmp,output)
                report['checks'].append('Daily-source annual and February totals, exact work and organization filters, missing-week analytics, 200% zoom and responsive widths, and protected UTF-8 CSV')
                check_extended(page)
                report['checks'].append('Canonical changes, usual level, regularity, structure and contribution views use the current SQLite selection; print uses the dedicated layout')
                check_groups(page,confirmations)
                report['checks'].append('Group creation, member assignment and removal, and native Cancel/OK for explicit deletion')
                check_reference_catalog(page,confirmations,output)
                report['checks'].append('Catalog search and create/reset validation precede the compact bounded list at desktop and narrow widths; the owned test entry is removed')
                check_metrics(page,tmp)
                report['checks'].append('Installed native metric imports and ratio-of-sums queries refuse repeated JSON and XLSX sources')
                original=check_backup_and_replace(page,tmp,snapshot)
                assert workbook.read_bytes()==original_xlsx
                report['checks'].append('SQLite online backup quick-check, no overwrite, and atomic 366-report replacement with a verified pre-import backup')
                check_scheduled_backup(page)
                report['checks'].append('Opt-in scheduled SQLite backup is available for verified restore and disabling it retains the last successful copy')
                navigate(page,'upload')
                page.locator('#unifiedUploadQuality').click()
                wait_for_text(page,'#qualitySummary','Отсутствие замечаний само по себе не подтверждает полноту',90_000)
                assert page.locator('#qualityTable tbody tr').count()<=50
                expect(page.locator('#qualitySummary')).to_be_visible()
                expect(page.locator('#uploadedReports > summary')).to_be_hidden()
                page.locator('#unifiedUploadList').click()
                expect(page.locator('#uploadedReports > summary')).to_be_visible()
                expect(page.locator('#qualitySummary')).to_be_hidden()
                assert page.url==entry_url
                page.screenshot(path=str(output/'windows-webview2.png'))
                page.evaluate("localStorage.setItem('work-reports-view-v1','upload'); localStorage.setItem('analiz-sql-startup-v1','classic')")
                report['navigation_before_restart']=page.locator('.view.active').get_attribute('id')
                (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')

            with independent('Reference shell after replacement and restart'), installed_page() as (page,_,_):
                ready(page,366)
                report['navigation_after_restart']=page.locator('.view.active').get_attribute('id')
                assert report['navigation_before_restart']=='upload' and report['navigation_after_restart']=='dashboard'
                check_reference(page,output,True)
                report['checks'].append('Populated reference views, closing controls and focus after native replacement and restart')

            with independent('Reference shell in a narrow installed window'), installed_page() as (page,_,pid):
                ready(page,366)
                app = NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                report['narrowWindow'] = set_client_width(page, app, 720)
                check_reference(page,output,True,screenshot_scope='720px')
                report['checks'].append('Populated views, nested closing controls and focus in a 720 px native window')

            with installed_page() as (page,_,pid):
                ready(page,366)
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                assert api(page,'list')['folder']
                check_folder_rule_and_revocation(page,workbook,confirmations)
                report['checks'].append('Normal restart preserves native SQLite; folder autoimport shows the last success, records six new reports and blocks an existing source, a duplicate, an overlap and a revoked session')
                data_file=Path(api(page,'info')['dataFile'])
                safety_before=set(data_file.parent.glob('.analiz-restore-safety-*.sqlite3'))
                reviewed=api(page,'nativeRestorePreview',str(snapshot))
                assert reviewed['reportCount']==366 and len(reviewed['verification'])==64
                requested=api(page,'nativeRestoreSchedule',str(snapshot),reviewed['verification'])
                assert requested=={'reportCount':366,'restartRequired':True},requested
                verify_live=tmp/'still-live.sqlite3'
                api(page,'nativeBackupSqlite',str(verify_live))
                still_live=backup_contents(verify_live)
                assert len(still_live[0])==372 and still_live[0]!=original[0]
                assert api(page,'list')['rules'][workbook.name]['year']==2024
                report['checks'].append('Pending restore leaves the changed live SQLite and its Excel rules in place until restart')

            with installed_page() as (page,_,pid):
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
                navigate(page,'databases')
                expect(page.locator('#databaseRestoreStatus')).to_contain_text('Восстановление выполнено')
                assert workbook.read_bytes()==original_xlsx
                report['checks'].append('Installed restart restores exactly the SQLite backup, retains the replaced data and displays confirmation')
                check_source_evidence(page,tmp,output)
                report['checks'].append('Saved source evidence distinguishes zero provenance, source-day gaps, unknown organization and acknowledgment without changing source XLSX or read-only revision')
                check_filtered_zero_chart(page,tmp,output)
                report['checks'].append('Imported 10, 0, 20 into SQLite and verified hidden zero in the installed chart and table')
                check_partial_amount_charts(page,tmp,output)
                report['checks'].append('Partial Excel amounts appear in installed lines and tables; comparison shows one work across two departments over a date range with no value for a missing day')
                check_omitted_detail_evidence(page,tmp,output)
                report['checks'].append('Native omitted-detail tags distinguish invalid data, unknown organizations, invalid controls and included unnamed rows; persisted totals remain 3 and 8 with explicit acknowledgment')
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_delete_without_backup(page,tmp,confirmations)
                report['checks'].append('Single, selected, period and clear-all deletion preserve settings, XLSX and shared diagnostics, protect surviving linked reports, and reject stale revisions')
            with independent('Print all fifty-one changes'), installed_page(print_probe_phase=True) as (page,_,pid):
                ready(page,0)
                check_print_all_changes(page,tmp,output,pid,api,quality_evidence_xlsx,open_daily_overview)
                report['checks'].append('Both native print previews contained all 13 filtered SQLite rows in order')
    (output/'build-info.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    if independent_failures:
        raise ExceptionGroup('Installed WebView2 walkthroughs failed',independent_failures)
    write_reference_gallery(output, os.environ.get('REFERENCE_HEAD_SHA', ''), report['webview2'])
    print(json.dumps(report,ensure_ascii=False),flush=True)

if __name__=='__main__':
    try:run()
    except BaseException as error:
        record_secondary_failure(error,'Runtime diagnostics failed',lambda:diagnostics(error))
        raise
