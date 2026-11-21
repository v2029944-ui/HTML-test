"""Regression guards for exact native dialog targets; no Windows input is sent."""
import ctypes
import json
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

from reference_file_dialogs import OwnedFileDialogs, TitleBarInfoEx, draft_changes
from windows_smoke import NativeConfirmations


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


class FileDialogInspectionTests(unittest.TestCase):
    transient = 'FindAll failed: Unrecognized error. FullyQualifiedErrorId: ElementNotAvailableException'

    def setUp(self):
        self.state = {'owner': 102, 'pid': 41, 'title': 'Verified file dialog', 'visible': True}
        self.dialog = OwnedFileDialogs.__new__(OwnedFileDialogs)
        self.dialog.native = SimpleNamespace(pid=41,
            process=Mock(side_effect=lambda hwnd: self.state['pid']),
            window_class=Mock(return_value='#32770'))
        self.dialog.title = Mock(side_effect=lambda hwnd: self.state['title'])
        self.dialog.user32 = SimpleNamespace(
            GetWindow=Mock(side_effect=lambda hwnd, relation: self.state['owner']),
            IsWindowVisible=Mock(side_effect=lambda hwnd: self.state['visible']),
            IsWindowEnabled=Mock(side_effect=lambda hwnd: hwnd == 101),
            SendInput=Mock(), SendMessageTimeoutW=Mock())
        self.dialog.native_inventory = Mock(return_value=[])
        self.dialog.cancel_control = Mock(return_value=103)
        self.dialog.message = Mock()
        self.run = patch('reference_file_dialogs.subprocess.run').start()
        self.sleep = patch('reference_file_dialogs.time.sleep').start()
        self.addCleanup(patch.stopall)

    def result(self, error=None, **changes):
        facts = {'pid': 41, 'title': 'Verified file dialog', 'cancel': [], 'inventory': []}
        facts.update(changes)
        return SimpleNamespace(returncode=1 if error else 0, stderr=error or '', stdout=json.dumps(facts))

    def assert_no_input(self):
        self.dialog.user32.SendInput.assert_not_called()
        self.dialog.user32.SendMessageTimeoutW.assert_not_called()
        self.dialog.message.assert_not_called()

    def test_transient_tree_is_read_again_from_the_same_verified_window(self):
        self.run.side_effect = [self.result(self.transient), self.result()]
        facts, cancel = self.dialog.inspect(101, 'Verified file dialog')
        self.assertEqual((facts['uiaReadAttempts'], cancel), (2, 103))
        self.assertEqual(self.run.call_count, 2)
        for call in self.run.call_args_list:
            self.assertEqual(call.kwargs['env']['ANALIZ_SMOKE_DIALOG_HWND'], '101')
            self.assertEqual(call.kwargs['env']['ANALIZ_SMOKE_DIALOG_PID'], '41')
            self.assertEqual(call.kwargs['env']['ANALIZ_SMOKE_FILE_TITLE'], 'Verified file dialog')
        self.dialog.native_inventory.assert_called_once_with(101)
        self.dialog.cancel_control.assert_called_once()
        self.assert_no_input()

    def test_repeated_transient_failure_is_bounded_and_never_acts(self):
        self.run.return_value = self.result(self.transient)
        with self.assertRaisesRegex(AssertionError, 'UIA verification failed'):
            self.dialog.dismiss(101, 102, 'Verified file dialog', 'cancel')
        self.assertEqual(self.run.call_count, 3)
        self.dialog.native_inventory.assert_not_called()
        self.dialog.cancel_control.assert_not_called()
        self.assert_no_input()

    def test_identity_change_after_failed_read_is_not_retried(self):
        for key, value in [('owner', 999), ('pid', 99), ('title', 'Other dialog'), ('visible', False)]:
            with self.subTest(key=key):
                self.state.update(owner=102, pid=41, title='Verified file dialog', visible=True)
                self.run.reset_mock()
                def changed(*args, **kwargs):
                    self.state[key] = value
                    return self.result(self.transient)
                self.run.side_effect = changed
                with self.assertRaises(AssertionError):
                    self.dialog.inspect(101, 'Verified file dialog')
                self.assertEqual(self.run.call_count, 1)
                self.assert_no_input()

    def test_invalid_window_is_refused_before_provider_read(self):
        self.state['visible'] = False
        self.run.return_value = self.result()
        with self.assertRaises(AssertionError):
            self.dialog.inspect(101, 'Verified file dialog')
        self.run.assert_not_called()
        self.assert_no_input()

    def test_owner_change_during_successful_provider_read_is_refused(self):
        def changed(*args, **kwargs):
            self.state['owner'] = 999
            return self.result()
        self.run.side_effect = changed
        with self.assertRaises(AssertionError):
            self.dialog.inspect(101, 'Verified file dialog')
        self.assertEqual(self.run.call_count, 1)
        self.dialog.native_inventory.assert_not_called()
        self.dialog.cancel_control.assert_not_called()
        self.assert_no_input()

    def test_unrelated_error_is_not_retried_or_acted_on(self):
        self.run.return_value = self.result('Access denied')
        with self.assertRaisesRegex(AssertionError, 'Access denied'):
            self.dialog.inspect(101, 'Verified file dialog')
        self.assertEqual(self.run.call_count, 1)
        self.sleep.assert_not_called()
        self.assert_no_input()

    def test_successful_read_with_wrong_identity_is_not_accepted(self):
        for changes in [{'pid': 99}, {'title': 'Other dialog'}]:
            with self.subTest(changes=changes):
                self.run.reset_mock()
                self.run.return_value = self.result(**changes)
                with self.assertRaises(AssertionError):
                    self.dialog.inspect(101, 'Verified file dialog')
                self.assertEqual(self.run.call_count, 1)
                self.dialog.cancel_control.assert_not_called()
                self.assert_no_input()


