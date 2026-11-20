"""Optional last-process Windows print inventory; never reports unsupported as passed.

Call probe_reference_print(page, output, app_pid) last in a disposable installed_page.
The caller selects a usable Summary first. No print function replacement, printer
selection, OK, global keyboard input, database writes, or application exports.
Only a newly discovered, strictly attributed print target may receive Cancel/Esc.
"""
import json
import os
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
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct GuiThreadInfo {
  public uint cbSize,flags;
  public IntPtr hwndActive,hwndFocus,hwndCapture,hwndMenuOwner,hwndMoveSize,hwndCaret;
  public Rect rcCaret;
 }
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
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread,ref GuiThreadInfo info);
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
if ($action -in @('embedded-inspect','embedded-cancel','embedded-escape')) {
 $handle=[IntPtr][long]$env:ANALIZ_PRINT_MAIN_HWND
 $browserId=[int]$env:ANALIZ_PRINT_BROWSER_PID
 $browser=@($all | Where-Object {$_.ProcessId -eq $browserId -and $_.ParentProcessId -eq $appId -and $_.Name -eq 'msedgewebview2.exe'})
 if ($browser.Count -ne 1 -or -not $ids.Contains($browserId) -or
     (ProcessOf $handle) -ne $appId -or (ClassOf $handle) -ne 'Tauri Window' -or
     -not [PrintProbeNative]::IsWindowVisible($handle) -or
     -not [PrintProbeNative]::IsWindowEnabled($handle)) {throw 'Embedded preview owner identity changed'}
 $rootElement=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
 $windowCondition=[System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Window)
 $virtual=@($rootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$windowCondition) | Where-Object {
   $_.Current.Name -in @('Print','Печать') -and $_.Current.ProcessId -eq $browserId -and
   $_.Current.IsEnabled -and -not $_.Current.IsOffscreen
 })
 if ($virtual.Count -ne 1) {throw 'Unique embedded Print UIA window not confirmed'}
 $buttonCondition=[System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)
 $buttons=@($virtual[0].FindAll([System.Windows.Automation.TreeScope]::Descendants,$buttonCondition))
 $cancel=@($buttons | Where-Object {$_.Current.Name -in @('Cancel','Отмена') -and
   $_.Current.ProcessId -eq $browserId -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen})
 $submit=@($buttons | Where-Object {$_.Current.Name -in @('Print','Печать') -and
   $_.Current.ProcessId -eq $browserId -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen})
 if ($cancel.Count -ne 1 -or $submit.Count -ne 1) {throw 'Unique actual embedded Cancel/Print controls not confirmed'}
 $facts=[ordered]@{action=$action;main=$handle.ToInt64();browser_pid=$browserId;
   virtual_runtime_id=($virtual[0].GetRuntimeId() -join '.');cancel_name=$cancel[0].Current.Name;
   cancel_runtime_id=($cancel[0].GetRuntimeId() -join '.');dispatched=$false}
 if ($action -eq 'embedded-cancel') {
  $pattern=$cancel[0].GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
  $facts.dispatched=$true
 } elseif ($action -eq 'embedded-escape') {
  $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
  $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker
  $within=$false;$node=$focus
  for($n=0;$n -lt 64 -and $null -ne $node;$n++) {
   if (($node.GetRuntimeId() -join '.') -eq $facts.virtual_runtime_id) {$within=$true;break}
   $node=$walker.GetParent($node)
  }
  $gui=[PrintProbeNative+GuiThreadInfo]::new()
  $gui.cbSize=[System.Runtime.InteropServices.Marshal]::SizeOf([type][PrintProbeNative+GuiThreadInfo])
  # GA_ROOTOWNER=3 includes both native parents and popup ownership.
  if (-not $within -or $focus.Current.ProcessId -ne $browserId -or
      -not [PrintProbeNative]::GetGUIThreadInfo(0,[ref]$gui) -or
      [PrintProbeNative]::GetAncestor([PrintProbeNative]::GetForegroundWindow(),3) -ne $handle -or
      [PrintProbeNative]::GetAncestor($gui.hwndFocus,3) -ne $handle -or
      (ProcessOf $gui.hwndFocus) -ne $browserId) {throw 'Embedded Escape focus is not proven inside the same Print UI and Tauri root'}
  $facts['focus_hwnd']=$gui.hwndFocus.ToInt64()
  $facts['focus_name']=$focus.Current.Name
  if (-not [PrintProbeNative]::PostMessage($gui.hwndFocus,0x100,[UIntPtr]27,[IntPtr]0x00010001)) {throw 'Addressed embedded Escape keydown failed'}
  $facts['keyup_posted']=$false
  if ([PrintProbeNative]::IsWindow($gui.hwndFocus)) {
   $facts['keyup_posted']=[PrintProbeNative]::PostMessage($gui.hwndFocus,0x101,[UIntPtr]27,[IntPtr]0xC0010001)
  }
  $facts.dispatched=$true
 }
 [PSCustomObject]$facts | ConvertTo-Json -Depth 4 -Compress
 exit
}
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
 $keyUpSent=$null
 if ($action -eq 'cancel') {
  $pattern=$cancel[0].GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
 } elseif ($action -eq 'escape') {
  # Queue Escape for this verified HWND only; no SendKeys/SendInput/focus changes.
  if (-not [PrintProbeNative]::PostMessage($handle,0x100,[UIntPtr]27,[IntPtr]1)) {throw 'Targeted Escape keydown failed'}
  # Escape keydown may already destroy the dialog; do not turn that close
  # into an artificial failure by requiring delivery of keyup to a dead HWND.
  $keyUpSent=$false
  if ([PrintProbeNative]::IsWindow($handle)) {
   $keyUpSent=[PrintProbeNative]::PostMessage($handle,0x101,[UIntPtr]27,[IntPtr]0xC0000001)
  }
 } else {throw 'Unsupported print probe action'}
 [PSCustomObject]@{action=$action;hwnd=$handle.ToInt64();dispatched=$true;keyup_posted=$keyUpSent} | ConvertTo-Json -Compress
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


