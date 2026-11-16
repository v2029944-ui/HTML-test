"""Monthly calendar interactions, bounded DOM, and responsive layout."""
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright
from test_navigation import open_view

ROOT=Path(__file__).resolve().parents[1]

class MonthlyCalendar(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start()
        cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close();cls.pw.stop()

    def test_year_month_day_source_and_quality_at_320_390(self):
        for width in (320,390):
            with self.subTest(width=width):
                context=self.browser.new_context(viewport={'width':width,'height':844},is_mobile=True,has_touch=True)
                page=context.new_page();errors=[]
                page.on('pageerror',lambda error:errors.append(str(error)))
                page.route('https://calendar.local.test/index.html',lambda route:route.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
                page.goto('https://calendar.local.test/index.html');page.wait_for_function('state.db !== null')
                open_view(page,'upload')
                page.evaluate("""()=>{
                  const organizations=DEFAULT_ORGS.slice();
                  state.reports=Array.from({length:366},(_,i)=>{const start=addDays('2024-01-01',i);return {id:start,type:'day',start,end:start,organizations,rows:organizations.map(organization=>({organization,work:'Работа',count:0,amount:0})),warnings:[],issues:[],totals:[],fileName:'Год.xlsx',sheetName:'2024',amount:0,count:0};});
                  calendarYear=2024;calendarSelectedMonth=null;renderReports();renderReportCalendar();
                }""")
                self.assertEqual(page.locator('#reportCalendar>button').count(),12)
                self.assertEqual(page.locator('.calendar-day').count(),0)
                page.locator('#reportCalendar [data-month="2024-02"]').click()
                self.assertEqual(page.locator('.calendar-day').count(),29)
                page.locator('[data-date="2024-02-29"]').click()
                self.assertIn('Год.xlsx',page.locator('.calendar-day-details').inner_text())
                self.assertIn('2024',page.locator('.calendar-day-details').inner_text())
                self.assertEqual(page.locator('#reports .report-row').count(),50)
                for selector in ('#reportPeriods','#reportCalendar','.calendar-day-grid','.calendar-day-details'):
                    box=page.locator(selector).bounding_box()
                    self.assertGreaterEqual(box['x'],0)
                    self.assertLessEqual(box['x']+box['width'],width+1)
                page.locator('#calendarPrev').click()
                self.assertEqual(page.locator('#calendarRange').inner_text(),'2023 год')
                self.assertEqual(page.locator('#reportCalendar>button').count(),12)
                self.assertEqual(page.locator('.calendar-day').count(),0)
                page.locator('#calendarNext').click()
                self.assertEqual(page.locator('#calendarRange').inner_text(),'2024 год')
                page.locator('#reportCalendar [data-month="2024-02"]').click()
                page.locator('[data-date="2024-02-29"]').click()
                page.locator('.calendar-day-details button').click()
                self.assertEqual(page.locator('#qualityPeriod').input_value(),'day:2024-02-29')
                self.assertEqual(errors,[])
                context.close()
