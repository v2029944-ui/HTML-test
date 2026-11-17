"""Explicit labelled header evidence; real XLSX and existing manual/folder UI."""
import io,re,unittest,zipfile,xml.etree.ElementTree as ET
from pathlib import Path
from playwright.sync_api import sync_playwright
from test_import_controls import daily_workbook_bytes
from test_navigation import open_view
ROOT=Path(__file__).resolve().parents[1]

def dated_book(headers):
    src=zipfile.ZipFile(io.BytesIO(daily_workbook_bytes()))
    ns='{http://schemas.openxmlformats.org/spreadsheetml/2006/main}'
    sheet=ET.fromstring(src.read('xl/worksheets/sheet1.xml'));data=sheet.find(ns+'sheetData')
    for row in data:
        row.set('r',str(int(row.attrib['r'])+len(headers)))
        for cell in row:
            cell.set('r',re.sub(r'\d+$',lambda m:str(int(m[0])+len(headers)),cell.attrib['r']))
    for i,values in reversed(list(enumerate(headers,1))):
        row=ET.Element(ns+'row',r=str(i))
        for j,value in enumerate(values):
            c=ET.SubElement(row,ns+'c',r=chr(65+j)+str(i),t='inlineStr')
            ET.SubElement(ET.SubElement(c,ns+'is'),ns+'t').text=value
        data.insert(0,row)
    out=io.BytesIO()
    with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
        for name in src.namelist():z.writestr(name,ET.tostring(sheet) if name.endswith('sheet1.xml') else src.read(name))
    return out.getvalue()

