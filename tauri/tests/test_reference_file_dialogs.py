"""Regression guards for exact native dialog targets; no Windows input is sent."""
import ctypes
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

from reference_file_dialogs import OwnedFileDialogs, TitleBarInfoEx


class FileDialogCancelTests(unittest.TestCase):
    def setUp(self):
        self.dialog = OwnedFileDialogs.__new__(OwnedFileDialogs)
        self.dialog.native = SimpleNamespace(
            pid=41, process=Mock(return_value=41), window_class=Mock(return_value='Button'))
        self.dialog.user32 = SimpleNamespace(
            GetDlgItem=Mock(return_value=102), IsChild=Mock(return_value=True),
            GetDlgCtrlID=Mock(return_value=2), IsWindowVisible=Mock(return_value=True),
            IsWindowEnabled=Mock(return_value=True))
        self.facts = {'pid': 41, 'cancel': [], 'inventory': [
            {'type': 'ControlType.Button', 'automationId': 'provider-specific-id', 'name': 'Cancel'}]}

    def test_exact_native_id_accepts_missing_numeric_uia_id(self):
        self.assertEqual(self.dialog.cancel_control(101, self.facts), 102)
        self.dialog.user32.GetDlgItem.assert_called_once_with(101, 2)

    def test_no_native_id_does_not_guess_from_uia_label(self):
        self.dialog.user32.GetDlgItem.return_value = 0
        with self.assertRaisesRegex(AssertionError, 'No native IDCANCEL'):
            self.dialog.cancel_control(101, self.facts)

    def test_foreign_process_is_refused(self):
        self.dialog.native.process.return_value = 99
        self.assert_refused()

    def test_foreign_child_is_refused(self):
        self.dialog.user32.IsChild.return_value = False
        self.assert_refused()

    def test_wrong_control_id_is_refused(self):
        self.dialog.user32.GetDlgCtrlID.return_value = 1
        self.assert_refused()

    def test_nonbutton_is_refused(self):
        self.dialog.native.window_class.return_value = 'Edit'
        self.assert_refused()

    def test_hidden_button_is_refused(self):
        self.dialog.user32.IsWindowVisible.return_value = False
        self.assert_refused()

    def test_disabled_button_is_refused(self):
        self.dialog.user32.IsWindowEnabled.return_value = False
        self.assert_refused()

    def assert_refused(self):
        with self.assertRaises(AssertionError):
            self.dialog.cancel_control(101, self.facts)


class TitlebarClickTests(unittest.TestCase):
    def setUp(self):
        self.dialog = OwnedFileDialogs.__new__(OwnedFileDialogs)
        self.dialog.native = SimpleNamespace(pid=41, process=Mock(return_value=41))
        self.dialog.title = Mock(return_value='Verified file dialog')
        self.state, self.hit = 0, 20

        def message(hwnd, message, wparam=0, lparam=0):
            self.assertEqual(hwnd, 101)
            if message == 0x033F:
                info = TitleBarInfoEx.from_address(lparam)
                info.rgrect[5].left, info.rgrect[5].right = 560, 600
                info.rgrect[5].top, info.rgrect[5].bottom = 30, 60
                info.rgstate[5] = self.state
                return 0
            self.assertEqual(message, 0x0084)
            return self.hit

        def bounds(hwnd, rectangle):
            rectangle._obj.left, rectangle._obj.top = 10, 30
            rectangle._obj.right, rectangle._obj.bottom = 610, 500
            return True

        self.dialog.message = Mock(side_effect=message)
        self.dialog.user32 = SimpleNamespace(
            GetWindow=Mock(return_value=102), IsWindowVisible=Mock(return_value=True),
            IsWindowEnabled=Mock(side_effect=lambda hwnd: hwnd != 102),
            GetWindowRect=Mock(side_effect=bounds), WindowFromPoint=Mock(return_value=101),
            GetForegroundWindow=Mock(return_value=101), SendInput=Mock(return_value=3),
            GetSystemMetrics=Mock(side_effect={76: 0, 77: 0, 78: 1920, 79: 1080}.get))

    def test_clicks_only_native_close_rectangle_and_verifies_hit_test_twice(self):
        facts = self.dialog.close_titlebar(101, 102, 'Verified file dialog')
        self.assertEqual((facts['x'], facts['y'], facts['hitTest']), (580, 45, 20))
        self.assertTrue(facts['inputSent'])
        count, mouse, _ = self.dialog.user32.SendInput.call_args.args
        self.assertEqual(count, 3)
        self.assertEqual([event.type for event in mouse], [0, 0, 0])
        self.assertEqual([event.mi.dwFlags for event in mouse], [0xC001, 2, 4])
        self.assertEqual([call.args[1] for call in self.dialog.message.call_args_list],
                         [0x033F, 0x0084, 0x0084])

    def test_wrong_hit_target_never_clicks(self):
        self.hit = 1
        self.assert_refused()

    def test_hit_target_changed_before_input_never_clicks(self):
        original = self.dialog.message.side_effect
        hits = iter([20, 1])
        self.dialog.message.side_effect = lambda hwnd, message, wparam=0, lparam=0: (
            next(hits) if message == 0x0084 else original(hwnd, message, wparam, lparam))
        self.assert_refused()

    def test_covered_point_never_clicks(self):
        self.dialog.user32.WindowFromPoint.return_value = 999
        self.assert_refused()

    def test_background_dialog_never_clicks(self):
        self.dialog.user32.GetForegroundWindow.return_value = 999
        self.assert_refused()

    def test_disabled_close_never_clicks(self):
        self.state = 1
        self.assert_refused()

    def test_invisible_close_never_clicks(self):
        self.state = 0x8000
        self.assert_refused()

    def test_foreign_pid_never_clicks(self):
        self.dialog.native.process.return_value = 999
        self.assert_refused()

    def assert_refused(self):
        with self.assertRaises(AssertionError):
            self.dialog.close_titlebar(101, 102, 'Verified file dialog')
        self.dialog.user32.SendInput.assert_not_called()