def _native(app_pid, screenshot, action=None, hwnd=None, main_hwnd=None, browser_pid=None):
    env = dict(os.environ, ANALIZ_PRINT_APP_PID=str(app_pid),
               ANALIZ_PRINT_SCREENSHOT=str(screenshot))
    for key in ['ANALIZ_PRINT_ACTION', 'ANALIZ_PRINT_HWND', 'ANALIZ_PRINT_MAIN_HWND', 'ANALIZ_PRINT_BROWSER_PID']:
        env.pop(key, None)
    if action:
        env.update(ANALIZ_PRINT_ACTION=action, ANALIZ_PRINT_HWND=str(hwnd),
                   ANALIZ_PRINT_MAIN_HWND=str(main_hwnd))
        if browser_pid is not None:
            env['ANALIZ_PRINT_BROWSER_PID'] = str(browser_pid)
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


_PRINT_URLS = ('edge://print/', 'chrome://print/',
               'edge-untrusted://print/', 'chrome-untrusted://print/')


def _attribute_preview(snapshot, before, source, app_pid):
    """Attribute an actual preview, including WebView2's HWND-less embedded UI."""
    source_id = source['targetId']
    old_targets = before.get('targets', [])
    old_ids = {target['id'] for target in old_targets}
    roots = [window for window in before.get('native', {}).get('windows', [])
             if window['pid'] == app_pid and window['class'] == 'Tauri Window'
             and window['visible'] and not window['owner']]
    if len(roots) != 1:
        return None
    current_source = [item for item in snapshot.get('targets', []) if item['id'] == source_id]
    if (len(current_source) != 1 or current_source[0].get('type') != 'page'
            or current_source[0].get('pid') != source.get('pid')
            or current_source[0].get('browserContextId') != source.get('browserContextId')):
        return None
    main = roots[0]['hwnd']
    native = snapshot.get('native', {})
    processes = {item['pid']: item for item in native.get('processes', [])}

    def browser_parent(pid):
        seen = set()
        while pid != app_pid:
            if pid in seen or pid not in processes:
                return None
            seen.add(pid)
            item = processes[pid]
            if item['name'] != 'msedgewebview2.exe':
                return None
            if item['parent'] == app_pid:
                return pid
            pid = item['parent']
        return None

    def visible_control(window, control_type, name=None):
        return [node for node in window.get('controls', [])
                if node['type'] == control_type and node['enabled'] and not node['offscreen']
                and (name is None or node['name'] in name)]

    main_after = [window for window in native.get('windows', [])
                  if window['hwnd'] == main and window['pid'] == app_pid
                  and window['class'] == 'Tauri Window' and window['visible']
                  and not window.get('uia_error') and not window['uia_truncated']]
    source_browser = browser_parent(source.get('pid'))
    context = source.get('browserContextId')
    original_page = [item for item in old_targets if item.get('type') == 'page']
    embedded = None
    if (len(main_after) == 1 and context and source_browser
            and len(original_page) == 1 and original_page[0]['id'] == source_id
            and original_page[0].get('pid') == source.get('pid')
            and original_page[0].get('browserContextId') == context
            and not visible_control(roots[0], 'ControlType.Window', ('Print', 'Печать'))):
        window = main_after[0]
        print_windows = visible_control(window, 'ControlType.Window', ('Print', 'Печать'))
        cancel = visible_control(window, 'ControlType.Button', ('Cancel', 'Отмена'))
        submit = visible_control(window, 'ControlType.Button', ('Print', 'Печать'))
        if (len(print_windows) == len(cancel) == len(submit) == 1
                and all(node['pid'] == source_browser for node in print_windows + cancel + submit)):
            embedded = {'main': main, 'browser_pid': source_browser,
                        'browser_context': context, 'uia_window': print_windows[0],
                        'uia_cancel': cancel[0]}

    candidates = []
    for target in snapshot.get('targets', []):
        if (target['id'] in old_ids or target.get('type') != 'page'
                or target.get('url', '').split('?', 1)[0] not in _PRINT_URLS):
            continue
        opener = target.get('openerId')
        if opener == source_id:
            candidates.append({'kind': 'html', 'id': target['id'], 'url': target['url'],
                               'attribution': {'source_opener': source_id}})
            continue
        if opener or not embedded or target.get('browserContextId') != context:
            continue
        if browser_parent(target.get('pid')) != source_browser:
            continue
        pdf = [item for item in snapshot.get('targets', [])
               if item.get('parentId') == target['id'] and item.get('type') == 'iframe'
               and item.get('browserContextId') == context
               and item.get('url', '').startswith('chrome-untrusted://print/')
               and item.get('url', '').endswith('/print.pdf')
               and browser_parent(item.get('pid')) == source_browser]
        if len(pdf) == 1:
            candidates.append({'kind': 'html', 'id': target['id'], 'url': target['url'],
                               'attribution': dict(embedded, pdf_target=pdf[0]['id'],
                                                   source_target=source_id)})

    old_windows = {window['hwnd'] for window in before.get('native', {}).get('windows', [])}
    for window in native.get('windows', []):
        cancel = [node for node in visible_control(window, 'ControlType.Button', ('Cancel', 'Отмена'))
                  if node['control_id'] == 2]
        if (window['hwnd'] not in old_windows and window['class'] == '#32770'
                and window['title'] in ('Print', 'Печать', 'Print Preview', 'Предварительный просмотр')
                and main in window['owners'] and window['visible'] and window['enabled']
                and not window.get('uia_error') and not window['uia_truncated'] and len(cancel) == 1):
            candidates.append({'kind': 'native', 'hwnd': window['hwnd'], 'main': main})
    return candidates[0] if len(candidates) == 1 else None


