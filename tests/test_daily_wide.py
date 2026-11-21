"""Synthetic real XLSX fixtures for shared daily-column import; no Excel dependencies."""
import io,zipfile,datetime,json,unittest
from pathlib import Path
from xml.sax.saxutils import escape
from playwright.sync_api import sync_playwright
from test_navigation import open_view
ROOT=Path(__file__).resolve().parents[1]

def wide_xlsx(year=2024,quarter_sheets=False,short=False,include_year=True,serial=False,blank_day=None,works_per_org=1):
    months=[(1,4),(4,7),(7,10),(10,13)] if quarter_sheets else [(1,13)]
    files={}
    for idx,(first,last) in enumerate(months,1):
        begin=datetime.date(year,first,1)
        end=datetime.date(year+1,1,1) if last==13 else datetime.date(year,last,1)
        dates=[begin+datetime.timedelta(days=i) for i in range((end-begin).days)]
        rows=[['Служебный заголовок'] for _ in range(16)]
        rows.append(['Годовой отчёт '+str(year) if include_year else 'Отчёт'])
        date_row=['Работа'];metric_row=['']
        for date in dates:
            value=(date-datetime.date(1899,12,30)).days if serial else date.strftime('%d.%m') if short else date.isoformat()
            date_row.extend([value,None]);metric_row.extend(['Количество','Сумма'])
        date_row.extend(['Итого за период',None]);metric_row.extend(['Количество','Сумма']);rows.extend([date_row,metric_row])
        for org in ['СМУ16','АВТО','ТРАК','ЦЕНТР']:
            for name in [org]+['Альфа' if works_per_org==1 else 'Альфа '+str(i+1) for i in range(works_per_org)]:
                row=[name]
                factor=works_per_org if name==org else 1
                for date in dates:row.extend([None,None] if date.isoformat()==blank_day else [.5*factor,5*factor])
                available=len(dates)-(1 if blank_day in [d.isoformat() for d in dates] else 0)
                row.extend([.5*factor*available,5*factor*available]);rows.append(row)
        overall=['Итого']
        for date in dates:overall.extend([None,None] if date.isoformat()==blank_day else [2*works_per_org,20*works_per_org])
        overall.extend([2*works_per_org*available,20*works_per_org*available]);rows.append(overall)
        def col(n):
            result=''
            while n:n,r=divmod(n-1,26);result=chr(65+r)+result
            return result
        xml=[]
        for ri,row in enumerate(rows,1):
            cells=[]
            for ci,value in enumerate(row,1):
                if value is None:continue
                ref=col(ci)+str(ri)
                cells.append(f'<c r="{ref}" t="inlineStr"><is><t>{escape(value)}</t></is></c>' if isinstance(value,str) else f'<c r="{ref}"><v>{value}</v></c>')
            xml.append(f'<row r="{ri}">'+''.join(cells)+'</row>')
        merges=['A18:A19']+[col(i*2+2)+'18:'+col(i*2+3)+'18' for i in range(len(dates)+1)]
        files[f'xl/worksheets/sheet{idx}.xml']='<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'+''.join(xml)+'</sheetData><mergeCells>'+''.join('<mergeCell ref="'+ref+'"/>' for ref in merges)+'</mergeCells></worksheet>'
    files['[Content_Types].xml']='<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'+''.join(f'<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' for i in range(1,len(months)+1))+'</Types>'
    files['_rels/.rels']='<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>'
    files['xl/workbook.xml']='<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'+''.join(f'<sheet name="'+(f'Q{i}' if quarter_sheets else 'Лист')+f'" sheetId="{i}" r:id="rId{i}"/>' for i in range(1,len(months)+1))+'</sheets></workbook>'
    files['xl/_rels/workbook.xml.rels']='<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'+''.join(f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>' for i in range(1,len(months)+1))+'</Relationships>'
    buffer=io.BytesIO()
    with zipfile.ZipFile(buffer,'w',zipfile.ZIP_DEFLATED) as result:
        for name,value in files.items():result.writestr(name,value)
    return buffer.getvalue()

class DailyWideImport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw=sync_playwright().start();cls.browser=cls.pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    @classmethod
    def tearDownClass(cls):cls.browser.close();cls.pw.stop()
    def setUp(self):
        self.context=self.browser.new_context(viewport={'width':390,'height':844});self.page=self.context.new_page();self.errors=[]
        self.page.on('pageerror',lambda err:self.errors.append(str(err)))
        self.page.route('https://wide.test/index.html',lambda route:route.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()))
        self.page.goto('https://wide.test/index.html');self.page.wait_for_function('state.db!==null');open_view(self.page,'upload')
    def tearDown(self):self.context.close();self.assertEqual(self.errors,[])
    def upload(self,payload):
        self.page.locator('#excelFile').set_input_files({'name':'Синтетический.xlsx','mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet','buffer':payload})
        self.page.wait_for_function('state.workbook!==null')
    def test_complete_years_and_four_quarter_sheets_are_real_xlsx(self):
        for year,quarters,serial in [(2025,False,False),(2024,False,True),(2024,True,False)]:
            with self.subTest(year=year,quarters=quarters):
                self.upload(wide_xlsx(year,quarters,serial=serial));self.assertEqual(self.page.locator('#importFormat').input_value(),'daily-wide')
                self.page.locator('#inspectFile').click();days=366 if year%4==0 else 365
                self.page.wait_for_function(f'state.pending.length==={days}')
                self.assertTrue(self.page.evaluate('state.pending.every(r=>r.type==="day"&&r.rows.length===4&&r.warnings.length===0)'))
                self.assertEqual(self.page.evaluate('state.pending.reduce((s,r)=>s+r.amount,0)'),days*20)
                self.assertLessEqual(self.page.locator('#importPreview .import-preview-card').count(),31)
        self.page.locator('#saveImport').click();self.page.wait_for_function('state.reports.length===366')
        self.page.reload();self.page.wait_for_function('state.reports.length===366')
        open_view(self.page,'upload');self.assertEqual(self.page.locator('#reportCalendar>button').count(),12)
        self.page.locator('[data-month="2024-02"]').click();self.assertEqual(self.page.locator('.calendar-day').count(),29)
        self.assertLessEqual(self.page.locator('#reports .report-row').count(),50)
    def test_large_year_real_xlsx_has_500_services_and_bounded_preview(self):
        import time
        payload=wide_xlsx(2024,works_per_org=125)
        started=time.monotonic();self.upload(payload);self.page.locator('#inspectFile').click()
        self.page.wait_for_function('state.pending.length===366',timeout=60000)
        elapsed=time.monotonic()-started
        self.assertEqual(self.page.evaluate('state.pending.reduce((n,r)=>n+r.rows.length,0)'),183000)
        self.assertEqual(self.page.evaluate('state.pending.reduce((n,r)=>n+r.amount,0)'),915000)
        self.assertEqual(self.page.evaluate('state.pending.reduce((n,r)=>n+r.warnings.length,0)'),0)
        self.assertLessEqual(self.page.locator('#importPreview .import-preview-card').count(),31)
        self.assertFalse(self.page.locator('#saveImport').is_disabled())
        print(f'Synthetic annual XLSX: {len(payload)} compressed bytes, 500 services/day, 366 days, preview {elapsed:.2f}s',flush=True)

    def test_ambiguous_year_and_blank_day_are_not_silent_zeroes(self):
        self.upload(wide_xlsx(2024,short=True,include_year=False,blank_day='2024-02-05'))
        self.page.locator('#inspectFile').click();self.page.wait_for_function("document.querySelector('#flash').textContent.includes('Год не определён')")
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
        self.page.locator('#dailyYear').fill('2024');self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===365')
        self.assertTrue(self.page.evaluate("!state.pending.some(r=>r.start==='2024-02-05')"))
        self.assertTrue(self.page.evaluate("state.pending.some(r=>r.issues.some(i=>i.type==='missing-day'))"))
        self.page.once('dialog',lambda dialog:dialog.dismiss());self.page.locator('#saveImport').click();self.assertEqual(self.page.evaluate('state.reports.length'),0)
    def test_changed_workbook_must_keep_every_sheet_in_the_saved_mapping(self):
        self.upload(wide_xlsx(2024))
        outcome=self.page.evaluate("""()=>{
          const mappings={'Старое имя':{workCol:0,headerRow:18,firstDayCol:1}};
          try{return parseDailyWorkbook(state.workbook,{sheets:[],mappings}).reports.length;}
          catch(error){return error.message;}
        }""")
        self.assertIsInstance(outcome,str)
        self.assertIn('Старое имя',outcome)
        self.assertEqual(self.page.evaluate('state.reports.length'),0)

    def test_folder_wide_sync_real_parser_and_labeled_fields(self):
        self.page.evaluate('''payload=>{
          window.wideBytes=payload;window.signature='1';window.writes=0;
          window.rule={type:'day',start:'',sheet:'',format:'daily-wide',year:2024,sheets:[],mappings:{}};
          window.desktopAPI={list:async()=>({folder:'C:/Reports',files:[{name:'Год.xlsx',size:payload.length,signature,tooLarge:false}],rules:{'Год.xlsx':rule}}),info:async()=>({dataFile:'C:/Db/reports.sqlite3'}),
            readExcel:async()=>({bytes:wideBytes,signature,sha256:'hash'}),checkExcel:async(name,value)=>value===signature,
            save:async data=>{writes++;window.saved=structuredClone(data)},onFilesChanged:()=>()=>{},setRule:async(name,value)=>{rule=value;return value}};
        }''',list(wide_xlsx(2024)))
        self.page.add_script_tag(content=(ROOT/'tauri/folder-importer.js').read_text());self.page.wait_for_function('writes===1&&state.reports.length===366')
        for width in [320,390,1280]:
            self.page.set_viewport_size({'width':width,'height':900})
            self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
            row=self.page.locator('.desktop-file-row');self.assertGreater(row.locator('label>span').count(),3)
        self.page.get_by_role('button',name='Проверить и сохранить вручную',exact=True).click();self.page.wait_for_function('state.pending.length===366')
        self.assertEqual(self.page.locator('#importFormat').input_value(),'daily-wide');self.assertEqual(self.page.locator('#dailyYear').input_value(),'2024')
        self.assertEqual(self.page.evaluate('writes'),1)
        self.page.get_by_role('button',name='Закрыть ручную проверку',exact=True).click()
        self.page.evaluate('signature="2";wideBytes=[1,2,3]');self.page.get_by_role('button',name='Проверить файлы сейчас',exact=True).click()
        self.page.wait_for_function('!state.desktopSyncing');self.assertEqual(self.page.evaluate('writes'),1);self.assertEqual(self.page.evaluate('state.reports.length'),366)
        self.page.evaluate('payload=>{wideBytes=payload;signature="3"}',list(wide_xlsx(2024,blank_day='2024-02-05')))
        self.page.get_by_role('button',name='Проверить файлы сейчас',exact=True).click();self.page.wait_for_function('!state.desktopSyncing')
        self.assertEqual(self.page.evaluate('writes'),1);self.assertEqual(self.page.evaluate('state.reports.length'),366)
        self.page.get_by_role('button',name='Проверить и сохранить вручную',exact=True).click();self.page.wait_for_function('state.pending.length===365')
        self.page.locator('#replaceExisting').check();self.page.once('dialog',lambda dialog:dialog.accept());self.page.locator('#saveImport').click()
        self.page.wait_for_function('writes===2&&state.folderImport===null')
        self.assertEqual(self.page.evaluate('state.reports.length'),365)
        self.assertFalse(self.page.evaluate("state.reports.some(r=>r.start==='2024-02-05')"))

    def test_explicit_legacy_format_in_mixed_workbook_and_manual_preview(self):
        self.page.evaluate('''payload=>{
          const read=XLSX.read;
          XLSX.read=(...args)=>{const book=read(...args);book.SheetNames.push('Legacy');book.Sheets.Legacy=[['Работа','Количество','Сумма'],['СМУ16',2,20],['Альфа',2,20]];return book;};
          window.rule={format:'legacy',type:'week',start:'2024-01-01',sheet:'Legacy'};window.writes=0;
          window.desktopAPI={list:async()=>({folder:'C:/Reports',files:[{name:'mixed.xlsx',size:payload.length,signature:'1',tooLarge:false}],rules:{'mixed.xlsx':rule}}),info:async()=>({dataFile:'C:/Db/reports.sqlite3'}),
            readExcel:async()=>({bytes:payload,signature:'1',sha256:'hash'}),checkExcel:async()=>true,
            save:async()=>{writes++},onFilesChanged:()=>()=>{},setRule:async(name,value)=>{rule=value;return value}};
        }''',list(wide_xlsx(2024)))
        self.page.add_script_tag(content=(ROOT/'tauri/folder-importer.js').read_text())
        self.page.wait_for_function('writes===1&&state.reports.length===1')
        self.assertEqual(self.page.evaluate('state.reports[0].type'),'week')
        self.assertEqual(self.page.evaluate('state.reports[0].sheetName'),'Legacy')
        self.page.get_by_role('button',name='Проверить и сохранить вручную',exact=True).click()
        self.page.wait_for_function('state.pending.length===1')
        self.assertEqual(self.page.locator('#importFormat').input_value(),'legacy')
        self.assertTrue(self.page.locator('#importType').is_visible())
        self.assertTrue(self.page.locator('#sheetSelect').is_visible())
        self.assertEqual(self.page.locator('#sheetSelect').input_value(),'Legacy')