class BackupOpenControlsTests(unittest.TestCase):
    def setUp(self):
        self.dialog = OwnedFileDialogs.__new__(OwnedFileDialogs)
        classes = {102: 'Button', 103: 'Edit', 104: 'ComboBox', 105: 'ComboBoxEx32'}
        self.dialog.native = SimpleNamespace(pid=41, process=Mock(return_value=41),
                                             window_class=Mock(side_effect=classes.get))
        self.dialog.user32 = SimpleNamespace(
            GetDlgItem=Mock(return_value=102), IsChild=Mock(return_value=True),
            GetParent=Mock(side_effect={103: 104, 104: 105}.get),
            GetDlgCtrlID=Mock(side_effect=lambda hwnd: 1 if hwnd == 102 else 1148),
            IsWindowVisible=Mock(return_value=True), IsWindowEnabled=Mock(return_value=True))
        self.facts = {'nativeChildren': [
            {'handle': 103, 'class': 'Edit', 'controlId': 1148, 'pid': 41,
             'child': True, 'visible': True, 'enabled': True},
            {'handle': 120, 'class': 'ToolbarWindow32', 'controlId': 1001, 'pid': 41,
             'child': True, 'visible': True, 'enabled': True}],
            'inventory': [{'handle': 103, 'type': 'ControlType.Pane', 'automationId': '1148'},
                          {'handle': 102, 'type': 'ControlType.Pane', 'automationId': '1'}]}

    def test_actual_open_hierarchy_resolves_despite_uia_pane_roles(self):
        self.assertEqual(self.dialog.backup_open_controls(101, self.facts), (103, 102))
        self.dialog.user32.GetDlgItem.assert_called_once_with(101, 1)

    def test_unrelated_address_edit_does_not_replace_file_edit(self):
        self.facts['nativeChildren'][0]['controlId'] = 1001
        self.assert_refused()

    def test_multiple_file_edits_are_refused(self):
        self.facts['nativeChildren'].append(dict(self.facts['nativeChildren'][0], handle=106))
        self.assert_refused()

    def test_foreign_combo_parent_is_refused(self):
        self.dialog.native.process.side_effect = lambda hwnd: 99 if hwnd == 104 else 41
        self.assert_refused()

    def test_nonbutton_idok_is_refused(self):
        self.dialog.native.window_class.side_effect = lambda hwnd: {
            102: 'Edit', 103: 'Edit', 104: 'ComboBox', 105: 'ComboBoxEx32'}.get(hwnd)
        self.assert_refused()

    def test_read_only_preview_sets_and_reads_back_exact_path_before_idok(self):
        self.dialog.verify_window = Mock()
        self.dialog.inspect = Mock(return_value=(self.facts, 110))
        classes = self.dialog.native.window_class.side_effect
        self.dialog.native.window_class.side_effect = lambda hwnd: '#32770' if hwnd == 101 else classes(hwnd)
        closed = False

        def message(hwnd, message, wparam=0, lparam=0):
            nonlocal closed
            if message == 0x000C:
                self.assertEqual(hwnd, 103)
                self.assertEqual(ctypes.wstring_at(lparam), str(backup))
                return 1
            if message == 0x000D:
                self.assertEqual(hwnd, 103)
                text = ctypes.create_unicode_buffer(str(backup))
                ctypes.memmove(lparam, text, ctypes.sizeof(text))
                return len(str(backup))
            self.assertEqual((hwnd, message), (102, 0x00F5))
            closed = True
            return 0

        self.dialog.message = Mock(side_effect=message)
        self.dialog.user32.IsWindowVisible.side_effect = lambda hwnd: not (hwnd == 101 and closed)
        with tempfile.TemporaryDirectory() as scratch:
            backup = Path(scratch) / 'isolated.sqlite3'
            backup.write_bytes(b'test file selection fixture')
            result = self.dialog.open_backup_for_preview(
                101, 100, 'Проверить резервную копию SQLite', backup, None)
        self.assertEqual(result['action'], 'open-backup')
        self.assertEqual([call.args[1] for call in self.dialog.message.call_args_list],
                         [0x000C, 0x000D, 0x00F5])

    def test_save_dialog_cannot_be_accepted_by_preview_helper(self):
        self.dialog.message = Mock()
        with self.assertRaisesRegex(AssertionError, 'Only restore-preview Open'):
            self.dialog.open_backup_for_preview(101, 100, 'Сохранить резервную копию SQLite',
                                                Path('unused.sqlite3'), None)
        self.dialog.message.assert_not_called()

    def assert_refused(self):
        with self.assertRaises(AssertionError):
            self.dialog.backup_open_controls(101, self.facts)


if __name__ == '__main__':
    unittest.main()
