"""Local Chromium checks; run python -m unittest discover -s tests -v. No server/Actions."""
import csv
import io
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


class ImportControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw = sync_playwright().start()
        cls.browser = cls.pw.chromium.launch(executable_path='/usr/bin/chromium', headless=True)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.pw.stop()

    def setUp(self):
        self.context = self.browser.new_context(viewport={'width':390,'height':844},is_mobile=True,has_touch=True,accept_downloads=True)
        self.page = self.context.new_page()
        self.errors = []
        self.page.on('pageerror', lambda error:self.errors.append(str(error)))
        self.page.route('https://html.local.test/index.html', lambda route:route.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
        self.page.goto('https://html.local.test/index.html')
        self.page.wait_for_function('state.db !== null')

    def tearDown(self):
        errors = self.errors[:]
        self.context.close()
        self.assertEqual(errors, [], 'Browser runtime errors')

    def test_native_date_and_month_fit_empty_and_filled(self):
        self.page.locator('[data-view=upload]').click()
        for width in [320,375,390,428,1024]:
            self.page.set_viewport_size({'width':width,'height':844})
            for kind,selector,value,start in [('week','#importWeek','2026-10-19','2026-10-19'),('month','#importMonth','2026-11','2026-11-01')]:
                self.page.locator('#importType').select_option(kind)
                for candidate in ['',value]:
                    self.page.locator(selector).fill(candidate)
                    field=self.page.locator(selector).bounding_box()
                    adjacent=self.page.locator('#importType').bounding_box()
                    parent=self.page.locator(selector).locator('..').bounding_box()
                    self.assertAlmostEqual(field['height'],adjacent['height'],delta=1,msg=(width,kind,candidate))
                    self.assertLessEqual(field['x']+field['width'],parent['x']+parent['width']+1)
                    self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
                    if candidate:self.assertEqual(self.page.evaluate('importStart()'),start)
                self.assertFalse(self.page.locator('#monthStartLabel' if kind=='week' else '#weekStartLabel').is_visible())

    def test_conditional_chart_filter_and_mixed_metric_csv(self):
        self.page.evaluate("""async()=>{
          const reports=['2026-10-19','2026-10-26'].map((start,i)=>{
            const organizations=DEFAULT_ORGS.filter((org,j)=>i===0||j!==0);
            return {id:start,type:'week',start,end:periodEnd(start,'week'),organizations,totals:[],warnings:[],rows:organizations.map(organization=>{
              const j=DEFAULT_ORGS.indexOf(organization);return {organization,work:'Проверочная работа',count:j+1.5,amount:10*(j+1)*(i+1)};
            })};
          });
          await storeReports(reports);state.settings.groups=[];state.selectionInitialized=false;state.selectedMetrics=new Set(['count','amount']);renderAll();
          document.querySelector('#overviewMode').value='range';document.querySelector('#rangeStart').value='2026-10-19';document.querySelector('#rangeEnd').value='2026-10-26';
          document.querySelector('#advancedAnalytics').open=true;renderDashboard(false);
        }""")
        self.page.locator('#chartBreakdown').select_option('works')
        self.assertTrue(self.page.locator('#chartOrgScopeLabel').evaluate('(node)=>node.hidden && getComputedStyle(node).display==="none"'))
        self.page.locator('#chartBreakdown').select_option('organizations')
        self.assertTrue(self.page.locator('#chartOrgScopeLabel').is_visible())
        self.page.locator('#chartOrgScope').select_option('all')
        self.assertTrue(self.page.evaluate("""()=>{
          const index=analyticsIndex(),result=departmentTrendData(index,'2026-10-19','2026-10-26',[...state.selectedOrganizations]);
          return result.data[1].get('СМУ16').amount===null && result.data[1].get('АВТО').amount===40;
        }"""))
        with self.page.expect_download() as download:
            self.page.locator('#chartOrgTotals .table-export').click()
        raw=Path(download.value.path()).read_bytes()
        self.assertTrue(raw.startswith(b'\xef\xbb\xbf'))
        rows=list(csv.reader(io.StringIO(raw.decode('utf-8-sig')),delimiter=';'))
        self.assertEqual(next(row[1] for row in rows if row and row[0]=='Показатель'),'Сравнение подразделений')
        self.assertIn('сумма и количество',next(row[1] for row in rows if row and row[0]=='Единицы'))
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='СМУ16'),['','','1 / 2'])
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='АВТО'),['60','5','2 / 2'])
        self.page.locator('#chartBreakdown').select_option('works')
        self.assertFalse(self.page.locator('#chartOrgScopeLabel').is_visible())


if __name__ == '__main__':
    unittest.main()
