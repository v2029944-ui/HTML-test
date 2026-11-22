"""Keep Windows smoke waits compatible with the unchanged strict application CSP."""
import sys
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright, Error as PlaywrightError

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tauri' / 'tests'))
import windows_smoke as smoke

class CspSafeWindowsWaits(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start()
        cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    @classmethod
    def tearDownClass(cls):cls.browser.close();cls.pw.stop()
    def setUp(self):
        self.context=self.browser.new_context();self.page=self.context.new_page()
        self.page.route('https://strict-csp.test/app.js',lambda route:route.fulfill(
            content_type='text/javascript',body="""
              window.pageEvalBlocked=false;
              try{window.eval('true')}catch(error){window.pageEvalBlocked=error.name==='EvalError'}
              window.ready=false;
              document.querySelector('#start').addEventListener('click',()=>setTimeout(()=>window.ready=true,150));
            """))
        self.page.route('https://strict-csp.test/',lambda route:route.fulfill(
            headers={'Content-Security-Policy':"default-src 'self'; script-src 'self'"},
            content_type='text/html',body='<!doctype html><title>CSP wait regression</title><button id="start">Start</button><script src="/app.js"></script>'))
        self.page.goto('https://strict-csp.test/')
    def tearDown(self):self.context.close()
    def test_eventual_state_is_observed_while_page_eval_remains_blocked(self):
        self.assertTrue(self.page.evaluate('window.pageEvalBlocked'))
        self.page.locator('#start').click()
        smoke.wait_for_app_state(self.page,'window.ready===true','Delayed workbook',timeout_ms=2000)
        self.assertTrue(self.page.evaluate('window.ready'))
    def test_timeout_identifies_the_operation(self):
        with self.assertRaisesRegex(RuntimeError,'Preview of 366 daily reports did not complete within 50 ms'):
            smoke.wait_for_app_state(self.page,'false','Preview of 366 daily reports',timeout_ms=50)
    def test_script_errors_are_not_hidden_as_timeouts(self):
        with self.assertRaisesRegex(PlaywrightError,'actual application error'):
            smoke.wait_for_app_state(self.page,"(()=>{throw Error('actual application error')})()",'Workbook')
