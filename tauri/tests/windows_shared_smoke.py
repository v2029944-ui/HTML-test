"""Two installed WebView2 clients, private local databases and a real temporary SMB share.

Only the separately compiled ci-webview2 installer accepts the root/port hooks.
No bridge, dialog, storage command, polling timer or network share is replaced.
"""
import ctypes
import hashlib
import json
import ntpath
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import time
import urllib.request
import uuid
from contextlib import contextmanager, ExitStack, closing

from playwright.sync_api import expect, sync_playwright

from reference_file_dialogs import OwnedFileDialogs
from windows_smoke import (ROOT, NativeConfirmations, api, navigate, ready,
                           record_secondary_failure, wait_for_text,
                           wait_for_webview_page, wide_xlsx, windows_test_workspace)


INSTALLER_AUTOSTART_CLEANUP = r"""
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
# Enumerate successfully even when this executable has not auto-started.
# A missing named process is not a process-enumeration error.
$owned=@(Get-Process -ErrorAction Stop | Where-Object {
  $_.ProcessName -eq 'analiz-rabot-tauri' -and $_.Path -eq $env:ANALIZ_SMOKE_BINARY
})
[Console]::WriteLine(('SMB_INSTALLER_AUTOSTART_INSTANCES={0}' -f $owned.Count))
foreach($process in $owned) {
  taskkill /PID $process.Id /T /F | Out-Null
  if($LASTEXITCODE -ne 0){throw ('Owned installer process did not stop: PID {0}, exit {1}' -f $process.Id,$LASTEXITCODE)}
}
exit 0
"""


def stop_installer_autostart(executable):
    result = subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', INSTALLER_AUTOSTART_CLEANUP],
                            env=dict(os.environ, ANALIZ_SMOKE_BINARY=str(executable)),
                            check=True, capture_output=True, encoding='utf-8', errors='replace', timeout=30)
    if result.stdout.strip():
        print(result.stdout.strip(), flush=True)


def windows_path(value):
    value = str(value)
    if value.startswith('\\\\?\\UNC\\'):
        value = '\\\\' + value[8:]
    elif value.startswith('\\\\?\\'):
        value = value[4:]
    return ntpath.normcase(ntpath.normpath(value))


def require_private_database(value, root):
    """Reject UNC, relative, sibling-client and coincidental prefix matches."""
    path, parent = windows_path(value), windows_path(root)
    drive = ntpath.splitdrive(path)[0]
    assert len(drive) == 2 and drive[1] == ':' and ntpath.isabs(path), ('Non-local data file', path)
    assert ntpath.commonpath([path, parent]) == parent, ('Wrong independent client root', path, parent)
    return path


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(64 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def generations(folder):
    return {path.name: {'sha256': file_hash(path), 'bytes': path.stat().st_size}
            for path in sorted(Path(folder).glob('analiz-generation-*.snapshot'))}


@contextmanager
def smb_workspace(root):
    """Account-only GUID share; remove exactly the share/path created here."""
    name = 'AnalizGui' + uuid.uuid4().hex
    backing = Path(root) / name
    backing.mkdir()
    environment = dict(os.environ, ANALIZ_SMOKE_SHARE_NAME=name, ANALIZ_SMOKE_SHARE_PATH=str(backing))
    create = r"""
    $ErrorActionPreference='Stop'
    $account=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    New-SmbShare -Name $env:ANALIZ_SMOKE_SHARE_NAME -Path $env:ANALIZ_SMOKE_SHARE_PATH -FullAccess $account -ErrorAction Stop | Out-Null
    $share=Get-SmbShare -Name $env:ANALIZ_SMOKE_SHARE_NAME -ErrorAction Stop
    if($share.Path -ine $env:ANALIZ_SMOKE_SHARE_PATH){throw 'Unexpected SMB backing path'}
    exit 0
    """
    remove = r"""
    $ErrorActionPreference='Stop'
    $shares=@(Get-SmbShare -ErrorAction Stop | Where-Object {$_.Name -ceq $env:ANALIZ_SMOKE_SHARE_NAME})
    if($shares.Count -eq 0){exit 0}
    if($shares.Count -ne 1 -or $shares[0].Path -ine $env:ANALIZ_SMOKE_SHARE_PATH){throw 'Refusing to remove a different SMB share'}
    Remove-SmbShare -Name $env:ANALIZ_SMOKE_SHARE_NAME -Force -ErrorAction Stop
    exit 0
    """
    failure = None
    try:
        subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', create],
                       env=environment, check=True, capture_output=True, timeout=30)
        folder = Path('\\\\localhost\\' + name) / 'workspace'
        folder.mkdir()  # Actual UNC I/O verifies the SMB connection and account ACL.
        yield folder
    except BaseException as error:
        failure = error
        raise
    finally:
        action = lambda: subprocess.run(
            ['powershell', '-NoProfile', '-NonInteractive', '-Command', remove],
            env=environment, check=True, capture_output=True, timeout=30)
        if failure:
            record_secondary_failure(failure, 'Temporary SMB share cleanup failed', action)
        else:
            action()


