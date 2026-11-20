"""Cancel only verified, owned Windows common file dialogs in the installed app.

The empty-database walkthrough only cancels. The after-data walkthrough creates
one isolated temporary backup and opens it for read-only verification; it never
accepts a Save destination or restores/imports data. No dialog/IPC API is replaced.
"""
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from playwright.sync_api import expect

TIMEOUT = 90_000


class OwnedFileDialogs:
    """Reuse NativeConfirmations' PID/window primitives, never global SendKeys."""

    inspect_script = r"""
    [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $ErrorActionPreference='Stop'
    Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
    $pidExpected=[int]$env:ANALIZ_SMOKE_DIALOG_PID
    $hwndExpected=[long]$env:ANALIZ_SMOKE_DIALOG_HWND
    $dialog=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$hwndExpected)
    if ($null -eq $dialog -or $dialog.Current.ProcessId -ne $pidExpected -or
        $dialog.Current.Name -cne $env:ANALIZ_SMOKE_FILE_TITLE) {
      throw 'File dialog identity/title changed'
    }
    $nodes=$dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                           [System.Windows.Automation.Condition]::TrueCondition)
    $cancel=@();$inventory=@()
    foreach ($node in $nodes) {
      $c=$node.Current
      $inventory += [PSCustomObject]@{pid=$c.ProcessId;type=$c.ControlType.ProgrammaticName;
        name=$c.Name;automationId=$c.AutomationId;enabled=$c.IsEnabled;
        offscreen=$c.IsOffscreen;handle=[long]$c.NativeWindowHandle}
      if ($c.ProcessId -eq $pidExpected -and
          $c.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
          $c.AutomationId -eq '2') {
        $cancel += [PSCustomObject]@{automationId=$c.AutomationId;
          enabled=$c.IsEnabled;handle=[long]$c.NativeWindowHandle}
      }
    }
    if ($env:ANALIZ_SMOKE_FILE_SCREENSHOT) {
      Add-Type -AssemblyName System.Drawing
      $r=$dialog.Current.BoundingRectangle
      if ($r.Width -le 0 -or $r.Height -le 0) { throw 'File dialog has no visible bounds' }
      $bitmap=[System.Drawing.Bitmap]::new([int]$r.Width,[int]$r.Height)
      $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
      try {
        $graphics.CopyFromScreen([int]$r.X,[int]$r.Y,0,0,$bitmap.Size)
        $bitmap.Save($env:ANALIZ_SMOKE_FILE_SCREENSHOT)
      } finally { $graphics.Dispose();$bitmap.Dispose() }
    }
    [PSCustomObject]@{pid=$dialog.Current.ProcessId;title=$dialog.Current.Name;
      cancel=$cancel;inventory=$inventory} | ConvertTo-Json -Depth 5 -Compress
    """

    action_script = r"""
    [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
    $ErrorActionPreference='Stop'
    Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
    $pidExpected=[int]$env:ANALIZ_SMOKE_DIALOG_PID
    $dialog=[System.Windows.Automation.AutomationElement]::FromHandle(
      [IntPtr][long]$env:ANALIZ_SMOKE_DIALOG_HWND)
    if ($null -eq $dialog -or $dialog.Current.ProcessId -ne $pidExpected -or
        $dialog.Current.Name -cne $env:ANALIZ_SMOKE_FILE_TITLE -or
        -not $dialog.Current.IsEnabled -or $dialog.Current.IsOffscreen) {
      throw 'File dialog identity/title/visibility changed'
    }
    $tree=[System.Windows.Automation.TreeScope]::Descendants
    $all=[System.Windows.Automation.Condition]::TrueCondition
    $nodes=$dialog.FindAll($tree,$all)
    if ($env:ANALIZ_SMOKE_FILE_ACTION -eq 'close') {
      $buttons=@()
      foreach ($bar in $nodes) {
        if ($bar.Current.ProcessId -ne $pidExpected -or
            $bar.Current.ControlType.ProgrammaticName -ne 'ControlType.TitleBar') {continue}
        foreach ($node in $bar.FindAll($tree,$all)) {
          $c=$node.Current
          if ($c.ProcessId -eq $pidExpected -and
              $c.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
              $c.AutomationId -eq 'Close' -and $c.IsEnabled -and -not $c.IsOffscreen) {
            $buttons += $node
          }
        }
      }
      if ($buttons.Count -ne 1) {throw 'Expected exactly one visible enabled TitleBar.Close'}
      $invoke=$null
      if (-not $buttons[0].TryGetCurrentPattern(
          [System.Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)) {
        throw 'The actual title-bar close button has no Invoke pattern'
      }
      $invoke.Invoke()
      [PSCustomObject]@{action='close';automationId='Close';parentType='ControlType.TitleBar'} |
        ConvertTo-Json -Compress
    } elseif ($env:ANALIZ_SMOKE_FILE_ACTION -eq 'open-backup') {
      # Accept only the known temporary backup for read-only preview, never Save.
      $edits=@();$open=@()
      foreach ($node in $nodes) {
        $c=$node.Current
        if ($c.ProcessId -ne $pidExpected) {continue}
        if ($c.ControlType.ProgrammaticName -eq 'ControlType.ComboBox' -and
            $c.AutomationId -eq '1148') {
          foreach ($edit in $node.FindAll($tree,$all)) {
            if ($edit.Current.ProcessId -eq $pidExpected -and
                $edit.Current.ControlType.ProgrammaticName -eq 'ControlType.Edit' -and
                $edit.Current.AutomationId -eq '1001' -and $edit.Current.IsEnabled) {
              $edits += $edit
            }
          }
        }
        if ($c.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
            $c.AutomationId -eq '1' -and $c.IsEnabled -and -not $c.IsOffscreen) {
          $open += $node
        }
      }
      if ($edits.Count -ne 1 -or $open.Count -ne 1) {throw 'Ambiguous file-name/Open controls'}
      $path=$env:ANALIZ_SMOKE_BACKUP_PATH
      if (-not [System.IO.Path]::IsPathRooted($path) -or
          -not [System.IO.File]::Exists($path)) {throw 'Temporary backup path is not a real absolute file'}
      $value=$null
      if (-not $edits[0].TryGetCurrentPattern(
          [System.Windows.Automation.ValuePattern]::Pattern,[ref]$value)) {
        throw 'The verified file-name edit has no Value pattern'
      }
      $value.SetValue($path)
      if ($value.Current.Value -cne $path) {throw 'File-name edit did not retain the backup path'}
      if ($dialog.Current.ProcessId -ne $pidExpected -or
          $dialog.Current.Name -cne $env:ANALIZ_SMOKE_FILE_TITLE) {throw 'File dialog changed before Open'}
      $invoke=$null
      if (-not $open[0].TryGetCurrentPattern(
          [System.Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)) {
        throw 'The verified Open button has no Invoke pattern'
      }
      $invoke.Invoke()
      [PSCustomObject]@{action='open-backup';automationId='1';fileNameAutomationId='1001'} |
        ConvertTo-Json -Compress
    } else {throw 'Unsupported file-dialog action'}
    """

    def action(self, hwnd, title, action, backup=None):
        result = subprocess.run(
            ['powershell', '-NoProfile', '-NonInteractive', '-Command', self.action_script],
            env=dict(os.environ, ANALIZ_SMOKE_DIALOG_PID=str(self.native.pid),
                     ANALIZ_SMOKE_DIALOG_HWND=str(hwnd), ANALIZ_SMOKE_FILE_TITLE=title,
                     ANALIZ_SMOKE_FILE_ACTION=action, ANALIZ_SMOKE_BACKUP_PATH=str(backup or '')),
            capture_output=True, encoding='utf-8', errors='replace', timeout=20)
        if result.returncode:
            raise AssertionError('Owned file-dialog action failed: ' + result.stderr.strip())
        fact = json.loads(result.stdout)
        assert fact['action'] == action, fact
        return fact

    def __init__(self, confirmations):
        self.native = confirmations
        self.user32 = confirmations.user32
        self.user32.GetWindowTextW.restype = ctypes.c_int
        self.user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        self.user32.GetDlgCtrlID.restype = ctypes.c_int
        self.user32.GetDlgCtrlID.argtypes = [wintypes.HWND]
        self.user32.GetDlgItem.restype = wintypes.HWND
        self.user32.GetDlgItem.argtypes = [wintypes.HWND, ctypes.c_int]
        self.user32.GetParent.restype = wintypes.HWND
        self.user32.GetParent.argtypes = [wintypes.HWND]
        self.user32.IsChild.restype = wintypes.BOOL
        self.user32.IsChild.argtypes = [wintypes.HWND, wintypes.HWND]

    def native_inventory(self, hwnd):
        nodes = []
        callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        self.user32.EnumChildWindows.restype = wintypes.BOOL
        self.user32.EnumChildWindows.argtypes = [wintypes.HWND, callback_type, wintypes.LPARAM]

        @callback_type
        def collect(child, _):
            nodes.append({'handle': int(child), 'parent': int(self.user32.GetParent(child) or 0),
                          'pid': self.native.process(child), 'class': self.native.window_class(child),
                          'controlId': self.user32.GetDlgCtrlID(child), 'text': self.title(child),
                          'visible': bool(self.user32.IsWindowVisible(child)),
                          'enabled': bool(self.user32.IsWindowEnabled(child)),
                          'child': bool(self.user32.IsChild(hwnd, child))})
            return True

        # EnumChildWindows returns zero for an empty tree without setting error.
        self.user32.EnumChildWindows(hwnd, collect, 0)
        return nodes

    def cancel_control(self, hwnd, facts):
        # IFileDialog's UIA AutomationId need not equal the Win32 IDCANCEL.
        # Resolve the exact common-controls ID; never infer from text/count/order.
        cancel = self.user32.GetDlgItem(hwnd, 2)
        assert cancel, ('No native IDCANCEL child in verified file dialog', facts)
        assert self.native.process(cancel) == self.native.pid, facts
        assert self.user32.IsChild(hwnd, cancel) and self.user32.GetDlgCtrlID(cancel) == 2, facts
        assert self.native.window_class(cancel).lower() == 'button', facts
        assert self.user32.IsWindowVisible(cancel) and self.user32.IsWindowEnabled(cancel), facts
        return cancel

    def escape(self, hwnd):
        # Send a real key only while Windows itself reports focus inside this
        # exact verified file dialog. Never use ambient/global SendKeys.
        class GuiThreadInfo(ctypes.Structure):
            _fields_ = [('cbSize', wintypes.DWORD), ('flags', wintypes.DWORD),
                        ('hwndActive', wintypes.HWND), ('hwndFocus', wintypes.HWND),
                        ('hwndCapture', wintypes.HWND), ('hwndMenuOwner', wintypes.HWND),
                        ('hwndMoveSize', wintypes.HWND), ('hwndCaret', wintypes.HWND),
                        ('rcCaret', wintypes.RECT)]

        class MouseInput(ctypes.Structure):
            _fields_ = [('dx', wintypes.LONG), ('dy', wintypes.LONG),
                        ('mouseData', wintypes.DWORD), ('dwFlags', wintypes.DWORD),
                        ('time', wintypes.DWORD), ('dwExtraInfo', ctypes.c_size_t)]

        class KeyboardInput(ctypes.Structure):
            _fields_ = [('wVk', wintypes.WORD), ('wScan', wintypes.WORD),
                        ('dwFlags', wintypes.DWORD), ('time', wintypes.DWORD),
                        ('dwExtraInfo', ctypes.c_size_t)]

        class HardwareInput(ctypes.Structure):
            _fields_ = [('uMsg', wintypes.DWORD), ('wParamL', wintypes.WORD),
                        ('wParamH', wintypes.WORD)]

        class InputValue(ctypes.Union):
            _fields_ = [('mi', MouseInput), ('ki', KeyboardInput), ('hi', HardwareInput)]

        class Input(ctypes.Structure):
            _anonymous_ = ('value',)
            _fields_ = [('type', wintypes.DWORD), ('value', InputValue)]

        self.user32.GetForegroundWindow.restype = wintypes.HWND
        self.user32.GetForegroundWindow.argtypes = []
        self.user32.GetGUIThreadInfo.restype = wintypes.BOOL
        self.user32.GetGUIThreadInfo.argtypes = [wintypes.DWORD, ctypes.POINTER(GuiThreadInfo)]
        self.user32.SendInput.restype = wintypes.UINT
        self.user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(Input), ctypes.c_int]
        pid = wintypes.DWORD()
        thread = self.user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        info = GuiThreadInfo()
        info.cbSize = ctypes.sizeof(info)
        if not self.user32.GetGUIThreadInfo(thread, ctypes.byref(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        assert pid.value == self.native.pid
        assert self.user32.GetForegroundWindow() == hwnd and info.hwndActive == hwnd, (
            'Escape refused: verified dialog is not foreground/active')
        assert info.hwndFocus and self.native.process(info.hwndFocus) == self.native.pid
        assert info.hwndFocus == hwnd or self.user32.IsChild(hwnd, info.hwndFocus), (
            'Escape refused: keyboard focus left the verified dialog')
        keys = (Input * 2)()
        keys[0].type = keys[1].type = 1  # INPUT_KEYBOARD
        keys[0].ki.wVk = keys[1].ki.wVk = 0x1B  # VK_ESCAPE
        keys[1].ki.dwFlags = 0x0002  # KEYEVENTF_KEYUP
        if self.user32.SendInput(2, keys, ctypes.sizeof(Input)) != 2:
            raise ctypes.WinError(ctypes.get_last_error())
        return int(info.hwndFocus)

    def title(self, hwnd):
        text = ctypes.create_unicode_buffer(512)
        self.user32.GetWindowTextW(hwnd, text, len(text))
        return text.value

    def inspect(self, hwnd, title, screenshot=None):
        result = subprocess.run(
            ['powershell', '-NoProfile', '-NonInteractive', '-Command', self.inspect_script],
            env=dict(os.environ, ANALIZ_SMOKE_DIALOG_PID=str(self.native.pid),
                     ANALIZ_SMOKE_DIALOG_HWND=str(hwnd), ANALIZ_SMOKE_FILE_TITLE=title,
                     ANALIZ_SMOKE_FILE_SCREENSHOT=str(screenshot) if screenshot else ''),
            capture_output=True, encoding='utf-8', errors='replace', timeout=20)
        if result.returncode:
            raise AssertionError('Owned file-dialog UIA verification failed: ' + result.stderr.strip())
        facts = json.loads(result.stdout)
        facts['nativeChildren'] = self.native_inventory(hwnd)
        facts['dialogHwnd'] = int(hwnd)
        if screenshot:
            record = Path(screenshot).with_name('native-file-dialogs-inventory-' + Path(screenshot).stem + '.json')
            record.write_text(json.dumps(facts, ensure_ascii=False, indent=2), encoding='utf-8')
        assert facts['pid'] == self.native.pid and facts['title'] == title, facts
        cancel = self.cancel_control(hwnd, facts)
        return facts, cancel

    def find(self, expected):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            matches = []
            for hwnd in self.native.windows():
                if self.native.process(hwnd) != self.native.pid:
                    continue
                owner = self.user32.GetWindow(hwnd, 4)
                if (self.native.window_class(hwnd) != '#32770' or
                        not self.user32.IsWindowVisible(hwnd) or
                        self.native.process(owner) != self.native.pid):
                    continue
                title = self.title(hwnd)
                assert title == expected, ('Unexpected owned modal; refusing input', title, expected)
                matches.append((hwnd, owner))
            if matches:
                assert len(matches) == 1, ('Ambiguous native file dialog', expected, matches)
                hwnd, owner = matches[0]
                assert not self.user32.IsWindowEnabled(owner), ('File-dialog owner is not modal', expected)
                return hwnd, owner
            time.sleep(.05)
        raise AssertionError('Owned file dialog did not appear: ' + expected)

    def open_backup_for_preview(self, hwnd, owner, title, backup, screenshot):
        assert title == 'Проверить резервную копию SQLite', 'Only restore-preview Open may be accepted'
        assert self.native.process(hwnd) == self.native.pid and self.title(hwnd) == title
        assert self.user32.GetWindow(hwnd, 4) == owner and self.native.process(owner) == self.native.pid
        assert self.native.window_class(hwnd) == '#32770'
        assert not self.user32.IsWindowEnabled(owner)
        self.inspect(hwnd, title, screenshot)
        fact = self.action(hwnd, title, 'open-backup', backup)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(owner):
                return fact
            time.sleep(.05)
        raise AssertionError('Read-only backup Open did not return')

    def dismiss(self, hwnd, owner, title, how, screenshot=None):
        assert self.native.process(hwnd) == self.native.pid and self.title(hwnd) == title
        assert self.user32.GetWindow(hwnd, 4) == owner and self.native.process(owner) == self.native.pid
        assert self.native.window_class(hwnd) == '#32770'
        assert self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(hwnd)
        assert not self.user32.IsWindowEnabled(owner)
        facts, cancel = self.inspect(hwnd, title, screenshot)
        # Invoke the real visible title-bar button, independently verified
        # IDCANCEL, or a physical Escape key. No window-close message fallback.
        assert how in ('cancel', 'close', 'escape')
        focus = None
        if how == 'escape':
            focus = self.escape(hwnd)
        elif how == 'close':
            self.action(hwnd, title, 'close')
        else:
            result = ctypes.c_size_t()
            if not self.user32.SendMessageTimeoutW(cancel, 0x00F5, 0, 0, 0x0002, 3_000,
                                                   ctypes.byref(result)):
                raise ctypes.WinError(ctypes.get_last_error())
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(owner):
                return {'title': title, 'mode': how, 'pid': facts['pid'],
                        'class': '#32770', 'cancelLookup': 'GetDlgItem(IDCANCEL)', 'cancelControlId': 2,
                        'escapeFocusHwnd': focus,
                        'delivery': 'SendInput VK_ESCAPE down/up' if how == 'escape' else
                                    'BM_CLICK IDCANCEL' if how == 'cancel' else 'UIA Invoke TitleBar.Close'}
            time.sleep(.05)
        raise AssertionError('Native file dialog stayed open after ' + how + ': ' + title)


class ReferenceFileDialogWalkthrough:
    """Shared cancellation/reopen/focus proof for empty and populated workspaces."""

    def __init__(self, page, native, output, record_name='native-file-dialogs'):
        self.page, self.native = page, native
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.dialogs = OwnedFileDialogs(native)
        self.revision = native.revision()
        self.record_name = record_name
        self.evidence = {'revision': self.revision, 'dialogs': []}

    def settled(self):
        self.page.wait_for_function("""() => !window.tauriOperations.reads && !window.tauriOperations.saves
          && ![...document.querySelectorAll('[aria-busy="true"]')]
          .some(n => n.getClientRects().length)""", timeout=TIMEOUT)

    def section(self, name):
        self.page.locator('#refData').click()
        sheet = self.page.locator('#sheet')
        expect(sheet).to_be_visible()
        sheet.get_by_role('button', name=name, exact=True).click()
        expect(self.page.locator('#toolsPage')).to_be_visible(timeout=TIMEOUT)
        self.settled()

    def drafts(self):
        return self.page.evaluate("""() => ({
          fields:[...document.querySelectorAll('#refNative input[id],#refNative select[id],#refNative textarea[id]')]
            .filter(n=>n.type!=='file').map(n=>({id:n.id,value:n.value,checked:n.checked})),
          preview:document.querySelector('#importPreview')?.textContent,
          metricPreview:document.querySelector('#metricKindPreview')?.textContent,
          selectedSource:document.querySelector('#excelFileStatus')?.textContent,
          restorePreview:document.querySelector('#importBackup')?.parentNode.parentNode.parentNode.querySelector(':scope > p:last-of-type')?.textContent
        })""")

    def exercise(self, name, trigger, title, confirmation=None):
        expect(trigger).to_be_visible(timeout=TIMEOUT)
        expect(trigger).to_be_enabled(timeout=TIMEOUT)
        before = self.drafts()
        for index, how in enumerate(('cancel', 'close', 'escape')):
            if index:
                # Reopen through the restored focus, including hidden-input labels.
                expect(trigger).to_be_focused(timeout=TIMEOUT)
                trigger.press('Enter')
            else:
                trigger.click()
            if confirmation:
                dialog, parent = self.native.find_dialog(confirmation)
                self.native.answer(dialog, parent, confirmation, 1)
            hwnd, owner = self.dialogs.find(title)
            assert self.native.revision() == self.revision, name
            fact = self.dialogs.dismiss(hwnd, owner, title, how,
                                   self.output / ('native-file-' + name + '-' + how + '.png'))
            self.settled()
            expect(trigger).to_be_enabled(timeout=TIMEOUT)
            expect(trigger).to_be_focused(timeout=TIMEOUT)
            assert self.drafts() == before, ('Cancel changed file/parameter drafts', name, how)
            assert self.native.revision() == self.revision, ('Cancel wrote SQLite', name)
            caption = {'cancel': 'настоящая кнопка Cancel (IDCANCEL=2)',
                       'close': 'видимая кнопка × (UIA Invoke TitleBar.Close)',
                       'escape': 'настоящая клавиша Esc (SendInput, проверенный фокус)'}[how]
            self.evidence['dialogs'].append(dict(name=name, screenshot='native-file-' + name + '-' + how + '.png',
                                            caption=title + ' — ' + caption, **fact))
            (self.output / (self.record_name + '.json')).write_text(
                json.dumps(self.evidence, ensure_ascii=False, indent=2), encoding='utf-8')


def check_reference_file_dialogs(page, nativeConfirmations, output):
    walk = ReferenceFileDialogWalkthrough(page, nativeConfirmations, output)
    output, revision = walk.output, walk.revision
    exercise, section, settled = walk.exercise, walk.section, walk.settled
    initial_folder = page.evaluate('window.desktopAPI.list()')['folder']
    assert page.evaluate('window.unifiedUI.snapshot.reports') == 0
    assert not page.locator('#refWorkspaceDialog').is_visible()
    walk.evidence['notReachableOnEmptyDatabase'] = [
        'Summary CSV and separate restored-copy Save are exercised after importing test data']

    page.locator('#refImport').click()
    expect(page.locator('#upload')).to_be_visible(timeout=TIMEOUT)
    settled()
    exercise('report-xlsx', page.locator('#excelFile'), 'Выберите XLSX для нативного импорта')
    page.wait_for_function('typeof window.nativeFolderReady !== "undefined"', timeout=TIMEOUT)
    page.evaluate('window.nativeFolderReady')
    exercise('excel-folder', page.locator('#sourceFolderChoose'), 'Папка с Excel-отчётами')

    section('Настройки')
    exercise('sqlite-backup', page.locator('#exportBackup'), 'Сохранить резервную копию SQLite')
    exercise('sqlite-restore', page.locator('#importBackup').locator('xpath=ancestor::label[1]'),
             'Проверить резервную копию SQLite')

    section('Группы работ')
    exercise('groups-import', page.locator('#importGroups').locator('xpath=ancestor::label[1]'),
             'Импорт групп')
    exercise('groups-export', page.locator('#exportGroups'), 'Сохранить группы')
    catalog = page.locator('#groupCatalog')
    if not catalog.evaluate('n=>n.open'):
        catalog.locator(':scope > summary').click()
    exercise('catalog-xlsx', page.locator('#catalogFile').locator('xpath=ancestor::label[1]'),
             'Выберите справочник XLSX')

    section('Отчёты')
    metrics = page.locator('#additionalMetrics')
    if not metrics.evaluate('n=>n.open'):
        metrics.locator(':scope > summary').click()
    page.evaluate('window.metricKindWorkspaceReady')
    exercise('metric-json', page.locator('#metricKindChoose'), 'Выберите JSON для нового вида отчёта')

    def metric_date(field_id, selected=None, clear=False):
        field = page.locator('#' + field_id)
        trigger = field.locator('xpath=following-sibling::button[contains(@class,"ref-calendar-field")][1]')
        expect(trigger).to_be_visible(timeout=TIMEOUT)
        trigger.click()
        dialog = page.locator('dialog.period-calendar[open]')
        expect(dialog).to_be_visible()
        if clear:
            dialog.get_by_role('button', name='Очистить дату', exact=True).click()
        else:
            cell = (dialog.locator('.cal-grid button[data-stamp="' + selected + '"]:enabled')
                    if selected else dialog.locator('.cal-grid button[data-stamp]:enabled').first)
            selected = cell.get_attribute('data-stamp')
            cell.click()
        dialog.get_by_role('button', name='Подтвердить', exact=True).click()
        return selected

    assert page.locator('#metricKindFrom').input_value() == ''
    assert page.locator('#metricKindTo').input_value() == ''
    selected = metric_date('metricKindFrom')
    metric_date('metricKindTo', selected)
    exercise('metric-xlsx', page.locator('#metricKindChooseXlsx'), 'Выберите XLSX для нового вида отчёта')
    metric_date('metricKindFrom', clear=True)
    metric_date('metricKindTo', clear=True)

    page.locator('#backBtn').click()
    expect(page.locator('#toolsPage')).not_to_be_visible()
    page.locator('#sharedHeader .main-nav').get_by_role('button', name='Сводка', exact=True).click()
    assert nativeConfirmations.revision() == revision
    assert page.evaluate('window.unifiedUI.snapshot.reports') == 0
    assert page.evaluate('window.desktopAPI.list()')['folder'] == initial_folder
    page.screenshot(path=str(output / 'native-file-dialogs-returned.png'))


def check_reference_data_file_dialogs(page, nativeConfirmations, output):
    """Two Save methods reachable only with actual data and a verified backup.

    Call immediately after check_reference_data(), before open_full_workspace()
    and the subsequent data-mutating scenarios in windows_smoke.py.
    """
    walk = ReferenceFileDialogWalkthrough(page, nativeConfirmations, output,
                                         'native-file-dialogs-data')
    revision = walk.revision
    reports = page.evaluate('window.unifiedUI.snapshot.reports')
    assert reports > 0, 'Populated summary is required for the actual CSV Save route'
    assert not page.locator('#refWorkspaceDialog').is_visible()
    page.locator('#sharedHeader .main-nav').get_by_role(
        'button', name='Сводка', exact=True).click()
    walk.settled()
    csv = page.locator('#s1 .data-card .card-heading button')
    walk.exercise('summary-csv', csv, 'Сохранить результаты анализа')

    # A real backup in a unique temporary directory, never the live database.
    # The only accepted Open selects this exact file for read-only verification.
    with tempfile.TemporaryDirectory(prefix='analiz-native-dialog-') as temporary:
        backup = Path(temporary) / 'verified-test-backup.sqlite3'
        assert not backup.exists()
        page.evaluate('destination => window.desktopAPI.nativeBackupSqlite(destination)', str(backup))
        assert backup.is_file() and backup.stat().st_size > 0
        original = hashlib.sha256(backup.read_bytes()).hexdigest()
        assert nativeConfirmations.revision() == revision
        walk.section('Настройки')
        chooser = page.locator('#importBackup').locator('xpath=ancestor::label[1]')
        expect(chooser).to_be_visible()
        chooser.click()
        title = 'Проверить резервную копию SQLite'
        hwnd, owner = walk.dialogs.find(title)
        opened = walk.dialogs.open_backup_for_preview(
            hwnd, owner, title, backup, walk.output / 'native-file-verified-backup-open.png')
        walk.settled()
        status = chooser.locator('xpath=../..').locator('p').filter(
            has_text='Копия проверена. Отчётов: ' + str(reports) + '.')
        expect(status).to_be_visible(timeout=TIMEOUT)
        assert nativeConfirmations.revision() == revision
        assert hashlib.sha256(backup.read_bytes()).hexdigest() == original
        walk.evidence['backupPreview'] = dict(
            **opened, reportCount=reports, sourceSha256=original,
            screenshot='native-file-verified-backup-open.png',
            caption=title + ' — настоящий Open изолированной копии для проверки без восстановления')
        copy = page.get_by_role('button', name='Создать отдельную восстановленную копию', exact=True)
        walk.exercise('sqlite-restored-copy', copy, 'Создать отдельную восстановленную копию',
                      confirmation='Создать отдельную проверенную копию SQLite?')
        assert hashlib.sha256(backup.read_bytes()).hexdigest() == original
        assert sorted(p.name for p in Path(temporary).glob('*.sqlite3')) == [backup.name], (
            'Cancelling Save unexpectedly created a restored SQLite destination')
        assert nativeConfirmations.revision() == revision
        # Clear the private verified-preview draft before removing its temporary
        # source. Reload uses the real app bootstrap and retains the SQLite data.
        page.reload(wait_until='domcontentloaded')
        page.wait_for_function(
            'expected => window.unifiedUI?.snapshot?.reports === expected',
            arg=reports, timeout=TIMEOUT)
        walk.settled()
        assert nativeConfirmations.revision() == revision
        expect(page.locator('#refWorkspaceDialog')).not_to_be_visible(timeout=TIMEOUT)
        page.locator('#sharedHeader .main-nav').get_by_role(
            'button', name='Сводка', exact=True).click()
        page.screenshot(path=str(walk.output / 'native-file-dialogs-data-returned.png'))