class DraftChangesTests(unittest.TestCase):
    def test_empty_restore_can_show_cancellation_notice_without_a_selected_backup(self):
        self.assertEqual(draft_changes({'restorePreview': ''}, {'restorePreview': 'Выбор отменён.'},
                                       picker='sqlite-restore'), [])

    def test_reverse_notice_transition_is_not_ignored(self):
        self.assertEqual(len(draft_changes({'restorePreview': 'Выбор отменён.'}, {'restorePreview': ''},
                                           picker='sqlite-restore')), 1)

    def test_other_pickers_cannot_replace_restore_status_with_notice(self):
        for picker in (None, 'groups-import', 'metric-json', 'sqlite-backup'):
            self.assertEqual(len(draft_changes({'restorePreview': ''}, {'restorePreview': 'Выбор отменён.'},
                                               picker=picker)), 1)

    def test_verified_backup_preview_cannot_be_replaced_by_empty_or_cancel_notice(self):
        verified = {'restorePreview': 'Копия проверена. Отчётов: 366. Рабочая база не изменена.'}
        for status in ('', 'Выбор отменён.'):
            changes = draft_changes(verified, {'restorePreview': status}, picker='sqlite-restore')
            self.assertEqual(changes, [{'id': 'restorePreview', 'before': verified['restorePreview'],
                                        'after': status}])

    def test_unrelated_restore_status_is_not_hidden(self):
        self.assertEqual(len(draft_changes({'restorePreview': 'Восстановление ожидает перезапуска.'},
                                           {'restorePreview': 'Выбор отменён.'})), 1)

    def test_field_values_checked_state_and_removed_fields_are_exact_in_diagnostics(self):
        before = {'fields': [{'id': 'metricKindFrom', 'value': '2024-01-01'},
                             {'id': 'sourceAutoImport', 'checked': False},
                             {'id': 'metricKindTo', 'value': '2024-01-31'}]}
        after = {'fields': [{'id': 'metricKindFrom', 'value': ''},
                            {'id': 'sourceAutoImport', 'checked': True}]}
        changes = draft_changes(before, after)
        self.assertEqual([row['id'] for row in changes], ['metricKindFrom', 'sourceAutoImport', 'metricKindTo'])
        self.assertEqual(changes[0]['before']['value'], '2024-01-01')
        self.assertTrue(changes[1]['after']['checked'])
        self.assertIsNone(changes[2]['after'])


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
            result = self.dialog.open_file_for_preview(
                101, 100, 'Проверить резервную копию SQLite', backup, None)
        self.assertEqual(result['action'], 'open-backup')
        self.assertEqual([call.args[1] for call in self.dialog.message.call_args_list],
                         [0x000C, 0x000D, 0x00F5])

    def test_save_dialog_cannot_be_accepted_by_preview_helper(self):
        self.dialog.message = Mock()
        with self.assertRaisesRegex(AssertionError, 'Only a read-only preview Open'):
            self.dialog.open_file_for_preview(101, 100, 'Сохранить резервную копию SQLite',
                                                Path('unused.sqlite3'), None)
        self.dialog.message.assert_not_called()

    def assert_refused(self):
        with self.assertRaises(AssertionError):
            self.dialog.backup_open_controls(101, self.facts)


class NativeConfirmationCyclesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        (root / 'tauri/test-artifacts').mkdir(parents=True)
        self.record = root / 'tauri/test-artifacts/native-file-dialogs-confirmations.json'
        self.native = NativeConfirmations.__new__(NativeConfirmations)
        self.native.revision = Mock(return_value=42)
        self.native.find_dialog = Mock(return_value=(101, 102))
        self.native.answer = Mock()
        self.native.user32 = SimpleNamespace(IsWindowVisible=Mock(return_value=False),
                                             IsWindowEnabled=Mock(return_value=True))
        self.closer = Mock()
        self.closer.title.return_value = 'Exact native title'
        self.closer.titlebar_info.return_value = {'state': 0, 'closeRect': [560, 30, 600, 60]}
        self.closer.close_titlebar.return_value = {'inputSent': True}
        self.closer.escape.return_value = 103
        for name, value in [('ROOT', root), ('OwnedFileDialogs', Mock(return_value=self.closer)),
                            ('expect', Mock())]:
            active = patch('windows_smoke.' + name, value)
            active.start()
            self.addCleanup(active.stop)
        self.trigger = Mock()

    def test_cancel_only_never_sends_ok_and_reopens_from_focus(self):
        self.native.cancel_only(self.trigger, 'Actual prompt', drafts=lambda: ['unchanged draft'])
        self.assertEqual([call.args[-1] for call in self.native.answer.call_args_list], [2])
        self.closer.close_titlebar.assert_called_once()
        self.closer.escape.assert_called_once_with(101)
        self.assertEqual(self.trigger.press.call_count, 2)
        self.assertEqual([event['action'] for event in json.loads(self.record.read_text())],
                         ['cancel', 'close', 'escape'])

    def test_existing_authorized_accept_still_runs_after_three_cancel_modes(self):
        self.native.cancel_then_accept(self.trigger, 'Actual prompt')
        self.assertEqual([call.args[-1] for call in self.native.answer.call_args_list], [2, 1])
        self.assertEqual([event['action'] for event in json.loads(self.record.read_text())],
                         ['cancel', 'close', 'escape', 'accept'])

    def test_explicitly_disabled_close_is_recorded_without_claiming_click(self):
        self.closer.titlebar_info.return_value = {'state': 1, 'closeRect': [560, 30, 600, 60]}
        self.native.cancel_only(self.trigger, 'Actual prompt')
        self.closer.close_titlebar.assert_not_called()
        self.assertEqual([call.args[-1] for call in self.native.answer.call_args_list], [2, 2])
        close = json.loads(self.record.read_text())[1]
        self.assertEqual(close['outcome'], 'Close unavailable; dismissed using verified Cancel')

    def test_enabled_but_unusable_close_does_not_fall_back_to_cancel(self):
        self.closer.titlebar_info.return_value = {'state': 0, 'closeRect': [0, 0, 0, 0]}
        self.closer.close_titlebar.side_effect = AssertionError('No native Close bounds')
        with self.assertRaisesRegex(AssertionError, 'No native Close bounds'):
            self.native.cancel_only(self.trigger, 'Actual prompt')
        self.assertEqual([call.args[-1] for call in self.native.answer.call_args_list], [2])

    def test_changed_draft_stops_before_reopening(self):
        with self.assertRaisesRegex(AssertionError, 'changed drafts'):
            self.native.cancel_only(self.trigger, 'Actual prompt',
                                    drafts=Mock(side_effect=[['before'], ['changed']]))
        self.assertEqual(self.trigger.click.call_count, 1)
        self.trigger.press.assert_not_called()


if __name__ == '__main__':
    unittest.main()