class InstalledClient:
    def __init__(self, playwright, executable, root, name, port, output):
        self.playwright, self.executable = playwright, Path(executable)
        self.root, self.name, self.port, self.output = Path(root), name, port, Path(output)
        self.data = self.root / 'data'
        self.env = dict(os.environ, ANALIZ_SMOKE_DATA_ROOT=str(self.data),
                        ANALIZ_SMOKE_CDP_PORT=str(port),
                        WEBVIEW2_USER_DATA_FOLDER=str(self.root / 'webview'))
        self.env.pop('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', None)
        self.env['NO_PROXY'] = self.env.get('NO_PROXY', '') + ',127.0.0.1,localhost'
        self.process = self.browser = self.page = None
        self.verified = False
        self.errors = []

    def __enter__(self):
        self.start(0, shared=False)
        return self

    def __exit__(self, kind, error, traceback):
        if error:
            record_secondary_failure(error, self.name + ' screenshot failed', self.capture_failure)
            record_secondary_failure(error, self.name + ' process cleanup failed', self.force_stop)
        else:
            try:
                self.stop()
            except BaseException as failure:
                record_secondary_failure(failure, self.name + ' forced cleanup failed', self.force_stop)
                raise

    def start(self, reports, *, shared):
        self.root.mkdir(parents=True, exist_ok=True)
        with socket.socket() as probe:
            if probe.connect_ex(('127.0.0.1', self.port)) == 0:
                raise RuntimeError('Independent CDP port already in use: ' + str(self.port))
        self.process = subprocess.Popen([str(self.executable)], env=self.env)
        self.verified = False
        try:
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            deadline = time.monotonic() + 90
            while True:
                if self.process.poll() is not None:
                    raise RuntimeError(self.name + ' exited during startup')
                try:
                    with opener.open('http://127.0.0.1:' + str(self.port) + '/json/version', timeout=2) as reply:
                        self.engine = json.load(reply)
                    break
                except (OSError, ValueError):
                    if time.monotonic() >= deadline:
                        raise RuntimeError(self.name + ' independent CDP endpoint did not start')
                    time.sleep(.1)
            self.browser = self.playwright.chromium.connect_over_cdp('http://127.0.0.1:' + str(self.port))
            self.page = wait_for_webview_page(self.browser)
            self.errors = []
            self.page.on('pageerror', lambda error: self.errors.append(str(error)))
            ready(self.page, reports, shared=shared)
            info = api(self.page, 'info')
            require_private_database(info['dataFile'], self.data)
            self.verified = True
            self.personal_file = self.data / 'reports.sqlite3'
            self.confirmations = NativeConfirmations(self.process.pid, Path(info['dataFile']))
            self.dialogs = OwnedFileDialogs(self.confirmations)
        except BaseException as error:
            record_secondary_failure(error, self.name + ' startup cleanup failed', self.force_stop)
            raise

    def capture_failure(self):
        if self.page and not self.page.is_closed():
            self.page.screenshot(path=str(self.output / ('shared-smb-' + self.name + '-failure.png')), timeout=10_000)

    def stop(self):
        if self.process is None:
            return
        assert not self.errors, (self.name, self.errors)
        script = r"""
        $ErrorActionPreference='Stop'
        $process=Get-Process -Id $env:ANALIZ_SMOKE_PID -ErrorAction Stop
        if($process.Path -ine $env:ANALIZ_SMOKE_BINARY){throw 'Independent client binary mismatch'}
        if(-not $process.CloseMainWindow()){throw 'Independent client refused normal close'}
        exit 0
        """
        subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', script],
                       env=dict(self.env, ANALIZ_SMOKE_PID=str(self.process.pid),
                                ANALIZ_SMOKE_BINARY=str(self.executable)),
                       check=True, capture_output=True, timeout=15)
        assert self.process.wait(timeout=30) == 0, self.name + ' did not exit normally'
        self._disconnect()

    def force_stop(self):
        if self.process is not None and self.process.poll() is None:
            result = subprocess.run(['taskkill', '/PID', str(self.process.pid), '/T', '/F'],
                                    capture_output=True, timeout=15)
            if result.returncode and self.process.poll() is None:
                result.check_returncode()
            self.process.wait(timeout=15)
        self._disconnect()

    def _disconnect(self):
        # Never send Browser.close through an endpoint whose private root was not verified.
        if self.verified and self.browser and self.browser.is_connected():
            self.browser.close()
        self.browser = self.page = self.process = None

    def restart(self, reports):
        self.stop()
        self.start(reports, shared=True)

    def state(self):
        return self.page.evaluate('''() => {
          const value=window.sharedWorkspace.state;
          return {mode:value.mode,folder:value.folder,generation:value.generation,
            available:value.available,pending:value.pending,error:value.error,
            reports:window.unifiedUI.snapshot?.reports,revision:window.unifiedUI.revision};
        }''')


