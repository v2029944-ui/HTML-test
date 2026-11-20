"""Regression facts reduced from isolated Windows a2bff01/run44724400128.
No Windows automation is performed by these attribution tests.
"""
import copy
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

from reference_print_probe import _NATIVE, _attribute_preview


CAPTURED = {'app_pid': 1500,
 'source': {'targetId': 'E7C32ABB19344FD80B48E0904B210D94',
            'type': 'page',
            'title': 'Анализ работ — локальные отчёты',
            'url': 'http://tauri.localhost/',
            'attached': True,
            'canAccessOpener': False,
            'browserContextId': 'EF03D4B4DA727853CD5931EA2D48EF2D',
            'pid': 6588},
 'before': {'targets': [{'targetId': 'E7C32ABB19344FD80B48E0904B210D94',
                         'type': 'page',
                         'title': 'Анализ работ — локальные отчёты',
                         'url': 'http://tauri.localhost/',
                         'attached': True,
                         'canAccessOpener': False,
                         'browserContextId': 'EF03D4B4DA727853CD5931EA2D48EF2D',
                         'pid': 6588,
                         'id': 'E7C32ABB19344FD80B48E0904B210D94'}],
            'native': {'processes': [{'pid': 1500,
                                      'parent': 2740,
                                      'name': 'analiz-rabot-tauri.exe',
                                      'created': '2026-11-20T11:22:17.1428160+00:00'},
                                     {'pid': 6960,
                                      'parent': 1500,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2023300+00:00'},
                                     {'pid': 8196,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2118940+00:00'},
                                     {'pid': 872,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2918640+00:00'},
                                     {'pid': 8944,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2918720+00:00'},
                                     {'pid': 1688,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.3262310+00:00'},
                                     {'pid': 6588,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.3709630+00:00'}],
                       'windows': [{'hwnd': 524876,
                                    'pid': 1500,
                                    'class': 'Tauri Window',
                                    'title': 'Анализ выполненных работ',
                                    'visible': True,
                                    'enabled': True,
                                    'owner': 0,
                                    'owners': [],
                                    'parent': 0,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 917686,
                                    'pid': 1500,
                                    'class': 'Tao Thread Event Target',
                                    'title': '',
                                    'visible': True,
                                    'enabled': True,
                                    'owner': 0,
                                    'owners': [],
                                    'parent': 0,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 262818,
                                    'pid': 1500,
                                    'class': 'MSCTFIME UI',
                                    'title': 'MSCTFIME UI',
                                    'visible': False,
                                    'enabled': False,
                                    'owner': 786808,
                                    'owners': [786808, 524876],
                                    'parent': 786808,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 786808,
                                    'pid': 1500,
                                    'class': 'IME',
                                    'title': 'Default IME',
                                    'visible': False,
                                    'enabled': False,
                                    'owner': 524876,
                                    'owners': [524876],
                                    'parent': 524876,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False}]}},
 'opened': {'targets': [{'targetId': 'E7C32ABB19344FD80B48E0904B210D94',
                         'type': 'page',
                         'title': 'Анализ работ — локальные отчёты',
                         'url': 'http://tauri.localhost/',
                         'attached': True,
                         'canAccessOpener': False,
                         'browserContextId': 'EF03D4B4DA727853CD5931EA2D48EF2D',
                         'pid': 6588,
                         'id': 'E7C32ABB19344FD80B48E0904B210D94'},
                        {'targetId': '82C644BE724E4F326435B06330E85B20',
                         'type': 'page',
                         'title': 'Анализ работ — локальные отчёты',
                         'url': 'edge://print/',
                         'attached': False,
                         'canAccessOpener': False,
                         'browserContextId': 'EF03D4B4DA727853CD5931EA2D48EF2D',
                         'pid': 7188,
                         'id': '82C644BE724E4F326435B06330E85B20'},
                        {'targetId': '46AA251E73D409B1ADD0E6AFA34E1290',
                         'type': 'iframe',
                         'title': 'chrome-untrusted://print/28088476F391548C915FB0828F01F610/0/print.pdf',
                         'url': 'chrome-untrusted://print/28088476F391548C915FB0828F01F610/0/print.pdf',
                         'attached': True,
                         'parentId': '82C644BE724E4F326435B06330E85B20',
                         'canAccessOpener': False,
                         'parentFrameId': '8C5362405161F67C7A90746AC124F357',
                         'browserContextId': 'EF03D4B4DA727853CD5931EA2D48EF2D',
                         'pid': 2860,
                         'id': '46AA251E73D409B1ADD0E6AFA34E1290'}],
            'native': {'processes': [{'pid': 1500,
                                      'parent': 2740,
                                      'name': 'analiz-rabot-tauri.exe',
                                      'created': '2026-11-20T11:22:17.1428160+00:00'},
                                     {'pid': 6960,
                                      'parent': 1500,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2023300+00:00'},
                                     {'pid': 8196,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2118940+00:00'},
                                     {'pid': 872,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2918640+00:00'},
                                     {'pid': 8944,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.2918720+00:00'},
                                     {'pid': 1688,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.3262310+00:00'},
                                     {'pid': 6588,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:17.3709630+00:00'},
                                     {'pid': 7188,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:51.9523090+00:00'},
                                     {'pid': 2860,
                                      'parent': 6960,
                                      'name': 'msedgewebview2.exe',
                                      'created': '2026-11-20T11:22:53.3978920+00:00'}],
                       'windows': [{'hwnd': 524876,
                                    'pid': 1500,
                                    'class': 'Tauri Window',
                                    'title': 'Анализ выполненных работ',
                                    'visible': True,
                                    'enabled': True,
                                    'owner': 0,
                                    'owners': [],
                                    'parent': 0,
                                    'controls': [{'type': 'ControlType.Window',
                                                  'name': 'Print',
                                                  'automation_id': '',
                                                  'pid': 6960,
                                                  'hwnd': 0,
                                                  'control_id': None,
                                                  'enabled': True,
                                                  'offscreen': False},
                                                 {'type': 'ControlType.Button',
                                                  'name': 'Print',
                                                  'automation_id': '',
                                                  'pid': 6960,
                                                  'hwnd': 0,
                                                  'control_id': None,
                                                  'enabled': True,
                                                  'offscreen': False},
                                                 {'type': 'ControlType.Button',
                                                  'name': 'Cancel',
                                                  'automation_id': '',
                                                  'pid': 6960,
                                                  'hwnd': 0,
                                                  'control_id': None,
                                                  'enabled': True,
                                                  'offscreen': False}],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 917686,
                                    'pid': 1500,
                                    'class': 'Tao Thread Event Target',
                                    'title': '',
                                    'visible': True,
                                    'enabled': True,
                                    'owner': 0,
                                    'owners': [],
                                    'parent': 0,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 262818,
                                    'pid': 1500,
                                    'class': 'MSCTFIME UI',
                                    'title': 'MSCTFIME UI',
                                    'visible': False,
                                    'enabled': False,
                                    'owner': 786808,
                                    'owners': [786808, 524876],
                                    'parent': 786808,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False},
                                   {'hwnd': 786808,
                                    'pid': 1500,
                                    'class': 'IME',
                                    'title': 'Default IME',
                                    'visible': False,
                                    'enabled': False,
                                    'owner': 524876,
                                    'owners': [524876],
                                    'parent': 524876,
                                    'controls': [],
                                    'uia_error': None,
                                    'uia_truncated': False}]}}}

class PrintAttributionTests(unittest.TestCase):
    def setUp(self):
        self.f = copy.deepcopy(CAPTURED)
        self.opened = self.f['opened']
        self.preview = next(t for t in self.opened['targets'] if t['url'] == 'edge://print/')
        self.pdf = next(t for t in self.opened['targets'] if t['type'] == 'iframe')
        self.main = next(w for w in self.opened['native']['windows'] if w['class'] == 'Tauri Window')

    def target(self):
        return _attribute_preview(self.opened, self.f['before'], self.f['source'], self.f['app_pid'])

    def test_captured_embedded_print_has_no_opener_or_hwnd(self):
        target = self.target()
        self.assertEqual(target['id'], self.preview['id'])
        self.assertEqual(target['attribution']['main'], 524876)
        self.assertEqual(target['attribution']['browser_pid'], 6960)
        self.assertEqual(target['attribution']['uia_cancel']['hwnd'], 0)

    def test_unknown_browser_context_is_rejected(self):
        self.preview['browserContextId'] = 'different'
        self.assertIsNone(self.target())

    def test_unknown_renderer_is_rejected(self):
        self.preview['pid'] = 999999
        self.assertIsNone(self.target())

    def test_different_browser_process_is_rejected(self):
        self.opened['native']['processes'].append({'pid':999999,'parent':1500,'name':'msedgewebview2.exe'})
        next(p for p in self.opened['native']['processes'] if p['pid']==self.preview['pid'])['parent']=999999
        self.assertIsNone(self.target())

    def test_missing_pdf_parent_link_is_rejected(self):
        self.pdf['parentId'] = 'unrelated'
        self.assertIsNone(self.target())

    def test_unknown_pdf_renderer_is_rejected(self):
        self.pdf['pid'] = 999999
        self.assertIsNone(self.target())

    def test_cancel_must_be_visible_and_unique(self):
        cancel = next(c for c in self.main['controls'] if c['name']=='Cancel')
        cancel['offscreen']=True
        self.assertIsNone(self.target())
        cancel['offscreen']=False
        self.main['controls'].append(copy.deepcopy(cancel))
        self.assertIsNone(self.target())

    def test_print_uia_must_belong_to_same_browser(self):
        next(c for c in self.main['controls'] if c['name']=='Cancel')['pid']=999999
        self.assertIsNone(self.target())

    def test_truncated_uia_is_rejected(self):
        self.main['uia_truncated']=True
        self.assertIsNone(self.target())

    def test_other_app_window_is_rejected(self):
        self.main['hwnd']=999999
        self.assertIsNone(self.target())

    def test_two_tauri_windows_are_rejected(self):
        other=copy.deepcopy(self.main);other['hwnd']=999999
        self.f['before']['native']['windows'].append(other)
        self.assertIsNone(self.target())

    def test_two_previews_are_rejected(self):
        other=copy.deepcopy(self.preview);other['id']='other-preview'
        pdf=copy.deepcopy(self.pdf);pdf['id']='other-pdf';pdf['parentId']='other-preview'
        self.opened['targets'].extend([other,pdf])
        self.assertIsNone(self.target())

    def test_contradictory_opener_is_rejected(self):
        self.preview['openerId']='other-source'
        self.assertIsNone(self.target())

    def test_two_original_pages_are_rejected(self):
        other=copy.deepcopy(self.f['before']['targets'][0]);other['id']='other-source'
        self.f['before']['targets'].append(other)
        self.assertIsNone(self.target())

    def test_missing_source_target_is_rejected(self):
        self.opened['targets']=[t for t in self.opened['targets'] if t['id']!=self.f['source']['targetId']]
        self.assertIsNone(self.target())

    def test_preexisting_print_ui_is_rejected(self):
        before_main=next(w for w in self.f['before']['native']['windows'] if w['class']=='Tauri Window')
        before_main['controls']=copy.deepcopy(self.main['controls'])
        self.assertIsNone(self.target())


class PhysicalEscapePacketTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('powershell') or shutil.which('pwsh'), 'PowerShell runtime required')
    def test_real_native_packet_layout_and_unknown_target_refusal(self):
        # Compile actual C# declarations and inspect the Win32 INPUT bytes.
        # No Win32 functions or input injection are invoked by this test.
        declarations = re.search("Add-Type @'\\n(.*?)\\n'@", _NATIVE, re.S).group(1)
        validation = r'''
$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
[void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'native.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors | Out-String)}
Add-Type -Path (Join-Path $PSScriptRoot 'native.cs')
$keys=[PrintProbeNative]::EscapeInputs()
if($keys.Length -ne 2){throw 'Escape must contain exactly one down/up pair'}
if([System.Runtime.InteropServices.Marshal]::SizeOf([type][PrintProbeNative+Input]) -ne 40){throw 'Invalid x64 INPUT size'}
if([System.Runtime.InteropServices.Marshal]::OffsetOf([type][PrintProbeNative+Input],'data').ToInt32() -ne 8){throw 'Invalid INPUT union alignment'}
if($keys[0].type -ne 1 -or $keys[1].type -ne 1 -or
   $keys[0].data.keyboard.wVk -ne 27 -or $keys[1].data.keyboard.wVk -ne 27 -or
   $keys[0].data.keyboard.dwFlags -ne 0 -or $keys[1].data.keyboard.dwFlags -ne 2){throw 'Not one physical VK_ESCAPE down/up pair'}
$refused=$false
try{[void][PrintProbeNative]::SendGuardedEscape([IntPtr]::Zero,[IntPtr]::Zero,0,0)}
catch{if($_.Exception.ToString().Contains('Unproven input target')){$refused=$true}else{throw}}
if(-not $refused){throw 'Unknown target was not refused before any Win32 call'}
'PASS physical Escape packet and unknown-target refusal'
'''
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'native.ps1').write_text(_NATIVE, encoding='utf-8')
            (root / 'native.cs').write_text(declarations, encoding='utf-8')
            (root / 'validate.ps1').write_text(validation, encoding='utf-8')
            result = subprocess.run([shutil.which('powershell') or shutil.which('pwsh'),
                                     '-NoProfile', '-NonInteractive', '-File', str(root / 'validate.ps1')],
                                    capture_output=True, encoding='utf-8', errors='replace', timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('PASS physical Escape packet', result.stdout)


if __name__ == '__main__':
    unittest.main()
