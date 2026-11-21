"""Safety contracts for the installed SMB E2E. These tests send no OS input."""
import ctypes
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from windows_shared_smoke import InstalledClient, generations, native_select, require_private_database, smb_workspace, windows_path
from windows_smoke import invoke


class ClientIsolationTests(unittest.TestCase):
    def test_distinct_client_roots_profiles_and_ports_do_not_change_parent_environment(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            parent_environment = dict(os.environ)
            first = InstalledClient(None, root / 'app.exe', root / 'a', 'a', 9231, root)
            second = InstalledClient(None, root / 'app.exe', root / 'b', 'b', 9232, root)
            for key in ('ANALIZ_SMOKE_DATA_ROOT', 'ANALIZ_SMOKE_CDP_PORT', 'WEBVIEW2_USER_DATA_FOLDER'):
                self.assertNotEqual(first.env[key], second.env[key])
            self.assertFalse(first.data.exists())
            self.assertFalse(second.data.exists())
            self.assertEqual(dict(os.environ), parent_environment)

    def test_database_must_be_on_its_own_local_root_not_unc_or_a_prefix_collision(self):
        root = r'C:\fixture\client-a\data'
        expected = r'c:\fixture\client-a\data\shared-runtime\local\reports.sqlite3'
        self.assertEqual(require_private_database(r'\\?\C:\fixture\client-a\data\shared-runtime\local\reports.sqlite3', root), expected)
        for wrong in (r'\\localhost\test\reports.sqlite3', r'reports.sqlite3',
                      r'C:\fixture\client-b\data\reports.sqlite3',
                      r'C:\fixture\client-a\data-other\reports.sqlite3'):
            with self.subTest(wrong=wrong), self.assertRaises(AssertionError):
                require_private_database(wrong, root)

    def test_verbatim_unc_comparison_preserves_the_same_share_identity(self):
        self.assertEqual(windows_path(r'\\?\UNC\localhost\Share\workspace'),
                         windows_path(r'\\localhost\share\workspace'))

    def test_manifest_ignores_excel_and_incomplete_uploads_but_detects_changed_generations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'source.xlsx').write_bytes(b'fixture')
            (root / '.incomplete-upload').write_bytes(b'incomplete')
            snapshot = root / 'analiz-generation-00000000000000000001.snapshot'
            snapshot.write_bytes(b'original immutable generation')
            before = generations(root)
            self.assertEqual(list(before), [snapshot.name])
            snapshot.write_bytes(b'changed')
            self.assertNotEqual(generations(root), before)

    def test_unverified_cdp_endpoint_is_never_closed(self):
        client = InstalledClient(None, Path('unused.exe'), Path('unused-root'), 'fixture', 9231, Path('.'))
        browser = Mock()
        client.browser = browser
        client._disconnect()
        browser.close.assert_not_called()

    def test_partial_share_provisioning_failure_still_attempts_scoped_cleanup(self):
        failure = subprocess.CalledProcessError(1, 'create-share')
        with tempfile.TemporaryDirectory() as temporary, patch(
                'windows_shared_smoke.subprocess.run', side_effect=[failure, Mock(returncode=0)]) as execute:
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                with smb_workspace(Path(temporary)):
                    self.fail('A failed provision must not start clients')
            self.assertIs(caught.exception, failure)
            self.assertEqual(execute.call_count, 2)
            create, cleanup = execute.call_args_list
            self.assertEqual(create.kwargs['env']['ANALIZ_SMOKE_SHARE_NAME'],
                             cleanup.kwargs['env']['ANALIZ_SMOKE_SHARE_NAME'])
            self.assertIn('Remove-SmbShare', cleanup.args[0][-1])


