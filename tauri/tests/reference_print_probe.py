"""Optional last-process Windows print inventory; never reports unsupported as passed.

Call probe_reference_print(page, output, app_pid) last in a disposable installed_page.
The caller selects a usable Summary first. No print function replacement, printer
selection, OK, global keyboard input, database writes, or application exports.
Only a newly discovered, strictly attributed print target may receive Cancel/Esc.
"""
import json
import os
import re
from pathlib import Path
import subprocess
import time
import urllib.request

from playwright.sync_api import Error as PlaywrightError


_NATIVE = r"""
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PrintProbeNative {
 public delegate bool Callback(IntPtr hwnd, IntPtr arg);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback callback,IntPtr arg);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd,uint cmd);
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hwnd);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd,StringBuilder text,int count);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr hwnd);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr hwnd,uint message,UIntPtr wp,IntPtr lp);
}
'@
$appId=[int]$env:ANALIZ_PRINT_APP_PID
$all=@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CreationDate)
$root=@($all | Where-Object {$_.ProcessId -eq $appId})
if ($root.Count -ne 1 -or $root[0].Name -ne 'analiz-rabot-tauri.exe') {throw 'Isolated Tauri process identity not found'}
$ids=[System.Collections.Generic.HashSet[int]]::new()
[void]$ids.Add($appId)
do {
 $changed=$false
 foreach ($item in $all) {
  if ($ids.Contains([int]$item.ParentProcessId) -and
      $item.CreationDate -ge $root[0].CreationDate -and
      ($item.Name -eq 'msedgewebview2.exe' -or $item.ProcessId -eq $appId)) {
   if ($ids.Add([int]$item.ProcessId)) {$changed=$true}
  }
 }
} while ($changed)
function ProcessOf([IntPtr]$handle) {
 [uint32]$ownerId=0
 [void][PrintProbeNative]::GetWindowThreadProcessId($handle,[ref]$ownerId)
 return [int]$ownerId
}
function ClassOf([IntPtr]$handle) {
 $text=[System.Text.StringBuilder]::new(256)
 [void][PrintProbeNative]::GetClassName($handle,$text,256)
 return $text.ToString()
}
function TitleOf([IntPtr]$handle) {
 $text=[System.Text.StringBuilder]::new(512)
 [void][PrintProbeNative]::GetWindowText($handle,$text,512)
 return $text.ToString()
}
function OwnerChain([IntPtr]$handle) {
 $chain=@()
 for ($n=0;$n -lt 16;$n++) {
  $handle=[PrintProbeNative]::GetWindow($handle,4)
  if ($handle -eq [IntPtr]::Zero) {break}
  $chain += $handle.ToInt64()
 }
 return ,$chain
}
$action=$env:ANALIZ_PRINT_ACTION
if ($action) {
 $handle=[IntPtr][long]$env:ANALIZ_PRINT_HWND
 $main=[long]$env:ANALIZ_PRINT_MAIN_HWND
 $chain=OwnerChain $handle
 if (-not [PrintProbeNative]::IsWindow($handle) -or
     -not $ids.Contains((ProcessOf $handle)) -or
     -not [PrintProbeNative]::IsWindowVisible($handle) -or
     -not [PrintProbeNative]::IsWindowEnabled($handle) -or
     (ClassOf $handle) -ne '#32770' -or
     (TitleOf $handle) -notin @('Print','Печать','Print Preview','Предварительный просмотр') -or
     $chain -notcontains $main -or (ProcessOf ([IntPtr]$main)) -ne $appId) {
  throw 'Print dialog identity changed; refusing action'
 }
 $element=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
 $buttons=$element.FindAll([System.Windows.Automation.TreeScope]::Descendants,
   [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Button))
 $cancel=@()
 foreach($button in $buttons) {
  $info=$button.Current
  if ($info.Name -in @('Cancel','Отмена') -and $info.IsEnabled -and -not $info.IsOffscreen -and
      $ids.Contains($info.ProcessId) -and $info.NativeWindowHandle -ne 0 -and
      [PrintProbeNative]::GetDlgCtrlID([IntPtr]$info.NativeWindowHandle) -eq 2) {$cancel += $button}
 }
 if ($cancel.Count -ne 1) {throw 'Unique visible native IDCANCEL=2 not confirmed'}
 if ($action -eq 'cancel') {
  $pattern=$cancel[0].GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
 } elseif ($action -eq 'escape') {
  # Queue Escape for this verified HWND only; no SendKeys/SendInput/focus changes.
  if (-not [PrintProbeNative]::PostMessage($handle,0x100,[UIntPtr]27,[IntPtr]1)) {throw 'Targeted Escape keydown failed'}
  if (-not [PrintProbeNative]::PostMessage($handle,0x101,[UIntPtr]27,[IntPtr]0xC0000001)) {throw 'Targeted Escape keyup failed'}
 } else {throw 'Unsupported print probe action'}
 [PSCustomObject]@{action=$action;hwnd=$handle.ToInt64();dispatched=$true} | ConvertTo-Json -Compress
 exit
}
$screenError=$null
try {
 Add-Type -AssemblyName System.Windows.Forms,System.Drawing
 $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
 $bitmap=[System.Drawing.Bitmap]::new($bounds.Width,$bounds.Height)
 $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
 try {
  $graphics.CopyFromScreen($bounds.Location,[System.Drawing.Point]::Empty,$bounds.Size)
  $bitmap.Save($env:ANALIZ_PRINT_SCREENSHOT)
 } finally {$graphics.Dispose();$bitmap.Dispose()}
} catch {$screenError=$_.Exception.Message}
$handles=[System.Collections.Generic.List[IntPtr]]::new()
$callback=[PrintProbeNative+Callback]{param($handle,$arg)
 if ($ids.Contains((ProcessOf $handle))) {$handles.Add($handle)}
 return $true
}
[void][PrintProbeNative]::EnumWindows($callback,[IntPtr]::Zero)
$windows=@()
foreach ($handle in $handles) {
 $controls=@();$uiaError=$null;$truncated=$false
 try {
  $element=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
  $nodes=$element.FindAll([System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
  $truncated=$nodes.Count -gt 400
  for($n=0;$n -lt [Math]::Min($nodes.Count,400);$n++) {
   $info=$nodes[$n].Current
   if (-not $ids.Contains($info.ProcessId)) {continue}
   $controlHandle=[IntPtr]$info.NativeWindowHandle
   $controlId=$null
   if ($controlHandle -ne [IntPtr]::Zero) {$controlId=[PrintProbeNative]::GetDlgCtrlID($controlHandle)}
   $controls += [PSCustomObject]@{type=$info.ControlType.ProgrammaticName;name=$info.Name;
    automation_id=$info.AutomationId;pid=$info.ProcessId;hwnd=$info.NativeWindowHandle;
    control_id=$controlId;enabled=$info.IsEnabled;offscreen=$info.IsOffscreen}
  }
 } catch {$uiaError=$_.Exception.Message}
 $windows += [PSCustomObject]@{hwnd=$handle.ToInt64();pid=(ProcessOf $handle);
  class=(ClassOf $handle);title=(TitleOf $handle);
  visible=[PrintProbeNative]::IsWindowVisible($handle);enabled=[PrintProbeNative]::IsWindowEnabled($handle);
  owner=[PrintProbeNative]::GetWindow($handle,4).ToInt64();owners=(OwnerChain $handle);
  parent=[PrintProbeNative]::GetParent($handle).ToInt64();
  controls=$controls;uia_error=$uiaError;uia_truncated=$truncated}
}
$processes=@($all | Where-Object {$ids.Contains([int]$_.ProcessId)} | ForEach-Object {
 [PSCustomObject]@{pid=$_.ProcessId;parent=$_.ParentProcessId;name=$_.Name;created=$_.CreationDate.ToString('o')}
})
[PSCustomObject]@{processes=$processes;windows=$windows;screenshot_error=$screenError} | ConvertTo-Json -Depth 7 -Compress
"""