def native_select(client, path, kind, step):
    """Known test paths only; exact HWND/PID/title and factual Win32 control IDs."""
    title = {'folder': 'Общая папка команды', 'xlsx': 'Выберите XLSX для нативного импорта'}[kind]
    path = Path(path)
    assert path.is_dir() if kind == 'folder' else path.is_file() and path.suffix == '.xlsx'
    hwnd, owner = client.dialogs.find(title)
    facts, _ = client.dialogs.inspect(hwnd, title, client.output / ('shared-smb-' + step + '.png'))
    client.dialogs.verify_window(hwnd, owner, title)
    native = client.confirmations
    if kind == 'folder':
        edit = native.user32.GetDlgItem(hwnd, 1152)
        opened = native.user32.GetDlgItem(hwnd, 1)
        for control, expected_id, expected_class in [(edit, 1152, 'edit'), (opened, 1, 'button')]:
            assert control and native.process(control) == native.pid, facts
            assert native.user32.IsChild(hwnd, control), facts
            assert native.user32.GetDlgCtrlID(control) == expected_id, facts
            assert native.window_class(control).lower() == expected_class, facts
            assert native.user32.IsWindowVisible(control) and native.user32.IsWindowEnabled(control), facts
    else:
        edit, opened = client.dialogs.backup_open_controls(hwnd, facts)
    text = ctypes.create_unicode_buffer(str(path))
    assert client.dialogs.message(edit, 0x000C, 0, ctypes.addressof(text)), 'Native path edit rejected WM_SETTEXT'
    readback = ctypes.create_unicode_buffer(len(str(path)) + 2)
    client.dialogs.message(edit, 0x000D, len(readback), ctypes.addressof(readback))
    assert readback.value == str(path), 'Native path did not round trip exactly'
    client.dialogs.verify_window(hwnd, owner, title)
    client.dialogs.message(opened, 0x00F5)  # Actual verified IDOK control, never a window-close command.
    deadline = time.monotonic() + 20
    while native.user32.IsWindowVisible(hwnd) or not native.user32.IsWindowEnabled(owner):
        if time.monotonic() >= deadline:
            raise AssertionError('Native selector did not return: ' + title)
        time.sleep(.05)


