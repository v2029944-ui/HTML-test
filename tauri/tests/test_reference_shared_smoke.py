"""Safety contracts for the installed SMB E2E. These tests send no OS input."""
import ctypes
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from windows_shared_smoke import (INSTALLER_AUTOSTART_CLEANUP, InstalledClient, generations,
                                  native_select, require_private_database, smb_workspace,
                                  stop_installer_autostart, wait_for_webview_shutdown, windows_path)
from windows_smoke import ROOT, invoke


class WebViewShutdownTests(unittest.TestCase):
    def test_restart_waits_for_the_old_listener_to_stop_accepting_connections(self):
        probe = Mock()
        probe.connect_ex.side_effect = [0, 0, 10061]
        with patch('windows_shared_smoke.socket.socket') as factory, patch('windows_shared_smoke.time.sleep') as pause:
            factory.return_value.__enter__.return_value = probe
            wait_for_webview_shutdown(9231)
        self.assertEqual(probe.connect_ex.call_count, 3)
        self.assertEqual(pause.call_count, 2)

    def test_an_active_listener_is_never_reused(self):
        probe = Mock()
        probe.connect_ex.return_value = 0
        with patch('windows_shared_smoke.socket.socket') as factory:
            factory.return_value.__enter__.return_value = probe
            with self.assertRaisesRegex(RuntimeError, 'did not close'):
                wait_for_webview_shutdown(9231, timeout=0)


class InstallerCleanupTests(unittest.TestCase):
    def test_python_wrapper_does_not_swallow_a_failed_kill(self):
        failure = subprocess.CalledProcessError(1, 'powershell', stderr='Owned installer process did not stop')
        with patch('windows_shared_smoke.subprocess.run', side_effect=failure) as execute:
            with self.assertRaises(subprocess.CalledProcessError):
                stop_installer_autostart(Path('owned.exe'))
        self.assertTrue(execute.call_args.kwargs['check'])


POWERSHELL = shutil.which('powershell') or shutil.which('pwsh')


@unittest.skipUnless(POWERSHELL, 'A real PowerShell engine is required; Windows CI runs these cases')
class InstallerPowerShellFlowTests(unittest.TestCase):
    """Execute the exact cleanup script with controlled producers, never OS kills."""
    def execute(self, processes, kill_exit=0):
        producer = '''
        function Get-Process {
          [CmdletBinding()] param([Parameter(Position=0)][string]$Name)
          if($Name){Write-Error 'No named process';return}
          %s
        }
        function taskkill {
          [Console]::WriteLine(('TEST_KILL ' + ($args -join ' ')))
          $global:LASTEXITCODE=%d
        }
        ''' % (processes, kill_exit)
        return subprocess.run([POWERSHELL, '-NoProfile', '-NonInteractive', '-Command',
                               producer + INSTALLER_AUTOSTART_CLEANUP],
                              env=dict(os.environ, ANALIZ_SMOKE_BINARY=r'C:\fixture\owned.exe'),
                              capture_output=True, encoding='utf-8', errors='replace', timeout=20)

    def test_no_autostart_process_is_success(self):
        result = self.execute('@()')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('SMB_INSTALLER_AUTOSTART_INSTANCES=0', result.stdout)
        self.assertNotIn('TEST_KILL', result.stdout)

    def test_successful_stop_targets_only_the_matching_binary(self):
        result = self.execute(r'''
          [pscustomobject]@{ProcessName='analiz-rabot-tauri';Path=$env:ANALIZ_SMOKE_BINARY;Id=731}
          [pscustomobject]@{ProcessName='analiz-rabot-tauri';Path='C:\other\foreign.exe';Id=949}
        ''')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('TEST_KILL /PID 731 /T /F', result.stdout)
        self.assertNotIn('949', result.stdout)

    def test_failed_stop_stays_a_failed_process_exit(self):
        result = self.execute("[pscustomobject]@{ProcessName='analiz-rabot-tauri';Path=$env:ANALIZ_SMOKE_BINARY;Id=731}", 5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Owned installer process did not stop', result.stderr)


class InstalledWorkflowOrderTests(unittest.TestCase):
    def test_smb_precedes_personal_and_neither_is_gated_on_the_others_success(self):
        for filename in ('tauri-windows.yml', 'tauri-signed-release.yml'):
            with self.subTest(workflow=filename):
                source = (ROOT / '.github/workflows' / filename).read_text(encoding='utf-8')
                starts = list(re.finditer(r'^      - (?:name|uses):', source, re.M))
                blocks = [source[m.start():starts[i+1].start() if i+1 < len(starts) else len(source)]
                          for i, m in enumerate(starts)]
                dependencies = next(i for i, block in enumerate(blocks) if 'python -m pip install playwright==' in block)
                helpers = next(i for i, block in enumerate(blocks) if "-p 'test_reference_*.py'" in block)
                shared = next(i for i, block in enumerate(blocks) if 'python tauri/tests/windows_shared_smoke.py' in block)
                personal = next(i for i, block in enumerate(blocks) if 'python tauri/tests/windows_smoke.py' in block)
                self.assertLess(dependencies, helpers)
                self.assertLess(helpers, shared)
                self.assertLess(shared, personal)
                self.assertEqual(source.count('python -m pip install playwright=='), 1)
                self.assertEqual(source.count("-p 'test_reference_*.py'"), 1)
                for index in (shared, personal):
                    self.assertIn('!cancelled()', blocks[index])
                    self.assertIn("steps.gui_smoke_dependencies.outcome == 'success'", blocks[index])
                    self.assertNotIn("steps.shared_gui_smoke.outcome == 'success'", blocks[index])
                    self.assertNotIn('steps.gui_helpers.outcome', blocks[index])


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