class _PreviewProtocolError(RuntimeError):
    pass


class _PreviewSession:
    """Public CDP routing to one attributed target, not the source or global input."""

    def __init__(self, browser_session, target):
        self.browser = browser_session
        self.target = target
        self.sequence = 0
        self.replies = {}
        self.detached = False
        self.last_command_dispatched = False
        self.session = None
        browser_session.on('Target.receivedMessageFromTarget', self._received)
        browser_session.on('Target.detachedFromTarget', self._detached)
        try:
            self.session = browser_session.send('Target.attachToTarget', {
                'targetId': target['id'], 'flatten': False})['sessionId']
        except Exception:
            self.close()
            raise

    def _received(self, event):
        if event.get('sessionId') == self.session:
            message = json.loads(event['message'])
            if 'id' in message:
                self.replies[message['id']] = message

    def _detached(self, event):
        if event.get('sessionId') == self.session:
            self.detached = True

    def send(self, method, params=None, allow_detach=False):
        self.last_command_dispatched = False
        self.sequence += 1
        sequence = self.sequence
        self.browser.send('Target.sendMessageToTarget', {
            'sessionId': self.session,
            'message': json.dumps({'id': sequence, 'method': method, 'params': params or {}})})
        self.last_command_dispatched = True
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if sequence in self.replies:
                message = self.replies.pop(sequence)
                if 'error' in message:
                    raise _PreviewProtocolError('Preview CDP ' + method + ': ' + str(message['error']))
                return message.get('result', {})
            if self.detached:
                if allow_detach:
                    return {'target_detached_after_dispatch': True}
                raise RuntimeError('Attributed print target detached during ' + method)
            # A browser-level round trip pumps CDP replies without evaluating
            # the source page, which is blocked while its print UI is open.
            self.browser.send('Target.getTargets')
            time.sleep(.02)
        raise RuntimeError('No response from attributed preview to ' + method)

    def close(self):
        self.browser.remove_listener('Target.receivedMessageFromTarget', self._received)
        self.browser.remove_listener('Target.detachedFromTarget', self._detached)
        if self.session and not self.detached:
            self.browser.send('Target.detachFromTarget', {'sessionId': self.session})