def connect_shared(client, folder, *, cancel_first=False):
    page = client.page
    navigate(page, 'storage')
    trigger = page.locator('#sharedWorkspaceConnect')
    if cancel_first:
        before = client.state()
        trigger.click()
        hwnd, owner = client.dialogs.find('Общая папка команды')
        client.dialogs.dismiss(hwnd, owner, 'Общая папка команды', 'cancel',
                               client.output / 'shared-smb-connect-cancel.png')
        expect(trigger).to_be_enabled(timeout=30_000)
        expect(trigger).to_be_focused(timeout=30_000)
        assert client.state() == before, 'Cancelling shared selection changed personal data/mode'
    trigger.click()
    native_select(client, folder, 'folder', 'connect-' + client.name)
    page.wait_for_function('''folder => window.sharedWorkspace.state?.mode==='shared'
      && window.sharedWorkspace.state.available && !window.sharedWorkspace.busy
      && window.unifiedUI.snapshot?.reports===0''', arg=str(folder), timeout=90_000)
    state = client.state()
    assert windows_path(state['folder']) == windows_path(folder), state
    expect(page.locator('#sharedWorkspaceStatus')).to_contain_text('Общая база подключена')
    info = api(page, 'info')
    require_private_database(info['dataFile'], client.data)
    page.screenshot(path=str(client.output / ('shared-smb-' + client.name + '-connected.png')))


def preview_xlsx(client, workbook):
    page = client.page
    page.locator('#refImport').click()
    page.locator('#excelFile').click()
    native_select(client, workbook, 'xlsx', 'xlsx-' + client.name)
    page.locator('#importFormat').select_option('daily-wide')
    page.locator('#dailyYear').fill('2024')
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 120_000)
    assert page.locator('#importPreview .kpi').first.locator('strong').inner_text() == '366'
    for field in ('unifiedAcceptQuality', 'unifiedAcceptOverlap'):
        acknowledgement = page.locator('#' + field)
        if acknowledgement.is_visible():
            acknowledgement.check()


def summary(client):
    snapshot = api(client.page, 'nativeSettingsBootstrap')
    result = api(client.page, 'managementReportSummary', snapshot['revision'], {'report_type': 'day'})
    assert (result['reports'], result['rows'], result['amount']) == (366, 1464, 7320), result
    assert client.page.evaluate('window.unifiedOriginal.state.reports.length') == 0
    return result


