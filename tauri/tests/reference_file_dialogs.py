"""Cancel only verified, owned Windows common file dialogs in the installed app.

The empty-database walkthrough only cancels. The after-data walkthrough opens an
isolated backup for read-only verification, then tests copy/schedule/cancel via
real IPC on that backup. It never accepts a Save destination or replaces the live
database. No dialog/IPC API is replaced.
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
from itertools import zip_longest

from playwright.sync_api import Error as PlaywrightError, expect

TIMEOUT = 90_000


def draft_changes(before, after, picker=None):
    """Compare saved choices/parameters, keeping exact values in failures.

    Only the cancelled restore-source chooser may replace an empty status with
    its cancellation notice. Every other picker, reverse transition and existing
    verified preview stays exact.
    """
    changes = []
    for left, right in zip_longest(before.get('fields', []), after.get('fields', [])):
        if left != right:
            changes.append({'id': (left or right).get('id'), 'before': left, 'after': right})
    for key in sorted((before.keys() | after.keys()) - {'fields'}):
        left, right = before.get(key), after.get(key)
        if left == right:
            continue
        if (picker == 'sqlite-restore' and key == 'restorePreview' and
                left == '' and right == 'Выбор отменён.'):
            continue
        changes.append({'id': key, 'before': left, 'after': right})
    return changes


def windows_input_type():
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

    return Input


class TitleBarInfoEx(ctypes.Structure):
    _fields_ = [('cbSize', wintypes.DWORD), ('rcTitleBar', wintypes.RECT),
                ('rgstate', wintypes.DWORD * 6), ('rgrect', wintypes.RECT * 6)]


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

    def message(self, hwnd, message, wparam=0, lparam=0):
        result = ctypes.c_size_t()
        if not self.user32.SendMessageTimeoutW(hwnd, message, wparam, lparam, 0x0002, 3_000,
                                               ctypes.byref(result)):
            raise ctypes.WinError(ctypes.get_last_error())
        return result.value

    def verify_window(self, hwnd, owner, title):
        assert self.native.process(hwnd) == self.native.pid and self.title(hwnd) == title
        assert self.user32.GetWindow(hwnd, 4) == owner and self.native.process(owner) == self.native.pid
        assert self.user32.IsWindowVisible(hwnd) and self.user32.IsWindowEnabled(hwnd)
        assert not self.user32.IsWindowEnabled(owner)

    def titlebar_info(self, hwnd):
        """Read the native Close slot; no input and no inference from UIA roles."""
        info = TitleBarInfoEx()
        info.cbSize = ctypes.sizeof(info)
        self.message(hwnd, 0x033F, 0, ctypes.addressof(info))  # WM_GETTITLEBARINFOEX
        rect = info.rgrect[5]
        return {'closeRect': [rect.left, rect.top, rect.right, rect.bottom],
                'state': int(info.rgstate[5])}

    def close_titlebar(self, hwnd, owner, title, screenshot=None):
        """Physically click only the exact window's enabled Win32 close region.

        Reusable for other owned native modal dialogs whose title/PID/owner have
        been independently identified. Never sends a window-close command.
        """
        facts = {'hwnd': int(hwnd), 'owner': int(owner), 'title': title, 'inputSent': False}
        try:
            self.verify_window(hwnd, owner, title)
            self.user32.GetWindowRect.restype = wintypes.BOOL
            self.user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
            self.user32.WindowFromPoint.restype = wintypes.HWND
            self.user32.WindowFromPoint.argtypes = [wintypes.POINT]
            self.user32.GetForegroundWindow.restype = wintypes.HWND
            self.user32.GetForegroundWindow.argtypes = []
            self.user32.GetSystemMetrics.restype = ctypes.c_int
            self.user32.GetSystemMetrics.argtypes = [ctypes.c_int]
            facts.update(self.titlebar_info(hwnd))
            rect = wintypes.RECT(*facts['closeRect'])
            state = facts['state']
            assert not state & (0x1 | 0x8000 | 0x10000), ('Close unavailable/invisible/offscreen', facts)
            assert rect.right > rect.left and rect.bottom > rect.top, ('No native Close bounds', facts)
            window = wintypes.RECT()
            assert self.user32.GetWindowRect(hwnd, ctypes.byref(window)), facts
            assert (window.left <= rect.left < rect.right <= window.right and
                    window.top <= rect.top < rect.bottom <= window.bottom), facts
            x, y = (rect.left + rect.right) // 2, (rect.top + rect.bottom) // 2
            assert -32768 <= x <= 32767 and -32768 <= y <= 32767, facts
            packed = (x & 0xFFFF) | ((y & 0xFFFF) << 16)
            facts.update(x=x, y=y, hitTest=self.message(hwnd, 0x0084, 0, packed))
            assert facts['hitTest'] == 20, ('Native point is not HTCLOSE', facts)
            assert self.user32.WindowFromPoint(wintypes.POINT(x, y)) == hwnd, ('Close point covered', facts)
            assert self.user32.GetForegroundWindow() == hwnd, ('Close dialog is not foreground', facts)
            left, top, width, height = [self.user32.GetSystemMetrics(i) for i in (76, 77, 78, 79)]
            assert width > 1 and height > 1 and left <= x < left + width and top <= y < top + height, facts
            Input = windows_input_type()
            self.user32.SendInput.restype = wintypes.UINT
            self.user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(Input), ctypes.c_int]
            mouse = (Input * 3)()
            mouse[0].mi.dx = round((x - left) * 65535 / (width - 1))
            mouse[0].mi.dy = round((y - top) * 65535 / (height - 1))
            mouse[0].mi.dwFlags = 0x0001 | 0x8000 | 0x4000  # MOVE | ABSOLUTE | VIRTUALDESK
            mouse[1].mi.dwFlags, mouse[2].mi.dwFlags = 0x0002, 0x0004  # LEFTDOWN, LEFTUP
            self.verify_window(hwnd, owner, title)
            assert self.message(hwnd, 0x0084, 0, packed) == 20, facts
            assert self.user32.WindowFromPoint(wintypes.POINT(x, y)) == hwnd, facts
            assert self.user32.GetForegroundWindow() == hwnd, facts
            if self.user32.SendInput(3, mouse, ctypes.sizeof(Input)) != 3:
                raise ctypes.WinError(ctypes.get_last_error())
            facts['inputSent'] = True
            return facts
        finally:
            if screenshot:
                record = Path(screenshot).with_name('native-file-dialogs-titlebar-' + Path(screenshot).stem + '.json')
                record.write_text(json.dumps(facts, ensure_ascii=False, indent=2), encoding='utf-8')

    def backup_open_controls(self, hwnd, facts):
        # Actual installed Open dialog: ComboBoxEx32(1148) > ComboBox(1148)
        # > Edit(1148). UIA reports all three and IDOK as Pane, not Edit/Button.
        edits = [n['handle'] for n in facts['nativeChildren']
                 if n['controlId'] == 1148 and n['class'].lower() == 'edit' and
                 n['pid'] == self.native.pid and n['child'] and n['visible'] and n['enabled']]
        assert len(edits) == 1, ('No unambiguous native file-name Edit(1148)', facts)
        edit = edits[0]
        for expected_class in ('combobox', 'comboboxex32'):
            parent = self.user32.GetParent(edit)
            assert self.native.process(parent) == self.native.pid and self.user32.IsChild(hwnd, parent), facts
            assert self.user32.GetDlgCtrlID(parent) == 1148, facts
            assert self.native.window_class(parent).lower() == expected_class, facts
            edit = parent
        edit = edits[0]
        assert self.native.process(edit) == self.native.pid and self.user32.IsChild(hwnd, edit), facts
        assert self.user32.GetDlgCtrlID(edit) == 1148 and self.native.window_class(edit).lower() == 'edit', facts
        assert self.user32.IsWindowVisible(edit) and self.user32.IsWindowEnabled(edit), facts
        opened = self.user32.GetDlgItem(hwnd, 1)
        assert opened and self.native.process(opened) == self.native.pid, facts
        assert self.user32.IsChild(hwnd, opened) and self.user32.GetDlgCtrlID(opened) == 1, facts
        assert self.native.window_class(opened).lower() == 'button', facts
        assert self.user32.IsWindowVisible(opened) and self.user32.IsWindowEnabled(opened), facts
        return edit, opened

    def escape(self, hwnd):
        # Send a real key only while Windows itself reports focus inside this
        # exact verified file dialog. Never use ambient/global SendKeys.
        class GuiThreadInfo(ctypes.Structure):
            _fields_ = [('cbSize', wintypes.DWORD), ('flags', wintypes.DWORD),
                        ('hwndActive', wintypes.HWND), ('hwndFocus', wintypes.HWND),
                        ('hwndCapture', wintypes.HWND), ('hwndMenuOwner', wintypes.HWND),
                        ('hwndMoveSize', wintypes.HWND), ('hwndCaret', wintypes.HWND),
                        ('rcCaret', wintypes.RECT)]

        Input = windows_input_type()

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

    def inspect(self, hwnd, title, screenshot=None, require_cancel=True):
        owner = self.user32.GetWindow(hwnd, 4)
        for attempt in range(1, 4):
            self.verify_window(hwnd, owner, title)
            assert self.native.window_class(hwnd) == '#32770'
            result = subprocess.run(
                ['powershell', '-NoProfile', '-NonInteractive', '-Command', self.inspect_script],
                env=dict(os.environ, ANALIZ_SMOKE_DIALOG_PID=str(self.native.pid),
                         ANALIZ_SMOKE_DIALOG_HWND=str(hwnd), ANALIZ_SMOKE_FILE_TITLE=title,
                         ANALIZ_SMOKE_FILE_SCREENSHOT=str(screenshot) if screenshot else ''),
                capture_output=True, encoding='utf-8', errors='replace', timeout=20)
            self.verify_window(hwnd, owner, title)
            if not result.returncode:
                break
            error = result.stderr.strip()
            # A common file dialog can rebuild its UIA descendant tree while
            # FindAll reads it. Retry only that provider error, on the same
            # still-visible owned window. Never infer controls or send input.
            if attempt == 3 or 'ElementNotAvailableException' not in error:
                raise AssertionError('Owned file-dialog UIA verification failed: ' + error)
            time.sleep(.1)
        facts = json.loads(result.stdout)
        assert facts['pid'] == self.native.pid and facts['title'] == title, facts
        facts['uiaReadAttempts'] = attempt
        facts['nativeChildren'] = self.native_inventory(hwnd)
        facts['dialogHwnd'] = int(hwnd)
        if screenshot:
            record = Path(screenshot).with_name('native-file-dialogs-inventory-' + Path(screenshot).stem + '.json')
            record.write_text(json.dumps(facts, ensure_ascii=False, indent=2), encoding='utf-8')
        self.verify_window(hwnd, owner, title)
        cancel = self.cancel_control(hwnd, facts) if require_cancel else None
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
        self.verify_window(hwnd, owner, title)
        assert self.native.window_class(hwnd) == '#32770'
        backup = Path(backup)
        assert backup.is_absolute() and backup.is_file() and backup.suffix == '.sqlite3'
        facts, _ = self.inspect(hwnd, title, screenshot)
        edit, opened = self.backup_open_controls(hwnd, facts)
        text = ctypes.create_unicode_buffer(str(backup))
        assert self.message(edit, 0x000C, 0, ctypes.addressof(text)), 'WM_SETTEXT refused backup path'
        readback = ctypes.create_unicode_buffer(len(str(backup)) + 2)
        self.message(edit, 0x000D, len(readback), ctypes.addressof(readback))  # WM_GETTEXT, cross-process
        assert readback.value == str(backup), 'Verified file-name edit did not retain the backup path'
        self.verify_window(hwnd, owner, title)
        assert self.backup_open_controls(hwnd, facts) == (edit, opened)
        self.message(opened, 0x00F5)  # BM_CLICK only on verified IDOK in restore-preview Open
        fact = {'action': 'open-backup', 'controlId': 1, 'fileNameControlId': 1148,
                'fileNameClass': 'Edit', 'delivery': 'WM_SETTEXT + BM_CLICK verified IDOK'}
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
        # Press the real visible title-bar button, independently verified
        # IDCANCEL, or a physical Escape key. No window-close message fallback.
        assert how in ('cancel', 'close', 'escape')
        focus, close = None, None
        if how == 'escape':
            focus = self.escape(hwnd)
        elif how == 'close':
            close = self.close_titlebar(hwnd, owner, title, screenshot)
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
                        'escapeFocusHwnd': focus, 'closeTarget': close,
                        'delivery': 'SendInput VK_ESCAPE down/up' if how == 'escape' else
                                    'BM_CLICK IDCANCEL' if how == 'cancel' else 'SendInput mouse click verified HTCLOSE'}
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
        key = {'Отчёты': 'upload', 'Группы работ': 'groups', 'Настройки': 'storage'}[name]
        tab = self.page.locator(f'.tool-tabs [data-section="{key}"]')
        if not tab.is_visible():
            self.page.locator('#refImport' if key == 'upload' else '#refSettings').click()
        if tab.get_attribute('aria-pressed') != 'true':
            tab.click()
        expect(self.page.locator('#' + key)).to_be_visible(timeout=TIMEOUT)
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
            after = self.drafts()
            changes = draft_changes(before, after, picker=name)
            if changes:
                record = self.output / ('native-file-dialogs-draft-diff-' + name + '-' + how + '.json')
                record.write_text(json.dumps({'before': before, 'after': after, 'changes': changes},
                                             ensure_ascii=False, indent=2), encoding='utf-8')
            assert not changes, ('Cancel changed file/parameter drafts', name, how, changes)
            assert self.native.revision() == self.revision, ('Cancel wrote SQLite', name)
            caption = {'cancel': 'настоящая кнопка Cancel (IDCANCEL=2)',
                       'close': 'видимая кнопка × (SendInput, Win32 HTCLOSE)',
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
    expect(page.locator('dialog:modal')).to_have_count(0)
    walk.evidence['notReachableOnEmptyDatabase'] = [
        'Summary CSV is exercised after importing test data']

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
    """Summary Save cancellation, read-only backup UI and retained restore IPC.

    Call immediately after check_reference_data(), before opening other sections
    and the subsequent data-mutating scenarios in windows_smoke.py.
    """
    walk = ReferenceFileDialogWalkthrough(page, nativeConfirmations, output,
                                         'native-file-dialogs-data')
    revision = walk.revision
    reports = page.evaluate('window.unifiedUI.snapshot.reports')
    assert reports > 0, 'Populated summary is required for the actual CSV Save route'
    expect(page.locator('dialog:modal')).to_have_count(0)
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
        for removed in ('Создать отдельную восстановленную копию',
                        'Восстановить при следующем запуске',
                        'Отменить запланированное восстановление'):
            expect(page.get_by_role('button', name=removed, exact=True)).to_have_count(0)
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
        for removed in ('Создать отдельную восстановленную копию',
                        'Восстановить при следующем запуске',
                        'Отменить запланированное восстановление'):
            expect(page.get_by_role('button', name=removed, exact=True)).to_have_count(0)

        # Restoring is no longer a user action in Settings. Preserve coverage of
        # the supported native APIs using only this runner's synthetic backup.
        reviewed = page.evaluate('source => window.desktopAPI.nativeRestorePreview(source)', str(backup))
        assert reviewed['reportCount'] == reports
        destination = Path(temporary) / 'recovered-test.sqlite3'
        arguments = [str(backup), reviewed['verification'], str(destination)]
        copied = page.evaluate('args => window.desktopAPI.nativeRestoreCopy(...args)', arguments)
        assert destination.is_file() and copied['reportCount'] == reports
        assert copied['verification'] == reviewed['verification']
        verified_copy = page.evaluate('source => window.desktopAPI.nativeRestorePreview(source)', str(destination))
        assert verified_copy['verification'] == reviewed['verification']
        destination_hash = hashlib.sha256(destination.read_bytes()).hexdigest()
        try:
            page.evaluate('args => window.desktopAPI.nativeRestoreCopy(...args)', arguments)
        except PlaywrightError as error:
            assert 'уже существует' in str(error), str(error)
        else:
            raise AssertionError('Restore IPC overwrote an existing destination')
        assert hashlib.sha256(destination.read_bytes()).hexdigest() == destination_hash
        scheduled = page.evaluate('args => window.desktopAPI.nativeRestoreSchedule(...args)', arguments[:2])
        try:
            assert scheduled['restartRequired'] is True and scheduled['reportCount'] == reports
        finally:
            assert page.evaluate('() => window.desktopAPI.nativeRestoreCancel()') is True
        assert page.evaluate('() => window.desktopAPI.nativeRestoreCancel()') is False
        assert hashlib.sha256(backup.read_bytes()).hexdigest() == original
        assert sorted(p.name for p in Path(temporary).glob('*.sqlite3')) == sorted([backup.name, destination.name])
        assert nativeConfirmations.revision() == revision
        walk.evidence['restoreIpc'] = dict(reportCount=reports, verification=reviewed['verification'],
            independentCopy=True, refusedOverwrite=True, scheduledThenCancelled=True,
            repeatedCancel=False, liveRevisionUnchanged=True)
        (walk.output / (walk.record_name + '.json')).write_text(
            json.dumps(walk.evidence, ensure_ascii=False, indent=2), encoding='utf-8')
        # Clear the private verified-preview draft before removing its temporary
        # source. Reload uses the real app bootstrap and retains the SQLite data.
        page.reload(wait_until='domcontentloaded')
        page.wait_for_function(
            'expected => window.unifiedUI?.snapshot?.reports === expected',
            arg=reports, timeout=TIMEOUT)
        walk.settled()
        assert nativeConfirmations.revision() == revision
        expect(page.locator('dialog:modal')).to_have_count(0)
        page.locator('#sharedHeader .main-nav').get_by_role(
            'button', name='Сводка', exact=True).click()
        page.screenshot(path=str(walk.output / 'native-file-dialogs-data-returned.png'))
