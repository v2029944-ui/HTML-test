"""Regression guards for native IDCANCEL resolution; no Windows input is sent."""
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

from reference_file_dialogs import OwnedFileDialogs


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


if __name__ == '__main__':
    unittest.main()
