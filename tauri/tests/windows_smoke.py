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
from reference_control_layout import check_control_layout
from reference_smoke import check_reference
from reference_catalog import check_reference_catalog
from reference_window_size import set_client_width
from reference_data_smoke import check_reference_data, choose_calendar_date
from reference_gallery import write_reference_gallery
from reference_print_probe import probe_reference_print
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
    tab = page.locator(f'.tool-tabs [data-section="{key}"]')
    if not tab.is_visible():
        if key in ('upload', 'groups', 'storage'):
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
    back=page.locator('#backBtn')
    back.focus();back.press('Escape')
    expect(page.locator('#toolsPage')).not_to_be_visible()
    expect(page.locator('#refAnalytics')).to_be_focused()
    navigate(page,'dashboard')
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


def set_calendar_field(page, field_id, value):
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
    page.get_by_role('button',name='Показать файлы папки',exact=True).click()
    page.locator('#sourceManualFile').select_option(label=workbook.name)
    page.get_by_role('button',name='Открыть для проверки',exact=True).click()
    wait_for_text(page,'#excelFileStatus',workbook.name)
    page.locator('#importFormat').select_option('daily-wide')
    set_calendar_field(page, 'dailyYear', '2024')
    metadata=api(page,'nativeImportSheets',str(workbook))
    assert [sheet['name'] for sheet in metadata]==['Q1','Q2','Q3','Q4'],metadata
    assert page.locator('#sheetSelect option').all_text_contents()==['Q1','Q2','Q3','Q4']
    for i in range(4):
        page.locator(f'#unifiedWidePeriod{i}').select_option('q'+str(i+1))
        assert page.locator(f'#unifiedWideSheet{i}').is_checked()
    assert page.locator('#unifiedWideSheetMode').input_value()=='selected'
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
    wait_for_text(page,'#unifiedImportStatus','Сохранено: 366 отчётов',120_000)
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
    assert api(page,'nativeRevision')==before


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
          .map(node=>node.value),restore:[...document.querySelector('#importBackup').parentNode.parentNode.parentNode
          .querySelectorAll(':scope > p')].map(node=>node.textContent)})''')

    confirmations.cancel_only(page.locator('#saveOrganizations'),
        'Изменить названия подразделений во всех отчётах и справочнике? '
        'Сначала будет создана резервная копия SQLite.',drafts=drafts)
    fields.first.fill(original)
    confirmations.cancel_only(page.locator('#clearReports'),
        'Удалить все импортированные отчёты? Группы останутся.',drafts=drafts)
    for removed in ('Создать отдельную восстановленную копию',
                    'Восстановить при следующем запуске',
                    'Отменить запланированное восстановление'):
        expect(page.get_by_role('button', name=removed, exact=True)).to_have_count(0)
    navigate(page,'upload')
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
    page.locator('#uploadedReports > summary').click()
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
    page.locator('#unifiedSource').select_option('daily')
    page.locator('#periodType').select_option('month')
    wait_for_app_state(page,'document.getElementById("overviewPeriod").type==="month"','Monthly period input')
    page.locator('#overviewMode').select_option('single')
    set_calendar_field(page, 'overviewPeriod', '2024-02')
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
    filters.locator('[data-filter="type"]').get_by_role('button',name='День',exact=True).click()
    filters.locator('[data-filter="mode"]').get_by_role('button',name='Один период',exact=True).click()
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
    set_calendar_field(page, 'overviewPeriod', '2025-W02')
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
    page.locator('#overviewMode').select_option('range')
    set_calendar_field(page, 'rangeStart', '2025-W02')
    set_calendar_field(page, 'rangeEnd', '2025-W07')
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
    set_calendar_field(page, 'rangeStart', '2025-W01')
    set_calendar_field(page, 'rangeEnd', '2025-W02')
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
    page.locator('#overviewMode').select_option('single')
    dashboard_month(page)


def check_extended(page):
    assert page.locator('#printSummary').is_enabled()
    page.emulate_media(media='print')
    try:
        assert page.evaluate('getComputedStyle(document.body).backgroundColor')=='rgb(255, 255, 255)'
        assert not page.locator('main.container').is_visible()
    finally:
        page.emulate_media(media='screen')
    set_calendar_field(page, 'analysisBasePeriod', '2023-12')
    wait_for_app_state(page,
        'window.unifiedUI.scope?.baseFrom==="2023-12-01" && window.unifiedUI.scope?.baseTo==="2023-12-31"',
        'Shared baseline calendar bounds')
    navigate(page,'comparison')
    wait_for_text(page,'#comparisonCoverage','Второй интервал: Полные данные',90_000)
    date=lambda index: page.locator(f'#refComparisonDates [data-compare-date="{index}"]')
    expect(date(0)).to_have_text('01.12.2023')
    page.locator('#refComparisonDates [data-edit-base]').click()
    choose_calendar_date(page,date(0),'2024-01-01')
    choose_calendar_date(page,date(1),'2024-01-31')
    navigate(page,'dashboard')
    set_calendar_field(page, 'analysisBasePeriod', '2023-11')
    wait_for_app_state(page,'window.unifiedUI.scope?.baseFrom==="2023-11-01"','Changed shared baseline')
    navigate(page,'comparison')
    expect(date(0)).to_have_text('01.01.2024')
    reset=page.locator('#refComparisonDates [data-reset-base]')
    reset.click()
    expect(date(0)).to_have_text('01.11.2023')
    expect(reset).to_be_hidden()
    # Both missing intervals show a reason, with no meaningless pager/CSV.
    page.locator('#refComparisonDates [data-edit-base]').click()
    choose_calendar_date(page,date(3),'2023-12-31')
    choose_calendar_date(page,date(2),'2023-12-01')
    wait_for_text(page,'#comparisonCoverage','базового и анализируемого периодов',90_000)
    expect(page.locator('#comparisonTable')).to_contain_text('Нет доступных значений для сравнения')
    expect(page.locator('#comparisonTable table')).to_have_count(0)
    expect(page.locator('#comparisonTable .change-pagination')).to_have_count(0)
    expect(page.locator('#comparisonTable button')).to_have_count(0)
    page.locator('#comparison').screenshot(path=str(ROOT/'tauri/test-artifacts/windows-sql-empty-comparison.png'))
    reset.click()
    wait_for_text(page,'#comparisonCoverage','Второй интервал: Полные данные',90_000)
    navigate(page,'dashboard')
    set_calendar_field(page, 'analysisBasePeriod', '2024-01')
    wait_for_app_state(page,'window.unifiedUI.scope?.baseFrom==="2024-01-01"','January shared baseline')
    navigate(page,'comparison')
    wait_for_text(page,'#comparisonCoverage','Первый интервал: Полные данные',90_000)
    page.locator('#contributions > summary').click()
    wait_for_text(page,'#contributionCheck','совпадает',90_000)
    navigate(page,'structurePanel')
    expect(page.locator('#chartPanel')).to_be_hidden()
    wait_for_text(page,'#structureResults','Итог:',90_000)
    trend=page.locator('.tool-tabs [data-section="chartPanel"]')
    trend.focus();trend.press('Enter')
    expect(page.locator('#workViewMode')).to_have_value('trend')
    expect(page.locator('#chartPanel')).to_be_visible()
    expect(page.locator('#structurePanel')).to_be_hidden()
    for control,result in (('usualPanel','usualCoverage'),('regularityPanel','regularityCoverage')):
        navigate(page,control)
        wait_for_text(page,f'#{result}','Полных периодов:',90_000)
    navigate(page,'changes')
    page.locator('#changesType').select_option('month')
    set_calendar_field(page, 'changesTo', '2024-02')
    expect(page.locator('[data-sql-pager="changesResults"]')).to_have_attribute('data-sql-window','0',timeout=90_000)
    # The pager can remain mounted while a date change reloads the KPIs.
    expect(page.locator('#changesSummary .kpi-action')).to_have_count(4,timeout=90_000)
    expect(page.locator('#changesSummary')).to_have_attribute('aria-busy','false')
    assert page.locator('#changesResults tbody tr').count()<=100
    slider=page.locator('#changesPercent')
    slider.focus();slider.press('End')
    expect(page.locator('#changesSensitivityValue')).to_have_text('200%')
    expect(page.locator('#changesSensitivityExample')).to_contain_text('200')
    slider.press('Home')
    expect(page.locator('#changesSensitivityValue')).to_have_text('0%')
    page.locator('#changesSummary .kpi-action').first.click()
    expect(page.locator('#changesActiveFilter')).to_contain_text('рост')
    expect(page.locator('#changesDirection')).to_have_value('up')


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
    expect(group.locator('summary')).to_be_visible(timeout=90_000)
    if not group.evaluate('node=>node.open'):
        group.locator('summary').click()
    assert confirmations.revision()>initial_revision
    member=group.locator('.group-member').filter(has_text='Альфа')
    member.locator('select').select_option('',timeout=90_000)
    wait_for_text(page,'#flash','Работа перенесена.',90_000)
    dismiss_notice=page.locator('#flash').get_by_role('button',name='Закрыть уведомление',exact=True)
    dismiss_notice.click()
    expect(page.locator('#flash')).to_be_hidden()
    snapshot=api(page,'nativeSettingsBootstrap')
    assert snapshot['memberships']==0 and snapshot['groups']==0,snapshot
    assert group.count()==0
    # Removing the final member intentionally removes its source group.
    # Recreate it to verify the separate explicit-delete confirmation path.
    group=create()
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
    navigate(page,'upload')
    loaded=page.locator('#uploadedReports')
    if not loaded.evaluate('node=>node.open'):
        loaded.locator(':scope > summary').click()
    card=page.locator('#reports .calendar-source').first
    expect(card.locator('strong')).to_contain_text('2030-01-06',timeout=90_000)
    confirmations.cancel_then_accept(card.get_by_role('button',name='Удалить отчёт',exact=True),
        'Удалить отчёт 2030-01-06 — 2030-01-06?')
    wait_for_app_state(page,
        "typeof window.unifiedUI.snapshot?.reports==='number' && window.unifiedUI.snapshot.reports==="+
        str(before['reports']+5),'Confirmed single-report deletion',90_000)
    remaining=backup_contents(database)
    assert remaining[0]==[row for row in stored[0] if row[0]!=rows[0]['id']]
    assert remaining[1:]==stored[1:]
    assert_rejected(lambda:api(page,'nativeDeleteReport',revision,rows[1]['id']),
                    'An old single-delete selection was accepted','stale storage revision')
    navigate(page,'storage')
    clear_revision=api(page,'nativeSettingsBootstrap')['revision']
    confirmations.cancel_then_accept(page.locator('#clearReports'),
                                    'Удалить все импортированные отчёты? Группы останутся.')
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
                report['checks'].append('Populated summary CSV cancellation and read-only backup verification preserve data; retained restore IPC copies without overwrite and schedules/cancels only the synthetic backup')
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
                navigate(page,'upload')
                wait_for_text(page,'#qualitySummary','Найдено:',90_000)
                assert page.locator('#qualityTable tbody tr').count()<=50
                assert page.locator('#uploadedReports > summary').is_visible()
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
                navigate(page,'storage')
                page.locator('#storage').get_by_text('Восстановление выполнено',exact=False).wait_for()
                assert workbook.read_bytes()==original_xlsx
                report['checks'].append('Installed restart restores exactly the SQLite backup, retains the replaced data and displays confirmation')
                check_source_evidence(page,tmp,output)
                report['checks'].append('Saved source evidence distinguishes zero provenance, source-day gaps, unknown organization and acknowledgment without changing source XLSX or read-only revision')
                check_omitted_detail_evidence(page,tmp,output)
                report['checks'].append('Native omitted-detail tags distinguish invalid data, unknown organizations, invalid controls and included unnamed rows; persisted totals remain 3 and 8 with explicit acknowledgment')
                confirmations=NativeConfirmations(pid,Path(api(page,'info')['dataFile']))
                check_delete_without_backup(page,tmp,confirmations)
                report['checks'].append('Single report and clear-all deletion keep Cancel/Close/Escape, reject stale revisions and preserve settings, metrics and XLSX without a backup chooser')
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
