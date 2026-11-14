"""Local Chromium checks; run python -m unittest discover -s tests -v. No server/Actions."""
import csv
import io
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


class ImportControls(unittest.TestCase):
    def load_analytics_fixture(self):
        self.page.evaluate('''async()=>{
          const orgs=DEFAULT_ORGS.slice(0,2),works=Array.from({length:10},(_,i)=>'Работа '+(i+1));
          const reports=['2026-10-19','2026-10-26'].map((start,i)=>({id:start,type:'week',start,end:periodEnd(start,'week'),organizations:i?[orgs[0]]:orgs,
            warnings:i?['Замечание импорта']:[],issues:i?[{reason:'Замечание импорта'}]:[],totals:[],
            rows:(i?[orgs[0]]:orgs).flatMap(organization=>works.map((work,j)=>({organization,work,count:j+0.5,amount:(j+1)*100})))}));
          await storeReports(reports);state.settings.groups=[];state.selectionInitialized=false;renderAll();
          state.selectedOrganizations=new Set(orgs);state.selectedCategories=new Set(works);
          document.querySelector('#overviewMode').value='range';document.querySelector('#rangeStart').value=reports[0].start;document.querySelector('#rangeEnd').value=reports[1].start;
          document.querySelector('#advancedAnalytics').open=true;document.querySelector('#chartBreakdown').value='works';renderDashboard(false);
        }''')

    def test_stable_colors_across_filters_chart_types_and_department_views(self):
        self.load_analytics_fixture()
        result=self.page.evaluate('''()=>{
          const root=document.createElement('div');document.body.append(root);
          const period=state.reports[0],orgs=DEFAULT_ORGS.slice(0,2),data=[new Map(orgs.map(org=>[org,{amount:100}]))];
          const read=()=>Object.fromEntries([...root.querySelectorAll('.chart-legend span')].map(node=>[node.textContent,node.style.getPropertyValue('--series')]));
          const cases=[];
          for(const chart of ['line','bar']){
            for(const categories of [orgs,[orgs[1]],[orgs[1],orgs[0]]]){
              drawChart([period],new Set(categories),'amount',root,{chart,seriesType:'organizations',categories,data});
              cases.push([...root.querySelectorAll('.chart-legend span')].every(node=>node.style.getPropertyValue('--series')===departmentSeriesColor(node.textContent)));
              cases.push([...root.querySelectorAll('[data-chart-tooltip]')].every(node=>node.getAttribute('fill')===departmentSeriesColor(node.dataset.chartTooltip.split(' · ')[0])));
            }
          }
          drawChart([period],new Set(orgs),'amount',root,{categories:['Работа 1','Работа 2']});const before=read();
          drawChart([period],new Set(orgs),'amount',root,{categories:['Работа 2']});cases.push(before['Работа 2']===read()['Работа 2']);
          root.remove();return cases;
        }''')
        self.assertTrue(all(result),result)
        self.page.locator('#chartBreakdown').select_option('organizations')
        self.page.locator('#chartSeriesLayout').select_option('separate')
        self.assertTrue(self.page.evaluate('''()=>[...document.querySelectorAll('#chartsContainer .chart-legend span')].every(node=>node.style.getPropertyValue('--series')===departmentSeriesColor(node.textContent))'''))

    def test_partial_calendar_neutral_changes_and_average_explanation(self):
        self.load_analytics_fixture()
        result=self.page.evaluate('''()=>{
          const root=document.querySelector('#chartsContainer');
          const calendar=calendarReportStatus(state.reports,state.reports[1],DEFAULT_ORGS.slice(0,2));
          return {text:root.textContent,marks:root.querySelectorAll('[data-chart-tooltip]').length,calendar:calendar.label,
            up:overviewTrendClass(2,1),down:overviewTrendClass(1,2),high:usualStatusClass('high'),low:usualStatusClass('low'),
            average:document.querySelector('.overview-footnote').textContent};
        }''')
        self.assertIn('Неполные данные: 1 из 2 подразделений',result['text'])
        self.assertNotIn('Нет отчёта',result['text'])
        self.assertEqual(result['marks'],8)
        self.assertEqual(result['calendar'],'Неполный · 1 замечание')
        for key in ['up','down','high','low']:self.assertEqual(result[key],'numeric-change')
        self.assertIn('сопоставимых единицах',result['average'])

    def test_series_pages_are_accessible_and_fit_mobile_and_desktop(self):
        self.load_analytics_fixture()
        for width in [320,390,1024,1440]:
            self.page.set_viewport_size({'width':width,'height':900})
            for theme in ['light','dark']:
                self.page.evaluate('(theme)=>document.documentElement.dataset.theme=theme',theme)
                control=self.page.locator('#chartsContainer .chart-series-control').first
                select=control.locator('select')
                select.select_option('0')
                self.assertIn('Отображается 8 из 10',control.inner_text())
                select.select_option('1')
                self.assertIn('Отображается 2 из 10',control.inner_text())
                legend=self.page.locator('#chartsContainer .chart-legend').first
                self.assertEqual(legend.locator('span').all_text_contents(),['Работа 9','Работа 10'])
                self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
                box=control.evaluate('(node)=>{const box=node.getBoundingClientRect();return {x:box.x,width:box.width};}')
                self.assertGreaterEqual(box['x'],0)
                self.assertLessEqual(box['x']+box['width'],width+1)
                self.assertEqual(select.evaluate('(node)=>document.activeElement===node'),True)
                self.assertIn('10',self.page.locator('#categoryCount').inner_text())
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
          await storeReports(reports);state.settings.groups=[];state.selectionInitialized=false;state.selectedMetrics=new Set(['count','amount']);state.chartMetrics=new Set(['count','amount']);renderAll();
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
        units=next(row[1] for row in rows if row and row[0]=='Единицы')
        self.assertIn('суммы',units)
        self.assertIn('количества',units)
        headers=next(row for row in rows if row and row[0]=='Подразделение')
        self.assertEqual(len(headers),4)
        self.assertIn('Сумма',headers[1])
        self.assertIn('Количество',headers[2])
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='СМУ16'),['','','1 / 2'])
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='АВТО'),['60','5','2 / 2'])
        self.page.locator('#chartBreakdown').select_option('works')
        self.assertFalse(self.page.locator('#chartOrgScopeLabel').is_visible())


if __name__ == '__main__':
    unittest.main()