class PeriodDateEvidence(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start();cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    @classmethod
    def tearDownClass(cls):cls.browser.close();cls.pw.stop()
    def setUp(self):
        self.context=self.browser.new_context();self.page=self.context.new_page();self.errors=[]
        self.page.on('pageerror',lambda e:self.errors.append(str(e)))
        self.page.route('https://date.test/',lambda r:r.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
        self.page.goto('https://date.test/');self.page.wait_for_function('state.db!==null');open_view(self.page,'upload')
    def tearDown(self):
        self.context.close();self.assertEqual(self.errors,[])
    def upload(self,payload,name='Анализ.xlsx'):
        self.page.locator('#excelFile').set_input_files({'name':name,'mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet','buffer':payload})
        self.page.wait_for_function('(name)=>state.fileName===name',arg=name)
    def confirm(self,accept):
        self.page.evaluate('(accept)=>{window.dateConfirms=0;window.confirm=async()=>{dateConfirms++;return accept;}}',accept)
    def test_explicit_choice_survives_evidence_and_new_file_drops_both(self):
        self.page.locator('#importWeek').fill('2026-11-16')
        self.upload(dated_book([['Начало периода: 09.11.2026']]))
        self.assertEqual(self.page.locator('#importWeek').input_value(),'2026-11-16')
        text=self.page.locator('#importDateEvidence').inner_text();self.assertIn('09.11.2026',text);self.assertIn('A1',text)
        self.confirm(False);self.page.get_by_role('button',name='Выбрать 09.11.2026').click()
        self.assertEqual(self.page.locator('#importWeek').input_value(),'2026-11-16')
        self.confirm(True);self.page.get_by_role('button',name='Выбрать 09.11.2026').click()
        self.page.wait_for_function("document.querySelector('#importWeek').value==='2026-11-09'")
        self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===6')
        self.assertEqual(self.page.evaluate('state.pending[0].start'),'2026-11-09')
        self.upload(daily_workbook_bytes(),'Другой.xlsx')
        self.assertEqual(self.page.locator('#importWeek').input_value(),'')
        self.assertNotIn('09.11.2026',self.page.locator('#importDateEvidence').inner_text())
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
    def test_conflicting_starts_require_confirmation_and_recheck_changed_date(self):
        self.upload(dated_book([['Начало периода: 09.11.2026'],['Начало периода: 16.11.2026']]))
        self.assertEqual(self.page.locator('#importWeek').input_value(),'')
        self.assertIn('противоречивые',self.page.locator('#importDateEvidence').inner_text())
        self.page.locator('#importWeek').fill('2026-11-09');self.confirm(False)
        self.page.locator('#inspectFile').click();self.page.wait_for_function('dateConfirms===1')
        self.assertEqual(self.page.evaluate('state.pending.length'),0);self.assertTrue(self.page.locator('#saveImport').is_disabled())
        self.confirm(True);self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===6')
        self.assertEqual(self.page.evaluate('dateConfirms'),1)
        self.page.locator('#importWeek').fill('2026-11-16');self.page.locator('#inspectFile').click()
        self.page.wait_for_function('dateConfirms===2')
    def test_month_day_and_unknown_date(self):
        for kind,header,date,value,field in [('month','Начало месяца: 2024-02','2024-02-01','2024-02','importMonth'),('day','Начало дня: 2026-11-13','2026-11-13','2026-11-13','importWeek')]:
            self.page.locator('#importType').select_option(kind)
            self.upload(dated_book([[header]]),kind+'.xlsx')
            self.assertEqual(self.page.locator('#'+field).input_value(),'')
            self.page.get_by_role('button',name='Выбрать '+('.'.join(reversed(date.split('-'))))).click()
            self.page.wait_for_function('(x)=>document.getElementById(x.field).value===x.value',arg={'field':field,'value':value})
            self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===6')
            self.assertEqual(self.page.evaluate('state.pending[0].start'),date)
        self.upload(daily_workbook_bytes(),'unknown.xlsx');self.page.locator('#inspectFile').click()
        self.page.wait_for_function("document.querySelector('#flash').textContent.includes('Укажите дату')")
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
    def test_delayed_confirmation_cannot_assign_old_date_to_another_workbook(self):
        self.upload(dated_book([['Начало периода: 09.11.2026'],['Начало периода: 16.11.2026']]))
        self.page.evaluate('()=>{window.confirm=()=>new Promise(resolve=>{window.finishDateConfirmation=resolve;});}')
        self.page.get_by_role('button',name='Выбрать 09.11.2026').click()
        self.page.wait_for_function("typeof finishDateConfirmation==='function'")
        self.upload(daily_workbook_bytes(),'new.xlsx')
        self.page.evaluate('finishDateConfirmation(true)')
        self.page.wait_for_function("document.querySelector('#flash').textContent.includes('Файл или настройки изменились')")
        self.assertEqual(self.page.locator('#importWeek').input_value(),'')
        self.assertEqual(self.page.evaluate('state.pending.length'),0)
    def folder(self,start):
        payload=dated_book([['Начало периода: 09.11.2026']])
        self.page.evaluate('''x=>{
          window.dateWrites=0;window.dateRuleWrites=0;window.dateRule={type:'week',start:x.start,sheet:''};
          window.desktopAPI={list:async()=>({folder:'C:/Test',files:[{name:'Анализ.xlsx',signature:'one',size:x.bytes.length,tooLarge:false}],rules:{'Анализ.xlsx':dateRule}}),
            info:async()=>({dataFile:'C:/Db/reports.sqlite3'}),readExcel:async()=>({bytes:x.bytes,signature:'one',sha256:'hash'}),checkExcel:async()=>true,
            save:async data=>{dateWrites++;window.dateSaved=data;},setRule:async()=>{dateRuleWrites++;},onFilesChanged:()=>()=>{}};
          state.reports=[{id:'kept',type:'day',start:'2025-01-01',end:'2025-01-01',fileName:'kept.xlsx',organizations:['СМУ16'],rows:[{organization:'СМУ16',work:'Keep',count:1,amount:1}],warnings:[],issues:[],totals:[],count:1,amount:1}];
        }''',{'start':start,'bytes':list(payload)})
        self.page.add_script_tag(content=(ROOT/'desktop/renderer.js').read_text())
    def test_folder_contradiction_preserves_rule_and_reports(self):
        self.folder('2026-10-19')
        self.page.wait_for_function("document.querySelector('#upload').textContent.includes('Дата правила не согласуется')")
        self.assertEqual(self.page.evaluate('[dateWrites,dateRuleWrites,state.reports.length]'),[0,0,1])
        self.assertEqual(self.page.evaluate('dateRule.start'),'2026-10-19')
        self.assertIn('A1',self.page.locator('#upload').inner_text())
        self.confirm(False);self.page.get_by_role('button',name='Проверить и сохранить вручную').click()
        self.page.wait_for_function('dateConfirms===1')
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
        self.assertEqual(self.page.evaluate('dateWrites'),0)
    def test_folder_agreement_shows_source_and_keeps_other_reports(self):
        self.folder('2026-11-09');self.page.wait_for_function('dateWrites===1')
        self.assertEqual(self.page.evaluate('dateSaved.reports.length'),7)
        self.assertEqual(self.page.evaluate('dateRuleWrites'),0)
        self.assertIn('подтверждена заголовком Дневные!A1',self.page.locator('#upload').inner_text())

if __name__=='__main__':unittest.main()