def run():
    if sys.platform != 'win32' or os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Run only on an isolated Windows GitHub Actions runner')
    output = ROOT / 'tauri/test-artifacts'
    output.mkdir(exist_ok=True)
    installers = list((ROOT / 'tauri/src-tauri/target/release/bundle/nsis').glob('*.exe'))
    assert len(installers) == 1, 'Expected one separately compiled GUI installer'
    preserved = output / 'setup.exe'
    assert preserved.is_file() and file_hash(preserved) != file_hash(installers[0]), 'Do not run the distributable build'
    proof = {'status': 'running', 'guiSha256': file_hash(installers[0]), 'steps': []}
    record = output / 'shared-smb-proof.json'

    def note(step, **values):
        proof['steps'].append({'step': step, **values})
        record.write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding='utf-8')

    try:
        with windows_test_workspace(os.environ.get('RUNNER_TEMP')) as root:
            root = Path(root)
            destination = root / 'installed'
            installer_env = dict(os.environ, ANALIZ_SMOKE_DATA_ROOT=str(root / 'installer-data'),
                                 ANALIZ_SMOKE_CDP_PORT='9233',
                                 WEBVIEW2_USER_DATA_FOLDER=str(root / 'installer-webview'))
            subprocess.run([str(installers[0]), '/S', '/D=' + str(destination)],
                           env=installer_env, check=True, timeout=240)
            executables = list(destination.rglob('analiz-rabot-tauri.exe'))
            assert len(executables) == 1
            # NSIS can autostart only this freshly installed test binary.
            stop_installer_autostart(executables[0])
            with smb_workspace(root) as folder, sync_playwright() as playwright, ExitStack() as clients:
                workbook = folder / 'synthetic-shared-year.xlsx'
                workbook.write_bytes(wide_xlsx(2024))
                source_hash = file_hash(workbook)
                a = clients.enter_context(InstalledClient(playwright, executables[0], root / 'client-a', 'a', 9231, output))
                b = clients.enter_context(InstalledClient(playwright, executables[0], root / 'client-b', 'b', 9232, output))
                assert a.process.pid != b.process.pid and a.data != b.data
                connect_shared(a, folder, cancel_first=True)
                connect_shared(b, folder)
                baseline = generations(folder)
                generation = a.state()['generation']
                assert b.state()['generation'] == generation
                note('two-independent-clients-connected', writer=a.state(), reader=b.state(),
                     writerPid=a.process.pid, readerPid=b.process.pid, folder=str(folder))
                preview_xlsx(a, workbook)
                save = a.page.locator('#saveImport')
                prompt = 'Повторно проверить Excel и атомарно сохранить 366 отчётов'
                save.click()
                dialog, owner = a.confirmations.find_dialog(prompt)
                a.confirmations.answer(dialog, owner, prompt, 2)
                wait_for_text(a.page, '#unifiedImportStatus', 'Сохранение отменено', 30_000)
                expect(save).to_be_focused(timeout=30_000)
                assert a.state()['reports'] == b.state()['reports'] == 0
                assert generations(folder) == baseline, 'Cancelled import published shared data'
                save.click()
                dialog, owner = a.confirmations.find_dialog(prompt)
                a.confirmations.answer(dialog, owner, prompt, 1)
                wait_for_text(a.page, '#unifiedImportStatus', 'Сохранено: 366 отчётов', 120_000)
                started = time.monotonic()
                # Observe only. The test never invokes sync/refresh or clicks Retry on the reader.
                b.page.wait_for_function('''generation => window.unifiedUI.snapshot?.reports===366
                  && window.sharedWorkspace.state.generation>generation
                  && document.querySelector('#unifiedIndexStatus').textContent.includes('Общая база SQLite. Отчётов: 366')''',
                                         arg=generation, timeout=120_000)
                a.page.wait_for_function('generation => window.sharedWorkspace.state.generation>generation',
                                         arg=generation, timeout=30_000)
                committed = generations(folder)
                assert a.state()['generation'] == generation + 1 == b.state()['generation']
                assert len(committed) == len(baseline) + 1
                assert all(committed[name] == value for name, value in baseline.items()), 'An old generation changed'
                assert summary(a) == summary(b)
                assert file_hash(workbook) == source_hash
                a.page.screenshot(path=str(output / 'shared-smb-writer-imported.png'))
                b.page.screenshot(path=str(output / 'shared-smb-reader-automatic.png'))
                note('reader-updated-without-test-refresh', seconds=time.monotonic() - started,
                     writer=a.state(), reader=b.state(), generations=committed, totals=summary(b))
                preview_xlsx(b, workbook)
                replacement = b.page.locator('#importPreview .kpi').filter(has_text='Замен')
                expect(replacement.locator('strong')).to_have_text('366')
                expect(b.page.locator('#replaceExisting')).not_to_be_checked()
                b.page.locator('#saveImport').click()
                wait_for_text(b.page, '#unifiedImportStatus', 'Подтвердите найденные замены', 30_000)
                assert generations(folder) == committed
                assert summary(a) == summary(b)
                note('repeat-source-is-replacement-not-duplicate', reader=b.state(), totals=summary(b))
                a.restart(366)
                b.restart(366)
                for client in (a, b):
                    state = client.state()
                    assert state['mode'] == 'shared' and state['available'] and not state['pending'], state
                    assert windows_path(state['folder']) == windows_path(folder), state
                    assert state['generation'] == generation + 1, state
                    summary(client)
                    require_private_database(api(client.page, 'info')['dataFile'], client.data)
                    with closing(sqlite3.connect(client.personal_file.as_uri() + '?mode=ro', uri=True)) as personal:
                        assert personal.execute('SELECT COUNT(*) FROM reports').fetchone() == (0,), 'Shared writes leaked into personal DB'
                    client.page.screenshot(path=str(output / ('shared-smb-' + client.name + '-reopened.png')))
                assert generations(folder) == committed, 'Reopen modified an immutable shared generation'
                note('both-clients-reopened-in-persisted-shared-workspace', writer=a.state(), reader=b.state())
                proof['status'] = 'passed'
                record.write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding='utf-8')
    except BaseException as error:
        proof['status'] = 'failed'
        proof['error'] = type(error).__name__ + ': ' + str(error)
        record.write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding='utf-8')
        raise


if __name__ == '__main__':
    try:
        run()
    except BaseException as error:
        output = ROOT / 'tauri/test-artifacts'
        output.mkdir(exist_ok=True)
        detail = type(error).__name__ + ': ' + str(error)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            stderr = error.stderr.decode('utf-8', errors='replace') if isinstance(error.stderr, bytes) else error.stderr
            detail += '\n' + stderr[-4000:]
        (output / 'shared-smb-error.txt').write_text(detail, encoding='utf-8')
        raise
