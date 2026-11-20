"""Cancel only verified, owned Windows common file dialogs in the installed app.

The empty-database walkthrough uses visible UI routes. It never chooses a path,
imports a report, creates a backup, changes the connected folder or updates data.
No native dialog/IPC API is replaced.
"""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
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
    $cancel=@()
    foreach ($node in $nodes) {
      $c=$node.Current
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
    [PSCustomObject]@{pid=$dialog.Current.ProcessId;title=$dialog.Current.Name;cancel=$cancel} |
      ConvertTo-Json -Depth 4 -Compress
    """

    def __init__(self, confirmations):
        self.native = confirmations
        self.user32 = confirmations.user32
        self.user32.GetWindowTextW.restype = ctypes.c_int
        self.user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        self.user32.GetDlgCtrlID.restype = ctypes.c_int
        self.user32.GetDlgCtrlID.argtypes = [wintypes.HWND]
        self.user32.IsChild.restype = wintypes.BOOL
        self.user32.IsChild.argtypes = [wintypes.HWND, wintypes.HWND]

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
        assert facts['pid'] == self.native.pid and facts['title'] == title, facts
        buttons = facts['cancel']
        if isinstance(buttons, dict):
            buttons = [buttons]
        assert len(buttons) == 1 and buttons[0]['enabled'] is True, facts
        cancel = buttons[0]['handle']
        assert cancel and self.native.process(cancel) == self.native.pid, facts
        assert self.user32.IsChild(hwnd, cancel) and self.user32.GetDlgCtrlID(cancel) == 2, facts
        assert self.user32.IsWindowVisible(cancel) and self.user32.IsWindowEnabled(cancel), facts
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

    def dismiss(self, hwnd, owner, title, how, screenshot=None):
        assert self.native.process(hwnd) == self.native.pid and self.title(hwnd) == title
        assert self.user32.GetWindow(hwnd, 4) == owner and self.native.process(owner) == self.native.pid
        assert self.native.window_class(hwnd) == '#32770'
        assert self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(hwnd)
        assert not self.user32.IsWindowEnabled(owner)
        facts, cancel = self.inspect(hwnd, title, screenshot)
        # BM_CLICK on the independently verified IDCANCEL child, or the exact
        # verified dialog's normal title-bar close path. Never activate IDOK.
        assert how in ('cancel', 'close', 'escape')
        focus = None
        if how == 'escape':
            focus = self.escape(hwnd)
        else:
            target, message = (cancel, 0x00F5) if how == 'cancel' else (hwnd, 0x0010)
            result = ctypes.c_size_t()
            if not self.user32.SendMessageTimeoutW(target, message, 0, 0, 0x0002, 3_000,
                                                   ctypes.byref(result)):
                raise ctypes.WinError(ctypes.get_last_error())
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(owner):
                return {'title': title, 'mode': how, 'pid': facts['pid'],
                        'class': '#32770', 'cancelAutomationId': '2', 'cancelControlId': 2,
                        'escapeFocusHwnd': focus,
                        'delivery': 'SendInput VK_ESCAPE down/up' if how == 'escape' else
                                    'BM_CLICK IDCANCEL' if how == 'cancel' else 'WM_CLOSE'}
            time.sleep(.05)
        raise AssertionError('Native file dialog stayed open after ' + how + ': ' + title)


def check_reference_file_dialogs(page, nativeConfirmations, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    dialogs = OwnedFileDialogs(nativeConfirmations)
    revision = nativeConfirmations.revision()
    initial_folder = page.evaluate('window.desktopAPI.list()')['folder']
    assert page.evaluate('window.unifiedUI.snapshot.reports') == 0
    assert not page.locator('#refWorkspaceDialog').is_visible()
    evidence = {'revision': revision, 'dialogs': [], 'notReachableWithoutSelectingAFile': [
        'Restore destination requires a verified backup and confirmation',
        'Report/summary/detail CSV is unavailable on the empty database']}

    def settled():
        page.wait_for_function("""() => !window.tauriOperations.reads && !window.tauriOperations.saves
          && ![...document.querySelectorAll('[aria-busy="true"]')]
          .some(n => n.getClientRects().length)""", timeout=TIMEOUT)

    def section(name):
        page.locator('#refData').click()
        sheet = page.locator('#sheet')
        expect(sheet).to_be_visible()
        sheet.get_by_role('button', name=name, exact=True).click()
        expect(page.locator('#toolsPage')).to_be_visible(timeout=TIMEOUT)
        settled()

    def drafts():
        return page.evaluate("""() => ({
          fields:[...document.querySelectorAll('#refNative input[id],#refNative select[id],#refNative textarea[id]')]
            .filter(n=>n.type!=='file').map(n=>({id:n.id,value:n.value,checked:n.checked})),
          preview:document.querySelector('#importPreview')?.textContent,
          metricPreview:document.querySelector('#metricKindPreview')?.textContent,
          selectedSource:document.querySelector('#excelFileStatus')?.textContent
        })""")

    def exercise(name, trigger, title):
        expect(trigger).to_be_visible(timeout=TIMEOUT)
        expect(trigger).to_be_enabled(timeout=TIMEOUT)
        before = drafts()
        for index, how in enumerate(('cancel', 'close', 'escape')):
            if index:
                # Reopen through the restored focus, including hidden-input labels.
                expect(trigger).to_be_focused(timeout=TIMEOUT)
                trigger.press('Enter')
            else:
                trigger.click()
            hwnd, owner = dialogs.find(title)
            assert nativeConfirmations.revision() == revision, name
            fact = dialogs.dismiss(hwnd, owner, title, how,
                                   output / ('native-file-' + name + '-' + how + '.png'))
            settled()
            expect(trigger).to_be_enabled(timeout=TIMEOUT)
            expect(trigger).to_be_focused(timeout=TIMEOUT)
            assert drafts() == before, ('Cancel changed file/parameter drafts', name, how)
            assert nativeConfirmations.revision() == revision, ('Cancel wrote SQLite', name)
            caption = {'cancel': 'настоящая кнопка Cancel (IDCANCEL=2)',
                       'close': 'нормальное закрытие системного окна (WM_CLOSE)',
                       'escape': 'настоящая клавиша Esc (SendInput, проверенный фокус)'}[how]
            evidence['dialogs'].append(dict(name=name, screenshot='native-file-' + name + '-' + how + '.png',
                                            caption=title + ' — ' + caption, **fact))
            (output / 'native-file-dialogs.json').write_text(
                json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')

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