def _native(app_pid, screenshot, action=None, hwnd=None, main_hwnd=None):
    env = dict(os.environ, ANALIZ_PRINT_APP_PID=str(app_pid),
               ANALIZ_PRINT_SCREENSHOT=str(screenshot))
    for key in ['ANALIZ_PRINT_ACTION', 'ANALIZ_PRINT_HWND', 'ANALIZ_PRINT_MAIN_HWND']:
        env.pop(key, None)
    if action:
        env.update(ANALIZ_PRINT_ACTION=action, ANALIZ_PRINT_HWND=str(hwnd),
                   ANALIZ_PRINT_MAIN_HWND=str(main_hwnd))
    result = subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', _NATIVE],
                            env=env, capture_output=True, encoding='utf-8', errors='replace',
                            timeout=30)
    if result.returncode:
        raise RuntimeError('Native print inventory/action failed: ' + result.stderr.strip())
    return json.loads(result.stdout)


def _targets(endpoint):
    direct = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with direct.open(endpoint.rstrip('/') + '/json/list', timeout=3) as response:
        return json.load(response)


def probe_reference_print(page, output, app_pid, endpoint='http://127.0.0.1:9227'):
    """Return evidence; unsupported/error are diagnostic statuses, never Cancel passes."""
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    report = {'app_pid': app_pid, 'status': 'started', 'attempts': [], 'snapshots': [],
              'restrictions': 'No mocks, printer selection, print-to-file, OK, or global input.'}
    path = output / 'reference-print-probe.json'

    def save():
        path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')

    def capture(label):
        snapshot = {'label': label}
        for key, read in [('targets', lambda: _targets(endpoint)),
                          ('native', lambda: _native(app_pid, output / ('reference-print-' + label + '.png')))]:
            try:
                snapshot[key] = read()
            except Exception as error:
                snapshot[key + '_error'] = type(error).__name__ + ': ' + str(error)
        report['snapshots'].append(snapshot)
        save()
        return snapshot

    def source_click(label):
        attempt = {'step': label}
        report['attempts'].append(attempt)
        save()
        try:
            # A real visible production element; no evaluate(click) or print replacement.
            page.locator('#s1').get_by_role('button', name='Печать / PDF', exact=True).click(
                timeout=5000, no_wait_after=True)
            attempt['click'] = 'completed'
        except PlaywrightError as error:
            # Modal print UI may block completion; still inspect the external native UI.
            attempt['click'] = type(error).__name__ + ': ' + str(error)
        save()

    def target_for(snapshot, before, source_id):
        old_ids = {target['id'] for target in before.get('targets', [])}
        html = [target for target in snapshot.get('targets', [])
                if target.get('id') not in old_ids and target.get('openerId') == source_id
                and target.get('url', '').split('?', 1)[0] in
                ('edge://print/', 'chrome://print/', 'edge-untrusted://print/', 'chrome-untrusted://print/')]
        roots = [window for window in before.get('native', {}).get('windows', [])
                 if window['pid'] == app_pid and window['visible'] and not window['owner']]
        old_windows = {window['hwnd'] for window in before.get('native', {}).get('windows', [])}
        native = []
        if len(roots) == 1:
            main = roots[0]['hwnd']
            for window in snapshot.get('native', {}).get('windows', []):
                cancel = [node for node in window['controls']
                          if node['type'] == 'ControlType.Button' and node['name'] in ('Cancel', 'Отмена')
                          and node['control_id'] == 2 and node['enabled'] and not node['offscreen']]
                if (window['hwnd'] not in old_windows and window['class'] == '#32770'
                        and window['title'] in ('Print', 'Печать', 'Print Preview', 'Предварительный просмотр')
                        and main in window['owners'] and window['visible'] and window['enabled']
                        and not window['uia_truncated'] and len(cancel) == 1):
                    native.append({'kind': 'native', 'hwnd': window['hwnd'], 'main': main})
        candidates = [{'kind': 'html', 'id': target['id'], 'url': target['url']} for target in html] + native
        if len(candidates) != 1:
            report['unavailable_reason'] = 'No unique new print target with proven source opener/owner and Cancel control'
            report['candidate_count'] = len(candidates)
            return None
        return candidates[0]

    def present(target, snapshot):
        if target['kind'] == 'html':
            return any(item['id'] == target['id'] for item in snapshot.get('targets', []))
        return any(item['hwnd'] == target['hwnd'] and item['visible']
                   for item in snapshot.get('native', {}).get('windows', []))

    def html_page(target):
        matches = []
        for context in page.context.browser.contexts:
            for candidate in context.pages:
                if candidate == page or candidate.url != target['url']:
                    continue
                session = context.new_cdp_session(candidate)
                try:
                    actual = session.send('Target.getTargetInfo')['targetInfo']
                    if actual['targetId'] == target['id']:
                        matches.append(candidate)
                finally:
                    session.detach()
        if len(matches) != 1:
            raise RuntimeError('Attributed print target is not a unique accessible Playwright page')
        return matches[0]

    def close_target(target, action):
        if target['kind'] == 'native':
            return _native(app_pid, output / 'unused.png', action, target['hwnd'], target['main'])
        candidate = html_page(target)
        cancel = candidate.get_by_role('button', name=re.compile(r'^(Cancel|Отмена)$'))
        if cancel.count() != 1 or not cancel.is_visible() or not cancel.is_enabled():
            raise RuntimeError('Attributed HTML preview has no unique visible enabled Cancel button')
        if action == 'cancel':
            cancel.click(timeout=5000, no_wait_after=True)
        else:
            candidate.keyboard.press('Escape')
        return {'action': action, 'target_id': target['id'], 'dispatched': True}

    def observed_close(target, label):
        # Let a real UI transition occur without relying on source-page JavaScript.
        for index in range(5):
            time.sleep(.25)
            snapshot = capture(label + '-' + str(index))
            if ('targets' if target['kind'] == 'html' else 'native') not in snapshot:
                return False
            if not present(target, snapshot):
                source_alive = any(item['id'] == source['targetId']
                                   for item in snapshot.get('targets', []))
                app_alive = any(item['pid'] == app_pid
                                for item in snapshot.get('native', {}).get('processes', []))
                if not source_alive or not app_alive:
                    report['close_observation_error'] = 'Preview disappeared but source target/app liveness not confirmed'
                    return False
                return True
        return False

    try:
        session = page.context.new_cdp_session(page)
        try:
            source = session.send('Target.getTargetInfo')['targetInfo']
        finally:
            session.detach()
        report['source_target'] = source
        before = capture('before')
        if 'native' not in before or 'targets' not in before:
            report['status'] = 'unavailable-before-click'
            return report
        source_click('open-for-cancel')
        target = None
        for index in range(3):
            time.sleep(.5)
            opened = capture('opened-' + str(index))
            target = target_for(opened, before, source['targetId'])
            if target:
                break
        if not target:
            report['status'] = 'unavailable-unattributed-preview'
            return report
        report.pop('unavailable_reason', None)
        report.pop('candidate_count', None)
        report['confirmed_target'] = target
        for action in ['cancel', 'escape']:
            if action == 'escape':
                baseline = capture('before-reopen')
                source_click('reopen-for-escape')
                reopened = capture('reopened')
                target = target_for(reopened, baseline, source['targetId'])
                if not target:
                    report['status'] = 'unavailable-reopened-target'
                    return report
            action_result = {'action': action, 'target': target}
            report['attempts'].append(action_result)
            try:
                action_result['dispatch'] = close_target(target, action)
            except Exception as error:
                action_result['error'] = type(error).__name__ + ': ' + str(error)
                report['status'] = 'unavailable-targeted-' + action
                return report
            action_result['closed'] = observed_close(target, action + '-after')
            report[action + '_observed'] = action_result['closed']
            if not action_result['closed']:
                report['status'] = 'targeted-' + action + '-did-not-close'
                return report
            try:
                action_result['source_focus'] = page.locator('#s1').get_by_role(
                    'button', name='Печать / PDF', exact=True).evaluate(
                        '(node) => ({documentFocused:document.hasFocus(),buttonFocused:document.activeElement===node})',
                        timeout=3000)
            except PlaywrightError as error:
                action_result['source_focus_error'] = type(error).__name__ + ': ' + str(error)
        report['cancel_observed'] = True
        report['escape_observed'] = True
        focus = [item.get('source_focus', {}) for item in report['attempts'] if item.get('action')]
        report['focus_restored'] = len(focus) == 2 and all(
            item.get('documentFocused') and item.get('buttonFocused') for item in focus)
        report['status'] = ('observed-cancel-reopen-escape-and-focus' if report['focus_restored']
                            else 'observed-cancel-reopen-escape-focus-not-restored')
        return report
    except Exception as error:
        report['status'] = 'probe-error'
        report['error'] = type(error).__name__ + ': ' + str(error)
        capture('error')
        return report
    finally:
        save()
        print('REFERENCE_PRINT_PROBE=' + report['status'], flush=True)