class FixtureInvokeTests(unittest.TestCase):
    def test_protected_raw_fixture_command_captures_identity_without_mutating_arguments(self):
        page = SimpleNamespace(evaluate=Mock(side_effect=[{'workspace_token': 'current'}, {'ok': True}]))
        arguments = {'folder': r'C:\fixture'}
        self.assertEqual(invoke(page, 'set_folder', arguments), {'ok': True})
        self.assertEqual(arguments, {'folder': r'C:\fixture'})
        self.assertEqual(page.evaluate.call_args.args[1],
                         {'command': 'set_folder', 'args': {'folder': r'C:\fixture', 'workspaceToken': 'current'}})

    def test_explicit_stale_token_cannot_be_replaced_with_current_identity(self):
        page = SimpleNamespace(evaluate=Mock(return_value='rejected'))
        invoke(page, 'set_folder', {'folder': r'C:\fixture', 'workspaceToken': 'old'})
        self.assertEqual(page.evaluate.call_count, 1)
        self.assertEqual(page.evaluate.call_args.args[1]['args']['workspaceToken'], 'old')

    def test_workspace_control_commands_remain_outside_the_protected_store_wrapper(self):
        page = SimpleNamespace(evaluate=Mock(return_value={}))
        invoke(page, 'shared_workspace_status')
        self.assertEqual(page.evaluate.call_count, 1)
        self.assertEqual(page.evaluate.call_args.args[1], {'command': 'shared_workspace_status', 'args': {}})


class FolderControlTests(unittest.TestCase):
    def setUp(self):
        self.make_client()

    def make_client(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.folder = Path(self.temporary.name)
        self.closed = False
        native = SimpleNamespace(pid=41, process=Mock(return_value=41),
                                 window_class=Mock(side_effect=lambda hwnd: 'Edit' if hwnd == 115 else 'Button'))
        native.user32 = SimpleNamespace(
            GetDlgItem=Mock(side_effect=lambda hwnd, number: 115 if number == 1152 else 101),
            IsChild=Mock(return_value=True), GetDlgCtrlID=Mock(side_effect=lambda hwnd: 1152 if hwnd == 115 else 1),
            IsWindowVisible=Mock(side_effect=lambda hwnd: not (hwnd == 100 and self.closed)),
            IsWindowEnabled=Mock(return_value=True))

        def message(hwnd, code, wparam=0, lparam=0):
            if code == 0x000C:
                self.assertEqual(hwnd, 115)
                self.assertEqual(ctypes.wstring_at(lparam), str(self.folder))
                return 1
            if code == 0x000D:
                text = ctypes.create_unicode_buffer(str(self.folder))
                ctypes.memmove(lparam, text, ctypes.sizeof(text))
                return len(str(self.folder))
            self.assertEqual((hwnd, code), (101, 0x00F5))
            self.closed = True
            return 0

        dialogs = SimpleNamespace(find=Mock(return_value=(100, 99)),
                                  inspect=Mock(return_value=({'pid': 41}, 102)),
                                  verify_window=Mock(), message=Mock(side_effect=message))
        self.client = SimpleNamespace(dialogs=dialogs, confirmations=native, output=self.folder)

    def test_folder_edit_uses_observed_id1152_and_actual_verified_idok(self):
        native_select(self.client, self.folder, 'folder', 'fixture')
        self.client.dialogs.find.assert_called_once_with('Общая папка команды')
        self.assertEqual([call.args[1] for call in self.client.dialogs.message.call_args_list],
                         [0x000C, 0x000D, 0x00F5])

    def test_foreign_process_or_wrong_control_class_never_receives_input(self):
        for condition in ('foreign', 'class', 'disabled', 'not-child', 'wrong-id'):
            with self.subTest(condition=condition):
                self.make_client()
                native = self.client.confirmations
                if condition == 'foreign':
                    native.process.return_value = 99
                elif condition == 'class':
                    native.window_class.side_effect = lambda hwnd: 'Pane'
                elif condition == 'disabled':
                    native.user32.IsWindowEnabled.return_value = False
                elif condition == 'not-child':
                    native.user32.IsChild.return_value = False
                else:
                    native.user32.GetDlgCtrlID.side_effect = lambda hwnd: 2
                with self.assertRaises(AssertionError):
                    native_select(self.client, self.folder, 'folder', 'fixture')
                self.client.dialogs.message.assert_not_called()

    def test_wrong_native_readback_is_refused_before_idok(self):
        original = self.client.dialogs.message.side_effect
        self.client.dialogs.message.side_effect = lambda hwnd, code, wparam=0, lparam=0: (
            0 if code == 0x000D else original(hwnd, code, wparam, lparam))
        with self.assertRaisesRegex(AssertionError, 'did not round trip'):
            native_select(self.client, self.folder, 'folder', 'fixture')
        self.assertEqual([call.args[1] for call in self.client.dialogs.message.call_args_list], [0x000C, 0x000D])


if __name__ == '__main__':
    unittest.main()
