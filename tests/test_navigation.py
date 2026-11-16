"""Navigation uses the existing five views; no external server or runtime dependencies."""
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[1]

def open_view(page,view):
    if view in ['upload','groups','storage']:
        if not page.locator('#managementNav').evaluate('(node)=>node.open'):
            page.locator('#managementToggle').click()
    page.locator('[data-view='+view+']').click()

class Navigation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start()
        cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    @classmethod
    def tearDownClass(cls):cls.browser.close();cls.pw.stop()
    def setUp(self):
        self.context=self.browser.new_context(viewport={'width':390,'height':844},is_mobile=True,has_touch=True)
        self.page=self.context.new_page();self.errors=[]
        self.page.on('pageerror',lambda error:self.errors.append(str(error)))
        self.page.route('https://navigation.test/',lambda route:route.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
        self.page.goto('https://navigation.test/');self.page.wait_for_function('state.db!==null')
    def tearDown(self):self.context.close();self.assertEqual(self.errors,[])
    def test_navigation_and_last_view_restore(self):
        self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'dashboard')
        self.assertEqual(self.page.locator('.tabs>.tab').all_text_contents(),['Аналитика','Анализ изменений'])
        for view in ['upload','groups','storage','changes','dashboard']:
            open_view(self.page,view)
            self.assertEqual(self.page.locator('.view.active').get_attribute('id'),view)
            self.assertEqual(self.page.locator('.tabs [aria-current=page]').count(),1)
            self.assertFalse(self.page.locator('#managementNav').evaluate('(node)=>node.open'))
            self.page.reload();self.page.wait_for_function('state.db!==null&&document.querySelector(".view.active").id==='+repr(view))
        self.page.evaluate("localStorage.setItem('work-reports-view-v1','unknown')")
        self.page.reload();self.page.wait_for_function('state.db!==null')
        self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'dashboard')
    def test_menu_keyboard_outside_click_and_shortcut(self):
        self.page.locator('#managementToggle').click();self.page.keyboard.press('Escape')
        self.assertFalse(self.page.locator('#managementNav').evaluate('(node)=>node.open'))
        self.assertTrue(self.page.locator('#managementToggle').evaluate('(node)=>node===document.activeElement'))
        self.page.locator('#managementToggle').click();self.page.locator('h1').click()
        self.assertFalse(self.page.locator('#managementNav').evaluate('(node)=>node.open'))
        self.page.locator('#reportsShortcut').click()
        self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'upload')
        self.assertTrue(self.page.locator('#managementToggle').evaluate('(node)=>node.classList.contains("active")'))
    def test_navigation_fit_all_designs_and_mobile_sizes(self):
        for width in [320,390,740,1024,1440]:
            self.page.set_viewport_size({'width':width,'height':900})
            for design in ['modern','classic','compact','neon','glass','corporate','paper','industrial']:
                for theme in ['light','dark']:
                    self.page.evaluate('([design,theme])=>{document.documentElement.dataset.design=design;document.documentElement.dataset.theme=theme}',[design,theme])
                    self.page.locator('#managementNav').evaluate('(node)=>node.open=true')
                    self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'),(width,design,theme))
                    for selector in ['.tabs>.tab','#managementToggle','.management-menu']:
                        for node in self.page.locator(selector).all():
                            b=node.bounding_box();self.assertGreaterEqual(b['x'],0);self.assertLessEqual(b['x']+b['width'],width+1)
                    self.page.locator('#managementNav').evaluate('(node)=>node.open=false')
    def test_unavailable_navigation_storage_does_not_block_views(self):
        self.page.evaluate("()=>{Storage.prototype.setItem=()=>{throw Error('blocked')};Storage.prototype.getItem=()=>{throw Error('blocked')}}")
        open_view(self.page,'storage');self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'storage')
        self.page.evaluate('restoreView()');self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'dashboard')
    def test_desktop_start_restores_view_after_loading_native_data(self):
        self.page.evaluate("localStorage.setItem('work-reports-view-v1','groups')")
        self.context.add_init_script("window.desktopAPI={load:async()=>({reports:[],settings:{orgNames:['СМУ16','АВТО','ТРАК','ЦЕНТР'],groups:[],catalog:[]}})}")
        self.page.reload();self.page.evaluate('window.appReady')
        self.assertEqual(self.page.locator('.view.active').get_attribute('id'),'groups')
        self.assertEqual(self.page.evaluate('state.reports.length'),0)
        self.assertFalse(self.page.evaluate('!!state.desktopBlocked'))