_CANCEL_CONTROL = r"""() => {
 const urls=['edge://print/','chrome://print/','edge-untrusted://print/','chrome-untrusted://print/'];
 if(!urls.includes(location.href.split('?')[0])) throw Error('Unexpected preview document');
 const nodes=[];
 const walk=root=>{for(const node of root.querySelectorAll('*')){
   if(node.matches('cr-button,button,[role="button"]')) nodes.push(node);
   if(node.shadowRoot) walk(node.shadowRoot);
 }};
 walk(document);
 const visible=node=>node.getClientRects().length && getComputedStyle(node).visibility!=='hidden' &&
   !node.disabled && node.getAttribute('aria-disabled')!=='true';
 const text=node=>(node.innerText || node.textContent || '').trim();
 const matches=nodes.filter(node=>visible(node) && /^(Cancel|Отмена)$/.test(text(node)));
 const outer=matches.filter(node=>node.tagName==='CR-BUTTON');
 const found=outer.length?outer:matches;
 if(found.length!==1) throw Error('Unique actual visible Cancel control not found: '+JSON.stringify(
   nodes.filter(visible).map(n=>({tag:n.tagName,id:n.id,text:text(n)}))));
 return found[0];
}"""


def probe_reference_print(page, output, app_pid, endpoint='http://127.0.0.1:9227'):
    """Return evidence; unsupported/error are diagnostic statuses, never Cancel passes."""
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    report = {'app_pid': app_pid, 'status': 'started', 'attempts': [], 'snapshots': [],
              'restrictions': 'No mocks, printer selection, print-to-file, OK, or global input.'}
    path = output / 'reference-print-probe.json'
    browser_session = None

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
        # /json/list often omits openerId. Browser Target.getTargets provides
        # the actual source relationship without evaluating blocked source JS.
        if browser_session is not None:
            try:
                infos = browser_session.send('Target.getTargets')['targetInfos']
                snapshot['http_targets'] = snapshot.get('targets', [])
                snapshot['targets'] = [dict(item, id=item['targetId']) for item in infos]
                snapshot['target_protocol'] = 'Target.getTargets'
            except Exception as error:
                snapshot['target_protocol_error'] = type(error).__name__ + ': ' + str(error)
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
        target = _attribute_preview(snapshot, before, source, app_pid)
        if target is None:
            report['unavailable_reason'] = 'No unique new print target with proven source opener or embedded WebView ownership'
        return target

    def present(target, snapshot):
        if target['kind'] == 'html':
            return any(item['id'] == target['id'] for item in snapshot.get('targets', []))
        return any(item['hwnd'] == target['hwnd'] and item['visible']
                   for item in snapshot.get('native', {}).get('windows', []))

    def close_target(target, action):
        if target['kind'] == 'native':
            return _native(app_pid, output / 'unused.png', action, target['hwnd'], target['main'])
        result = {'action': action, 'target_id': target['id']}
        proof = target.get('attribution', {})

        def embedded(mode):
            if 'main' not in proof or 'browser_pid' not in proof:
                raise RuntimeError('No embedded UIA ownership proof for fallback')
            facts = _native(app_pid, output / 'unused.png', 'embedded-' + mode,
                            main_hwnd=proof['main'], browser_pid=proof['browser_pid'])
            result['uia_' + mode] = facts
            if mode != 'inspect':
                result['strategy'] = 'verified-embedded-uia' if mode == 'cancel' else 'verified-focused-hwnd'
                result['dispatched'] = facts['dispatched']
            return result

        try:
            if browser_session is None:
                raise RuntimeError('No browser CDP session for attributed embedded preview')
            current = browser_session.send('Target.getTargetInfo', {'targetId': target['id']})['targetInfo']
            if current['url'] != target['url']:
                raise RuntimeError('Attributed preview document changed before action')
            if proof.get('browser_context') and current.get('browserContextId') != proof['browser_context']:
                raise RuntimeError('Attributed preview context changed before action')
            preview = _PreviewSession(browser_session, target)
        except (PlaywrightError, RuntimeError) as error:
            result['cdp_attach_error'] = type(error).__name__ + ': ' + str(error)
            return embedded(action)
        try:
            try:
                control = preview.send('Runtime.evaluate', {
                    'expression': '(() => {const n=(' + _CANCEL_CONTROL + ')();return {tag:n.tagName,id:n.id,text:(n.innerText||n.textContent||" ").trim(),url:location.href};})()',
                    'returnByValue': True})
                if 'exceptionDetails' in control:
                    raise RuntimeError('Actual Cancel inspection: ' + str(control['exceptionDetails']))
                result['cancel_control'] = control['result']['value']
            except (PlaywrightError, RuntimeError) as error:
                result['cdp_dom_error'] = type(error).__name__ + ': ' + str(error)
                if action == 'cancel':
                    return embedded('cancel')
                embedded('inspect')
            if action == 'cancel':
                try:
                    clicked = preview.send('Runtime.evaluate', {
                        'expression': '(() => {const n=(' + _CANCEL_CONTROL + ')();setTimeout(()=>n.click(),0);return {cancel_scheduled:true};})()',
                        'returnByValue': True, 'userGesture': True}, allow_detach=True)
                except _PreviewProtocolError as error:
                    result['cdp_cancel_error'] = str(error)
                    return embedded('cancel')
                if 'exceptionDetails' in clicked:
                    result['cdp_cancel_error'] = str(clicked['exceptionDetails'])
                    return embedded('cancel')
                result['cancel_dispatch'] = clicked
                result['strategy'] = 'exact-preview-cdp-cancel-control'
            else:
                key = {'key': 'Escape', 'code': 'Escape', 'windowsVirtualKeyCode': 27,
                       'nativeVirtualKeyCode': 27}
                try:
                    result['keydown'] = preview.send('Input.dispatchKeyEvent',
                        dict(key, type='keyDown'), allow_detach=True)
                except _PreviewProtocolError as error:
                    result['cdp_escape_error'] = str(error)
                    return embedded('escape')
                except PlaywrightError as error:
                    if preview.last_command_dispatched:
                        raise  # Delivery is unknown: never queue a second Escape.
                    result['cdp_escape_error'] = str(error)
                    return embedded('escape')
                if not preview.detached:
                    live = browser_session.send('Target.getTargets')['targetInfos']
                    if any(item['targetId'] == target['id'] for item in live):
                        try:
                            result['keyup'] = preview.send('Input.dispatchKeyEvent',
                                dict(key, type='keyUp'), allow_detach=True)
                        except (PlaywrightError, RuntimeError) as error:
                            live = browser_session.send('Target.getTargets')['targetInfos']
                            if any(item['targetId'] == target['id'] for item in live):
                                raise
                            result['keyup'] = {'target_closed_before_keyup': True, 'error': str(error)}
                    else:
                        result['keyup'] = {'target_closed_before_keyup': True}
                result['strategy'] = 'exact-preview-cdp-escape'
            result['dispatched'] = True
            return result
        finally:
            try:
                preview.close()
            except PlaywrightError as error:
                result['session_detach_error'] = type(error).__name__ + ': ' + str(error)

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
        try:
            browser_session = page.context.browser.new_browser_cdp_session()
        except PlaywrightError as error:
            report['browser_session_error'] = type(error).__name__ + ': ' + str(error)
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
                capture(action + '-dispatch-error')
                return report
            action_result['closed'] = observed_close(target, action + '-after')
            proof = target.get('attribution', {})
            if (not action_result['closed'] and target['kind'] == 'html'
                    and 'main' in proof and 'browser_pid' in proof):
                # A renderer command can be accepted without closing Edge UI.
                # Revalidate the actual embedded Print/Cancel controls. Escape
                # additionally requires native and UIA focus inside that same
                # owner root; never repeat a key against an unproven target.
                try:
                    action_result['guarded_native_fallback'] = _native(
                        app_pid, output / 'unused.png', 'embedded-' + action,
                        main_hwnd=proof['main'], browser_pid=proof['browser_pid'])
                    action_result['closed'] = observed_close(target, action + '-fallback-after')
                except Exception as error:
                    action_result['guarded_native_fallback_error'] = type(error).__name__ + ': ' + str(error)
                    capture(action + '-fallback-error')
            report[action + '_observed'] = action_result['closed']
            if not action_result['closed']:
                report['status'] = 'targeted-' + action + '-closure-not-confirmed'
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
        if browser_session is not None:
            try:
                browser_session.detach()
            except PlaywrightError as error:
                report['browser_session_detach_error'] = type(error).__name__ + ': ' + str(error)
        save()
        print('REFERENCE_PRINT_PROBE=' + report['status'], flush=True)
