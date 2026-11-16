"""Browser regression checks for native selects and the Tauri-only update notice."""
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[1]
class SelectsUpdates(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start();cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    @classmethod
    def tearDownClass(cls):cls.browser.close();cls.pw.stop()
    def setUp(self):
        self.context=self.browser.new_context(viewport={'width':390,'height':900},is_mobile=True,has_touch=True)
        self.page=self.context.new_page();self.errors=[];self.page.on('pageerror',lambda e:self.errors.append(str(e)))
        self.page.route('https://selects.test/',lambda r:r.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
        self.page.goto('https://selects.test/');self.page.evaluate('window.appReady')
    def tearDown(self):self.context.close();self.assertEqual(self.errors,[])
    def updates(self):
        self.page.evaluate('''()=>{window.downloads=0;window.tauriOperations={saves:0,reads:0,updating:false};window.__TAURI__={core:{invoke:async()=>true},updater:{check:async()=>({version:'0.2.0',currentVersion:'0.1.2',body:'Описание <img src=x onerror=bad()>',close:async()=>{},downloadAndInstall:async()=>{downloads++}})}}}''')
        self.page.add_script_tag(content=(ROOT/'tauri/updates.js').read_text());self.page.wait_for_function('document.querySelector("#appUpdateStatus").textContent.includes("Доступна версия")')
    def test_native_and_multiple_selects_share_geometry_and_work_with_keyboard(self):
        self.assertFalse(self.page.locator('#appUpdateNotice').count())
        for width in [320,390,1024]:
            self.page.set_viewport_size({'width':width,'height':900})
            for theme in ['light','dark']:
                self.page.evaluate('(theme)=>document.documentElement.dataset.theme=theme',theme)
                properties=self.page.evaluate('''()=>{const get=node=>{const s=getComputedStyle(node);return [s.fontSize,s.borderRadius,s.borderColor,s.backgroundColor,node.getBoundingClientRect().height]};return [get(document.querySelector('#periodType')),get(document.querySelector('#orgFilter summary')),get(document.querySelector('#categoryFilter .category-filter-control'))]}''')
                self.assertEqual(properties[0],properties[1]);self.assertEqual(properties[0],properties[2])
                self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
        self.page.locator('#periodType').click();self.page.keyboard.press('End');self.page.keyboard.press('Enter')
        self.assertEqual(self.page.locator('#periodType').input_value(),'day')
        summary=self.page.locator('#orgFilter summary');summary.click()
        checkbox=self.page.locator('#orgFilter input').first;was=checkbox.is_checked();checkbox.focus();self.page.keyboard.press('Space')
        self.assertNotEqual(self.page.locator('#orgFilter input').first.is_checked(),was)
        self.page.keyboard.press('Escape');self.assertFalse(self.page.locator('#orgFilter details').evaluate('(node)=>node.open'))
    def test_notice_deferral_badge_and_plain_text_notes(self):
        self.updates();self.assertTrue(self.page.locator('#appUpdateNotice').is_visible());self.assertTrue(self.page.locator('#appUpdateBadge').is_visible())
        self.page.locator('#noticeWhatsNew').click();self.assertIn('<img',self.page.locator('#appUpdateNotes').inner_text());self.assertEqual(self.page.locator('#appUpdateNotes img').count(),0)
        self.page.keyboard.press('Escape');self.page.locator('#deferAppUpdate').click();self.assertFalse(self.page.locator('#appUpdateNotice').is_visible())
        self.page.reload();self.page.evaluate('window.appReady');self.updates()
        self.assertFalse(self.page.locator('#appUpdateNotice').is_visible());self.assertTrue(self.page.locator('#appUpdateBadge').is_visible())
        self.page.locator('#managementToggle').click();self.page.locator('[data-view=storage]').click()
        self.page.locator('#checkAppUpdate').click();self.assertTrue(self.page.locator('#appUpdateNotice').is_visible())
        self.page.once('dialog',lambda d:d.dismiss());self.page.locator('#noticeInstallUpdate').click();self.assertEqual(self.page.evaluate('downloads'),0)
    def test_update_blocks_saves_and_active_import_then_requires_confirmation(self):
        self.updates();self.page.evaluate('window.tauriOperations.saves=1')
        self.page.locator('#noticeInstallUpdate').click();self.assertEqual(self.page.evaluate('downloads'),0)
        self.page.evaluate('window.tauriOperations.saves=0;state.excelImports=1')
        self.page.locator('#noticeInstallUpdate').click();self.assertEqual(self.page.evaluate('downloads'),0)
        self.page.evaluate('state.excelImports=0')
        self.page.once('dialog',lambda d:d.accept());self.page.locator('#noticeInstallUpdate').click()
        self.assertEqual(self.page.evaluate('downloads'),1);self.assertFalse(self.page.evaluate('state.appUpdating'))
    def test_long_native_options_and_update_notice_fit_phone(self):
        self.updates()
        for width in [320,390]:
            self.page.set_viewport_size({'width':width,'height':900})
            self.page.locator('#periodType option[value=week]').evaluate("node=>node.textContent='Очень длинное название отчётного периода для проверки переноса и границ'" )
            self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
            self.assertTrue(self.page.locator('#noticeInstallUpdate').is_visible())
