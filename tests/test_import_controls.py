"""Local Chromium checks; run python -m unittest discover -s tests -v. No server/Actions."""
import csv
import io
import json
import zipfile
from xml.sax.saxutils import escape
import unittest
from pathlib import Path
from playwright.sync_api import sync_playwright
from test_navigation import open_view

ROOT = Path(__file__).resolve().parents[1]


def daily_workbook_bytes():
    matrix=[['Работа','Количество','Сумма']*6]
    for name,factor in [('СМУ16',0.5),('Альфа',0.5),('АВТО',1.5),('Бета',1.5),('Итого',2)]:
        row=[]
        for k in range(1,7):
            amount=(30 if name=='Итого' else 10 if factor==0.5 else 20)*k*k
            row.extend([name,factor*k,amount])
        matrix.append(row)
    def column(n):
        result=''
        while n:n,r=divmod(n-1,26);result=chr(65+r)+result
        return result
    rows=[]
    for i,row in enumerate(matrix,1):
        cells=[]
        for j,value in enumerate(row,1):
            ref=column(j)+str(i)
            cells.append(f'<c r="{ref}" t="inlineStr"><is><t>{escape(value)}</t></is></c>' if isinstance(value,str) else f'<c r="{ref}"><v>{value}</v></c>')
        rows.append(f'<row r="{i}">'+''.join(cells)+'</row>')
    files={
        '[Content_Types].xml':'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
        '_rels/.rels':'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        'xl/workbook.xml':'<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Дневные" sheetId="1" r:id="rId1"/></sheets></workbook>',
        'xl/_rels/workbook.xml.rels':'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
        'xl/worksheets/sheet1.xml':'<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'+''.join(rows)+'</sheetData></worksheet>'
    }
    buffer=io.BytesIO()
    with zipfile.ZipFile(buffer,'w',zipfile.ZIP_STORED) as workbook:
        for name,value in files.items():workbook.writestr(name,value)
    return buffer.getvalue()


class ImportControls(unittest.TestCase):
    def test_week_start_is_not_invented_from_analyz_filename(self):
        open_view(self.page,'upload')
        self.assertEqual(self.page.locator('#importType').input_value(),'week')
        self.assertEqual(self.page.locator('#importWeek').input_value(),'')
        self.page.locator('#excelFile').set_input_files({
            'name':'Анализ.xlsx',
            'mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            'buffer':daily_workbook_bytes()
        })
        self.page.wait_for_function('state.workbook!==null')
        self.assertEqual(self.page.locator('#importWeek').input_value(),'')
        self.assertNotIn('19.10.2026', self.page.locator('#upload').inner_text())
        self.page.locator('#inspectFile').click()
        self.page.wait_for_function("document.querySelector('#flash').textContent.includes('Укажите дату начала первой недели')")
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
        # A date explicitly entered by the user must not be overwritten when choosing another file.
        self.page.locator('#importWeek').fill('2026-11-09')
        self.page.locator('#excelFile').set_input_files({
            'name':'Другой.xlsx',
            'mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            'buffer':daily_workbook_bytes()
        })
        self.page.wait_for_function("state.fileName==='Другой.xlsx'")
        self.assertEqual(self.page.locator('#importWeek').input_value(),'2026-11-09')

    def test_saved_legacy_week_rule_requires_review_not_silent_reset(self):
        self.page.evaluate("""()=>{window.desktopAPI={
          list:async()=>({
            folder:'C:/Reports',
            files:[{name:'Анализ.xlsx',size:1000,signature:'old',tooLarge:false}],
            rules:{'Анализ.xlsx':{type:'week',start:'2026-10-19',sheet:''}}
          }),
          info:async()=>({dataFile:'C:/Documents/Анализ работ/reports.sqlite3'}),
          readExcel:async()=>{throw Error('Source unavailable in test')},
          onFilesChanged:()=>()=>{}
        }}""")
        self.page.add_script_tag(content=(ROOT/'desktop/renderer.js').read_text())
        self.page.wait_for_function("document.querySelector('.desktop-file-row')?.textContent.includes('Проверьте начало первой недели')")
        row=self.page.locator('.desktop-file-row')
        self.assertIn('старые версии автоматически предлагали 19.10.2026',row.inner_text())
        self.assertEqual(row.locator('input[aria-label="Первый период файла"]').input_value(),'2026-10-19')
        self.assertEqual(self.page.evaluate("state.reports.length"),0)

    def test_folder_manual_import_keeps_auto_rejection_and_checks_source(self):
        good=daily_workbook_bytes()
        original=zipfile.ZipFile(io.BytesIO(good))
        buffer=io.BytesIO()
        with zipfile.ZipFile(buffer,'w',zipfile.ZIP_STORED) as result:
            for name in original.namelist():
                data=original.read(name)
                if name.endswith('sheet1.xml'):
                    data=data.replace(b'<c r="B3"><v>0.5</v></c>',b'<c r="B3"/>')
                result.writestr(name,data)
        self.page.evaluate("""payload=>{
          window.folderBytes=payload;window.folderSignature='one';window.nativeWrites=0;
          window.nativeRule={type:'day',start:'2026-11-10',sheet:''};
          window.desktopAPI={
            list:async()=>({folder:'C:/Test',files:[{name:'daily.xlsx',size:payload.length,signature:folderSignature,tooLarge:false}],rules:{'daily.xlsx':nativeRule}}),
            info:async()=>({dataFile:'C:/Db/reports.sqlite3'}),
            readExcel:async()=>({bytes:folderBytes,signature:folderSignature,sha256:'hash'}),
            checkExcel:async(name,signature)=>signature===folderSignature,
            save:async data=>{window.nativeWrites++;window.savedNative=structuredClone(data);},
            onFilesChanged:()=>()=>{}
          };
        }""",list(buffer.getvalue()))
        self.page.add_script_tag(content=(ROOT/'desktop/renderer.js').read_text())
        open_view(self.page,'upload')
        self.page.wait_for_function("document.querySelector('.desktop-file-row')&&document.querySelector('#upload').textContent.includes('Есть замечания к Excel')")
        self.assertEqual(self.page.evaluate('nativeWrites'),0)
        self.page.get_by_role('button',name='Проверить и сохранить вручную').click()
        self.page.wait_for_function('state.pending.length===6')
        self.assertIn('daily.xlsx',self.page.locator('#excelFileStatus').inner_text())
        self.assertGreater(self.page.evaluate('state.pending[0].warnings.length'),0)
        self.assertEqual(self.page.evaluate('nativeWrites'),0)
        self.page.evaluate('window.confirm=async()=>false')
        self.page.locator('#saveImport').click()
        self.page.wait_for_timeout(100)
        self.assertEqual(self.page.evaluate('nativeWrites'),0)
        self.assertEqual(self.page.evaluate('state.reports.length'),0)
        self.page.evaluate("window.confirm=async()=>true;folderSignature='changed'")
        self.page.locator('#saveImport').click()
        self.page.wait_for_function("document.querySelector('#flash').textContent.includes('Файл, папка или правило')")
        self.assertEqual(self.page.evaluate('nativeWrites'),0)
        self.page.evaluate("folderSignature='one'")
        self.page.locator('#saveImport').click()
        self.page.wait_for_function('nativeWrites===1&&state.folderImport===null')
        self.assertEqual(self.page.evaluate('savedNative.reports.length'),6)
        self.assertEqual(self.page.evaluate('savedNative.reports[0].desktopSource'),'daily.xlsx')
        self.assertGreater(self.page.evaluate('savedNative.reports[0].warnings.length'),0)
        self.page.get_by_role('button',name='Проверить файлы сейчас',exact=True).click()
        self.page.wait_for_function("!state.desktopSyncing&&document.querySelector('#upload').textContent.includes('Есть замечания к Excel')")
        self.assertEqual(self.page.evaluate('nativeWrites'),1)
        self.page.evaluate("bytes=>{folderBytes=bytes;folderSignature='two'}",list(good))
        self.page.get_by_role('button',name='Проверить файлы сейчас',exact=True).click()
        self.page.wait_for_function('nativeWrites===2&&!state.desktopSyncing')
        self.assertEqual(self.page.evaluate('state.reports.length'),6)
        self.assertEqual(self.page.evaluate('state.reports[0].warnings.length'),0)
        self.assertEqual(self.page.evaluate('state.reports[0].desktopSignature'),'two')

    def test_tauri_update_controls_wait_for_confirmation(self):
        self.page.evaluate("""()=>{
          window.updateDownloads=0;window.confirm=async()=>false;
          window.__TAURI__={core:{invoke:async()=>true},updater:{check:async()=>({
            version:'0.2.0',currentVersion:'0.1.0',close:async()=>{},
            downloadAndInstall:async callback=>{updateDownloads++;callback({event:'Finished'});}
          })}};
        }""")
        self.page.add_script_tag(content=(ROOT/'tauri/updates.js').read_text())
        open_view(self.page,'storage')
        self.page.wait_for_function("document.querySelector('#appUpdateStatus').textContent.includes('0.2.0')")
        for width in [390,1280]:
            self.page.set_viewport_size({'width':width,'height':900})
            self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
        self.page.locator('#installAppUpdate').click()
        self.page.wait_for_timeout(100)
        self.assertEqual(self.page.evaluate('updateDownloads'),0)
        self.page.evaluate('window.confirm=async()=>true')
        self.page.locator('#installAppUpdate').click()
        self.page.wait_for_function('updateDownloads===1&&!state.appUpdating')
        self.assertFalse(self.page.locator('main').evaluate('node=>node.inert'))

    def test_local_csp_preserves_inline_app_and_xlsx_import(self):
        import hashlib,base64,re
        html=(ROOT/'index.html').read_text()
        config=json.loads((ROOT/'tauri/src-tauri/tauri.conf.json').read_text())
        policy=config['app']['security']['csp']
        hashes=['\'sha256-'+base64.b64encode(hashlib.sha256(script.encode()).digest()).decode()+'\'' for script in re.findall(r'<script>([\s\S]*?)</script>',html)]
        policy=policy.replace("script-src 'self'","script-src 'self' "+' '.join(hashes))
        self.page.route('https://html.local.test/index.html',lambda route:route.fulfill(content_type='text/html',body=html,headers={'Content-Security-Policy':policy}))
        self.page.reload();self.page.locator('#storageModeInfo').filter(has_text='IndexedDB').wait_for(state='attached')
        open_view(self.page,'upload')
        self.page.locator('#excelFile').set_input_files({'name':'daily.xlsx','mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet','buffer':daily_workbook_bytes()})
        self.page.locator('#excelFileStatus').filter(has_text='Файл загружен').wait_for()
        self.page.locator('#importType').select_option('day');self.page.locator('#importWeek').fill('2026-11-10')
        self.page.locator('#inspectFile').click();self.page.locator('#importPreview strong').filter(has_text='Найдено отчётных периодов: 6').wait_for()
        self.page.locator('#saveImport').click();self.page.locator('#reportCount').filter(has_text='6').wait_for(state='attached')
        open_view(self.page,'dashboard')
        self.page.locator('#periodType').select_option('day')
        self.assertGreater(self.page.locator('svg').count(),0)

    def test_comparison_and_calendar_filters_are_below_their_headings(self):
        self.load_six_week_fixture()
        for width in [320,390,1280]:
            self.page.set_viewport_size({'width':width,'height':900})
            for mode in ['periods','organizations']:
                self.page.locator('#comparisonMode').select_option(mode)
                header=self.page.locator('#comparison>.sectionbar').bounding_box()
                field=self.page.locator('#comparisonMode').bounding_box()
                self.assertGreaterEqual(field['y'],header['y']+header['height'])
                self.assertAlmostEqual(field['x'],header['x'],delta=1)
                self.assertEqual(self.page.locator('#comparison>.sectionbar select').count(),0)
                if mode=='organizations':
                    scope=self.page.locator('#comparisonScope').bounding_box()
                    period=self.page.locator('#comparisonPeriod').bounding_box()
                    self.assertGreaterEqual(period['y'],scope['y']+scope['height'])
            open_view(self.page,'upload')
            header=self.page.locator('#reportPeriods>.sectionbar').bounding_box()
            field=self.page.locator('#calendarType').bounding_box()
            self.assertGreaterEqual(field['y'],header['y']+header['height'])
            self.assertAlmostEqual(field['x'],header['x'],delta=1)
            self.assertEqual(self.page.locator('#reportPeriods>.sectionbar select').count(),0)
            self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
            open_view(self.page,'dashboard')

    def test_daily_excel_import_comparison_csv_and_mixed_backup(self):
        open_view(self.page,'upload');self.page.locator('#importType').select_option('day')
        self.assertEqual(self.page.locator('#importDateCaption').inner_text(),'Первый день')
        self.page.locator('#importWeek').fill('2026-11-10')
        self.page.locator('#excelFile').set_input_files({'name':'Дневные.xlsx','mimeType':'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet','buffer':daily_workbook_bytes()})
        self.page.wait_for_function('state.workbook!==null')
        self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===6')
        self.assertEqual(self.page.evaluate('state.pending.map(report=>[report.type,report.start,report.end])'),[['day',f'2026-11-{i}',f'2026-11-{i}'] for i in range(10,16)])
        self.assertTrue(self.page.evaluate('state.pending.every(report=>report.warnings.length===0)'))
        self.page.locator('#importPeriodDates').evaluate('(node)=>node.closest("details").open=true')
        second=self.page.locator('#importPeriodDates input').nth(1)
        second.fill('2026-11-10');self.page.locator('#inspectFile').click()
        self.page.wait_for_function('document.querySelector("#flash").textContent.includes("Даты наборов пересекаются")')
        self.assertIn('Даты наборов пересекаются',self.page.locator('#flash').inner_text())
        self.assertTrue(self.page.locator('#saveImport').is_disabled())
        second.fill('2026-11-11');self.page.locator('#inspectFile').click();self.page.wait_for_function('state.pending.length===6')
        self.page.locator('#saveImport').click();self.page.wait_for_function('state.reports.length===6')
        open_view(self.page,'dashboard');self.page.locator('#periodType').select_option('day')
        self.page.evaluate("state.selectedOrganizations=new Set(DEFAULT_ORGS.slice(0,2));state.selectedMetrics=new Set(['amount','count']);document.querySelector('#advancedAnalytics').open=true;renderDashboard(false)")
        self.page.locator('#periodComparisonScope').select_option('range')
        self.assertEqual(self.page.locator('#comparisonCoverage').inner_text().count('3 дня'),2)
        self.assertEqual(self.page.evaluate('''()=>{const {before,after}=periodComparisonRanges(analyticsIndex(),[...state.selectedOrganizations],state.selectedCategories);return [before.total.count,before.total.amount,after.total.count,after.total.amount];}'''),[12,420,30,2310])
        axis=self.page.locator('#chartsContainer svg text').evaluate_all('(nodes)=>nodes.map(node=>[...node.childNodes].filter(child=>child.nodeType===Node.TEXT_NODE).map(child=>child.textContent).join(""))')
        self.assertIn('15.11',axis);self.assertNotIn('2026-11',axis)
        amount=self.page.locator('#comparisonTable .metric-table').filter(has=self.page.get_by_role('heading',name='Сумма',exact=True))
        with self.page.expect_download() as download:amount.locator('.table-export').click()
        rows=list(csv.reader(io.StringIO(Path(download.value.path()).read_text(encoding='utf-8-sig')),delimiter=';'))
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='Итого по выбранным работам'),['420','2310','1890','450'])
        self.page.evaluate('''async()=>{
          const organizations=DEFAULT_ORGS.slice(0,2),make=(type,start,amount)=>({id:type,type,start,end:periodEnd(start,type),organizations,totals:[],warnings:[],rows:organizations.map(organization=>({organization,work:'Старый отчёт',count:1,amount}))});
          await storeReports([...state.reports,make('week','2026-11-09',9999),make('month','2026-11-01',8888)]);renderAll();
        }''')
        self.assertEqual(self.page.evaluate('aggregateReportRange(analyticsIndex(),{from:"2026-11-10",to:"2026-11-15",organizations:DEFAULT_ORGS.slice(0,2)}).total.amount'),2730)
        open_view(self.page,'storage')
        with self.page.expect_download() as download:self.page.locator('#exportBackup').click()
        backup=Path(download.value.path()).read_bytes();data=json.loads(backup)
        self.assertEqual(data['version'],1);self.assertEqual({report['type'] for report in data['reports']},{'day','week','month'})
        self.page.evaluate('async()=>{await storeReports([]);renderAll()}')
        self.page.once('dialog',lambda dialog:dialog.accept())
        self.page.locator('#importBackup').set_input_files({'name':'backup.json','mimeType':'application/json','buffer':backup})
        self.page.wait_for_function('state.reports.length===8');self.page.reload();self.page.wait_for_function('state.reports.length===8')
        self.assertEqual(self.page.evaluate('state.reports.filter(report=>report.type==="day").length'),6)
        self.assertTrue(self.page.evaluate('state.reports.filter(report=>report.type==="day").every(report=>report.start===report.end)'))
        self.assertEqual(self.page.evaluate('state.db.version'),1)
        totals=self.page.evaluate('''()=>{
          const organizations=DEFAULT_ORGS.slice(0,2);
          return ['day','week','month'].map(type=>{const index=buildReportIndex(state.reports,[],{type,organizations}),start=type==='day'?'2026-11-10':type==='week'?'2026-11-09':'2026-11-01';return aggregateReportRange(index,{from:start,to:type==='day'?'2026-11-15':start,organizations}).total.amount;});
        }''')
        self.assertEqual(totals,[2730,19998,17776])

    def test_daily_missing_dates_calendar_baseline_and_changes(self):
        self.page.evaluate('''async()=>{
          const organizations=DEFAULT_ORGS.slice(0,2),reports=Array.from({length:8},(_,i)=>{const start=addDays('2026-11-01',i);return {id:start,type:'day',start,end:start,sourceCoverage:{start:'2026-11-01',end:'2026-11-08'},organizations,totals:[],warnings:[],rows:organizations.map(organization=>({organization,work:'Дневная работа',count:i+0.5,amount:(i+1)*100}))};}).filter((_,i)=>i!==4);
          await storeReports(reports);state.settings.groups=[];state.selectionInitialized=false;renderAll();
        }''')
        self.page.locator('#periodType').select_option('day')
        self.page.evaluate("state.selectedOrganizations=new Set(DEFAULT_ORGS.slice(0,2));document.querySelector('#advancedAnalytics').open=true;renderDashboard(false)")
        self.page.locator('#overviewMode').select_option('range');self.page.locator('#rangeStart').select_option('2026-11-01')
        self.assertIn('Пропуск',self.page.locator('#chartsContainer').inner_text())
        self.page.locator('#usualPanel>summary').click()
        self.page.wait_for_function('document.querySelector("#usualCoverage").textContent.includes("Среднее основано")')
        self.assertIn('Среднее основано на 3 из 4',self.page.locator('#usualCoverage').inner_text())
        self.page.locator('#regularityPanel>summary').click()
        self.page.wait_for_function('document.querySelector("#regularityCoverage").textContent.includes("Фактических периодов")')
        self.assertIn('Фактических периодов: 7 из 8',self.page.locator('#regularityCoverage').inner_text())
        open_view(self.page,'upload');self.page.locator('#calendarType').select_option('day')
        self.assertEqual(self.page.locator('#reportCalendar button').count(),12)
        self.page.locator('#reportCalendar [data-month=\"2026-11\"]').click()
        missing=self.page.locator('[data-date=\"2026-11-05\"]')
        self.assertIn('Нет отчёта',missing.get_attribute('aria-label'))
        self.page.locator('#qualityPeriod').select_option('day:2026-11-05');self.page.locator('#qualityOrg').select_option('СМУ16');self.page.locator('#qualityType').select_option('missing')
        self.assertIn('05.11.2026',self.page.locator('#qualityTable').inner_text())
        open_view(self.page,'changes');self.page.locator('#changesType').select_option('day');self.page.locator('#changesMode').select_option('all')
        self.assertTrue(self.page.evaluate("changeState.analysis.rows.length===2 && changeState.analysis.rows.every(row=>row.basePeriod.start==='2026-11-07'&&row.currentPeriod.start==='2026-11-08')"))
        self.page.locator('#changesMethod').select_option('range')
        self.assertTrue(self.page.evaluate('changeState.analysis.rows.length>0'))

    def load_six_week_fixture(self):
        self.page.evaluate('''async()=>{
          const organizations=DEFAULT_ORGS.slice(0,2);
          const reports=Array.from({length:6},(_,i)=>{const start=addDays('2026-09-07',7*i),k=i+1;return {id:start,type:'week',start,end:periodEnd(start,'week'),organizations,totals:[],warnings:[],rows:[{organization:organizations[0],work:'Альфа',count:k/2,amount:10*k*k},{organization:organizations[1],work:'Бета',count:1.5*k,amount:20*k*k}]};});
          await storeReports(reports);await saveSettings({...state.settings,groups:[{id:'g',name:'Общая группа',works:['Альфа','Бета']}]});
          state.selectedMetrics=new Set(['count','amount']);state.selectionInitialized=false;renderAll();state.selectedOrganizations=new Set(organizations);state.selectedCategories=new Set(['Общая группа']);
          document.querySelector('#advancedAnalytics').open=true;renderDashboard(false);
        }''')

    def test_group_coverage_search_and_marked_options(self):
        self.load_large_group_fixture();open_view(self.page,'groups')
        self.assertIn('Названий в группах: 2 из 304',self.page.locator('#groupCoverage').inner_text())
        self.assertIn('Без группы: 302',self.page.locator('#groupCoverage').inner_text())
        self.page.locator('#groupSearch').press_sequentially('ди')
        self.assertEqual(self.page.locator('#groupList details').count(),1)
        self.assertIn('Диагностика',self.page.locator('#groupList summary').inner_text())
        self.assertIn('1 из 304 наименований',self.page.locator('#groupList summary').inner_text())
        self.assertIn('2 из 304',self.page.locator('#groupCoverage').inner_text())
        self.page.locator('#groupList summary').click()
        self.assertEqual(self.page.locator('#groupList option[value=g1]').inner_text(),'▦ Датчики (группа)')
        self.page.locator('#groupSearch').fill('нет совпадений')
        self.assertIn('не найдены',self.page.locator('#groupList').inner_text())
        self.page.locator('#groupSearch').fill('')
        self.assertEqual(self.page.locator('#groupList details').count(),2)
        self.page.locator('#groupAuditPanel>summary').click()
        self.assertEqual(self.page.locator('#groupAuditResults option[value=g1]').first.inner_text(),'▦ Датчики (группа)')
        open_view(self.page,'dashboard');self.page.locator('#categoryFilter>summary').click()
        labels=self.page.locator('#categories .category-row span').all_text_contents()
        self.assertIn('▦ Датчики (группа)',labels);self.assertIn('Датчик 002',labels)
        self.assertTrue(self.page.evaluate("state.selectedCategories.has('Датчики') && ![...state.selectedCategories].some(name=>name.startsWith('▦'))"))
        self.assertEqual(self.page.evaluate("state.settings.groups.find(group=>group.id==='g1').name"),'Датчики')

    def test_three_weeks_versus_three_totals_contributions_and_csv(self):
        self.load_six_week_fixture()
        self.page.locator('#periodComparisonScope').select_option('range')
        self.assertTrue(self.page.locator('#periodComparisonRanges').is_visible())
        self.assertFalse(self.page.locator('#periodComparisonSingle').is_visible())
        expected={'baseRangeStart':'2026-09-07','baseRangeEnd':'2026-09-21','currentRangeStart':'2026-09-28','currentRangeEnd':'2026-10-12'}
        for key,value in expected.items():self.assertEqual(self.page.locator('#'+key).input_value(),value)
        totals=self.page.evaluate('''()=>{
          const {before,after}=periodComparisonRanges(analyticsIndex(),[...state.selectedOrganizations],state.selectedCategories),changes=analyzeContributions(before,after);
          return {before:before.total,after:after.total,sizes:[before.periods.length,after.periods.length],entries:before.by.size,delta:changes.amount.delta,reconciled:changes.amount.reconciled};
        }''')
        self.assertEqual(totals['sizes'],[3,3]);self.assertEqual(totals['entries'],1)
        self.assertEqual(totals['before'],{'count':12,'amount':420,'average':35})
        self.assertEqual(totals['after'],{'count':30,'amount':2310,'average':77})
        self.assertEqual(totals['delta'],1890);self.assertTrue(totals['reconciled'])
        self.assertEqual(self.page.locator('#comparisonCoverage').inner_text().count('3 недели'),2)
        self.assertIn('27.09.2026',self.page.locator('#comparisonTable').inner_text())
        self.assertIn('18.10.2026',self.page.locator('#comparisonTable').inner_text())
        amount=self.page.locator('#comparisonTable .metric-table').filter(has=self.page.get_by_role('heading',name='Сумма',exact=True))
        with self.page.expect_download() as download:amount.locator('.table-export').click()
        rows=list(csv.reader(io.StringIO(Path(download.value.path()).read_text(encoding='utf-8-sig')),delimiter=';'))
        self.assertEqual(next(row[1:] for row in rows if row and row[0]=='Итого по выбранным работам'),['420','2310','1890','450'])
        self.assertIn('27.09.2026',next(row[1] for row in rows if row and row[0]=='Первый интервал'))
        self.page.locator('#periodComparisonScope').select_option('single')
        self.page.locator('#basePeriod').select_option('2026-09-07');self.page.locator('#currentPeriod').select_option('2026-09-14')
        single=self.page.evaluate('''()=>{const {before,after}=periodComparisonRanges(analyticsIndex(),[...state.selectedOrganizations],state.selectedCategories);return [before.periods.length,after.periods.length,before.total.amount,after.total.amount];}''')
        self.assertEqual(single,[1,1,30,120])

    def test_range_comparison_missing_department_and_mobile_controls(self):
        self.load_six_week_fixture();self.page.locator('#periodComparisonScope').select_option('range')
        self.page.evaluate('''async()=>{
          const reports=state.reports.map((report,i)=>i===1?{...report,organizations:[DEFAULT_ORGS[0]],rows:report.rows.filter(row=>row.organization===DEFAULT_ORGS[0])}:report);
          await storeReports(reports);renderAll();
        }''')
        self.assertIn('АВТО: нет полного отчёта за 14.09.2026',self.page.locator('#comparisonCoverage').inner_text())
        result=self.page.evaluate('''()=>{const {before,after}=periodComparisonRanges(analyticsIndex(),[...state.selectedOrganizations],state.selectedCategories);return [before.complete,before.total.amount,after.complete,after.total.amount];}''')
        self.assertEqual(result,[False,None,True,2310])
        self.page.evaluate("state.selectedOrganizations=new Set([DEFAULT_ORGS[0]]);renderDashboard(false)")
        self.assertEqual(self.page.evaluate('''()=>{const {before,after}=periodComparisonRanges(analyticsIndex(),[...state.selectedOrganizations],state.selectedCategories);return [before.complete,before.total.count,before.total.amount,after.total.count,after.total.amount];}'''),[True,3,140,7.5,770])
        for width in [320,390,1024]:
            self.page.set_viewport_size({'width':width,'height':900})
            self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
            for key in ['baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd']:
                box=self.page.locator('#'+key).bounding_box()
                self.assertGreaterEqual(box['height'],44)
                self.assertGreaterEqual(box['x'],0);self.assertLessEqual(box['x']+box['width'],width+1)
        self.page.locator('#baseRangeEnd').select_option('2026-09-07')
        self.assertIn('разной длительности',self.page.locator('#comparisonCoverage').inner_text())
        self.page.locator('#currentRangeStart').select_option('2026-09-07')
        self.assertIn('пересекаются',self.page.locator('#comparisonCoverage').inner_text())

    def test_go_prefix_precedes_inside_matches_in_all_five_search_fields(self):
        self.page.evaluate('''async()=>{
          const names=['Автомобиль','Агрегат горелки','Головка блока','Горелка','Гофра','Замена головки','Ремонт горелки'],orgs=DEFAULT_ORGS.slice(0,2);
          const reports=['2026-10-19','2026-10-26'].map((start,i)=>({id:start,type:'week',start,end:periodEnd(start,'week'),organizations:orgs,totals:[],warnings:[],rows:orgs.flatMap(organization=>names.map((work,j)=>({organization,work,count:j+1,amount:(j+1)*(i+1)*100})))}));
          await storeReports(reports);state.settings.groups=[];state.selectionInitialized=false;renderAll();
          state.selectedOrganizations=new Set(orgs);state.selectedCategories=new Set(names);document.querySelector('#overviewMode').value='range';document.querySelector('#rangeStart').value=reports[0].start;document.querySelector('#rangeEnd').value=reports[1].start;document.querySelector('#advancedAnalytics').open=true;renderDashboard(false);
        }''')
        expected=['Головка блока','Горелка','Гофра','Агрегат горелки','Замена головки','Ремонт горелки']
        def query(field,rows):
            self.page.locator(field).press_sequentially('го')
            if field=='#changesSearch':self.page.wait_for_function('document.querySelector("#changesSort").disabled')
            actual=list(dict.fromkeys(self.page.locator(rows).all_text_contents()))
            self.assertEqual(actual,expected,(field,actual))
        self.page.locator('#categoryFilter>summary').click()
        query('#categorySearch','#categories .category-row span')
        self.page.locator('#categoryFilter>summary').click()
        self.page.locator('#regularityPanel>summary').click()
        query('#regularitySearch','#regularityResults tbody tr td:first-child')
        open_view(self.page,'groups');self.page.locator('#createGroup').click()
        self.page.evaluate("state.selectedWorks.add('Автомобиль');renderWorkList()")
        query('#workSearch','#worksList .work-row span')
        self.assertTrue(self.page.evaluate("state.selectedWorks.has('Автомобиль')"))
        self.page.locator('#workSearch').press('Backspace')
        self.assertEqual(self.page.locator('#worksList .work-row span').all_text_contents(),expected)
        self.page.locator('#workSearch').fill('')
        self.assertEqual(self.page.locator('#worksList .work-row span').first.inner_text(),'Автомобиль')
        self.page.locator('#groupAuditPanel>summary').click()
        query('#groupAuditSearch','#groupAuditResults tbody tr td:first-child')
        open_view(self.page,'changes');self.page.locator('#changesMode').select_option('all')
        query('#changesSearch','#changesResults .change-open')

    def load_large_group_fixture(self):
        self.page.evaluate('''async()=>{
          const orgs=DEFAULT_ORGS.slice(0,2),works=[...Array.from({length:300},(_,i)=>'Датчик '+String(i+1).padStart(3,'0')),'Диагностика','Демонтаж','Замена датчика','Проверка датчика'];
          const reports=['2026-10-19','2026-10-26'].map((start,i)=>({id:start,type:'week',start,end:periodEnd(start,'week'),organizations:orgs,totals:[],warnings:[],
            issues:i?Array.from({length:60},(_,j)=>({type:'invalid',reason:'Проверочная проблема '+j,row:j+2,organization:orgs[0],raw:['Тест']})):[],
            rows:orgs.flatMap(organization=>works.map((work,j)=>({organization,work,count:j+0.5,amount:(i+1)*(j+1)*100})))}));
          await storeReports(reports);await saveSettings({...state.settings,groups:[{id:'g1',name:'Датчики',works:['Датчик 001']},{id:'g2',name:'Диагностика',works:['Диагностика']}]});
          state.selectionInitialized=false;renderAll();state.selectedOrganizations=new Set(orgs);state.selectedCategories=new Set(categoryOrder(currentTypeReports(),new Set(orgs),'amount'));
          document.querySelector('#overviewMode').value='range';document.querySelector('#rangeStart').value=reports[0].start;document.querySelector('#rangeEnd').value=reports[1].start;
          document.querySelector('#advancedAnalytics').open=true;renderDashboard(false);
        }''')

    def test_group_workspace_search_selection_and_manual_save(self):
        self.load_large_group_fixture()
        before=self.page.evaluate('JSON.stringify(state.reports)')
        open_view(self.page,'groups')
        self.assertEqual(self.page.locator('#groups>.panel').count(),1)
        self.assertEqual(self.page.locator('#groupWorkspace #groupAuditPanel').count(),1)
        self.assertFalse(self.page.locator('#groupEditor').evaluate('(node)=>node.open'))
        self.page.locator('#createGroup').click()
        self.page.locator('#workPageSelect').select_option(value='12')
        self.assertIn('Датчик 300',self.page.locator('#worksList').inner_text())
        self.page.locator('#worksList label').filter(has_text='Датчик 300').locator('input').check()
        self.assertTrue(self.page.locator('#worksList label').filter(has_text='Датчик 300').locator('input').is_checked())
        self.page.locator('#workSearch').fill('ди')
        self.assertEqual(self.page.locator('#worksList .work-row span').all_text_contents(),['Диагностика'])
        self.assertTrue(self.page.evaluate("state.selectedWorks.has('Датчик 300')"))
        self.page.locator('#worksList input').check()
        self.page.locator('#workSearch').fill('дат')
        self.assertEqual(self.page.locator('#workPageSelect').input_value(),'1')
        self.page.locator('#workPageSelect').select_option(value='13')
        self.assertEqual(self.page.locator('#worksList .work-row span').all_text_contents(),['Замена датчика','Проверка датчика'])
        self.page.locator('#groupName').fill('Общая проверочная группа')
        self.page.locator('#saveGroup').click()
        self.page.wait_for_function("state.settings.groups.some(group=>group.name==='Общая проверочная группа')")
        self.assertEqual(self.page.evaluate("state.settings.groups.find(group=>group.name==='Общая проверочная группа').works.slice().sort((a,b)=>a.localeCompare(b,'ru'))"),['Датчик 300','Диагностика'])
        self.assertEqual(self.page.evaluate('JSON.stringify(state.reports)'),before)
        group=self.page.locator('#groupList details').filter(has_text='Общая проверочная группа')
        group.locator('summary').click()
        group.get_by_role('button',name='Изменить группу',exact=True).click()
        self.assertTrue(self.page.locator('#groupEditor').evaluate('(node)=>node.open'))
        self.assertEqual(self.page.locator('#groupName').input_value(),'Общая проверочная группа')
        self.assertEqual(self.page.evaluate('state.selectedWorks.size'),2)
        with self.page.expect_download() as download:self.page.locator('#exportGroups').click()
        backup_bytes=Path(download.value.path()).read_bytes()
        backup=json.loads(backup_bytes)
        self.assertEqual(backup['format'],'work-groups')
        self.assertEqual(len(backup['groups']),2)
        group.locator('.group-member').filter(has_text='Датчик 300').locator('select').select_option('g1')
        self.page.wait_for_function("state.settings.groups.find(group=>group.id==='g1').works.includes('Датчик 300')")
        self.assertEqual(self.page.evaluate("state.settings.groups.find(group=>group.name==='Общая проверочная группа').works"),['Диагностика'])
        self.page.once('dialog',lambda dialog:dialog.accept())
        self.page.locator('#importGroups').set_input_files({'name':'groups.json','mimeType':'application/json','buffer':backup_bytes})
        self.page.wait_for_function("state.settings.groups.some(group=>group.name==='Общая проверочная группа'&&group.works.length===2)")
        self.assertEqual(self.page.evaluate('JSON.stringify(state.reports)'),before)
        self.page.reload();self.page.wait_for_function('state.db!==null')
        self.assertTrue(self.page.evaluate("state.settings.groups.some(group=>group.name==='Общая проверочная группа'&&group.works.length===2)"))
        self.page.evaluate('''async()=>{
          await saveSettings({...state.settings,groups:[...state.settings.groups,{id:'legacy',name:'Архивная группа',works:['Датчик из старой копии']}]});renderAll();
        }''')
        open_view(self.page,'groups')
        legacy=self.page.locator('#groupList details[data-group=legacy]')
        legacy.locator('summary').click();legacy.get_by_role('button',name='Изменить группу',exact=True).click()
        self.page.locator('#workSearch').fill('датчик из')
        self.assertEqual(self.page.locator('#worksList .work-row span').all_text_contents(),['Датчик из старой копии'])
        self.assertTrue(self.page.locator('#worksList input').is_checked())

    def test_direct_page_jumps_and_search_across_analytics_lists(self):
        self.load_large_group_fixture()
        open_view(self.page,'groups')
        self.page.locator('#groupAuditPanel>summary').click()
        self.page.locator('#groupAuditPageSelect').select_option(value='13')
        self.assertIn('Проверка датчика',self.page.locator('#groupAuditResults').inner_text())
        self.page.locator('#groupAuditSearch').fill('ди')
        self.assertEqual(self.page.locator('#groupAuditPageSelect').input_value(),'1')
        self.assertTrue(self.page.locator('#groupAuditPageSelect').is_disabled())
        self.assertIn('Диагностика',self.page.locator('#groupAuditResults').inner_text())
        open_view(self.page,'dashboard')
        self.page.locator('#categoryFilter>summary').click()
        self.page.locator('#categorySearch').fill('ди')
        self.assertEqual(self.page.locator('#categories .category-row span').all_text_contents(),['▦ Диагностика (группа)'])
        self.page.locator('#categoryFilter>summary').click()
        self.page.locator('#regularityPanel>summary').click()
        self.page.locator('#regularityPageSelect').select_option(value='11')
        self.assertEqual(self.page.locator('#regularityPageSelect').input_value(),'11')
        self.page.locator('#regularitySearch').fill('дат')
        names=self.page.locator('#regularityResults tbody tr td:first-child').all_text_contents()
        self.assertTrue(all(name.startswith('Датч') for name in names),names)
        self.page.locator('#usualPanel>summary').click()
        self.page.locator('#usualPageSelect').select_option(value='11')
        self.assertEqual(self.page.locator('#usualPageSelect').input_value(),'11')
        self.page.locator('#contributions>summary').click()
        self.page.locator('#contributionPageSelect').select_option(value='11')
        self.assertEqual(self.page.locator('#contributionPageSelect').input_value(),'11')
        open_view(self.page,'upload')
        self.page.locator('#qualityPageSelect').select_option(value='3')
        self.assertEqual(self.page.locator('#qualityPageSelect').input_value(),'3')
        open_view(self.page,'changes')
        self.page.locator('#changesMode').select_option('all')
        self.page.locator('#changesPageSelect').select_option(value='11')
        self.assertEqual(self.page.locator('#changesPageSelect').input_value(),'11')
        self.page.locator('#changesSearch').fill('ди')
        self.page.wait_for_function("changeState.page===0 && document.querySelector('#changesResults').textContent.includes('Диагностика')")
        self.assertTrue(self.page.locator('#changesPageSelect').is_disabled())
        self.assertEqual(self.page.locator('#changesResults .change-open').all_text_contents(),['Диагностика']*4)
        self.assertTrue(self.page.locator('#changesSort').is_disabled())
        self.page.locator('#changesSearch').fill('')
        self.page.wait_for_function('!document.querySelector("#changesSort").disabled')

    def test_group_workspace_and_page_controls_fit_phone(self):
        self.load_large_group_fixture()
        open_view(self.page,'groups');self.page.locator('#createGroup').click()
        self.page.locator('#groupAuditPanel>summary').click()
        for width in [320,390,1024]:
            self.page.set_viewport_size({'width':width,'height':900})
            for theme in ['light','dark']:
                self.page.evaluate('(theme)=>document.documentElement.dataset.theme=theme',theme)
                self.assertTrue(self.page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
                for selector in ['#workPageSelect','#groupAuditPageSelect']:
                    box=self.page.locator(selector).evaluate('(node)=>{const r=node.getBoundingClientRect();return {x:r.x,width:r.width,height:r.height};}')
                    self.assertGreaterEqual(box['x'],0)
                    self.assertLessEqual(box['x']+box['width'],width+1)
                    self.assertGreaterEqual(box['height'],44)
                if width<580:
                    for selector in ['#workPrev','#workNext','#groupAuditPrev','#groupAuditNext']:
                        box=self.page.locator(selector).bounding_box()
                        self.assertGreaterEqual(box['width'],100)
                        self.assertLessEqual(box['height'],100)

    def test_calendar_and_uploaded_reports_share_one_section(self):
        self.load_analytics_fixture()
        open_view(self.page,'upload')
        reports=self.page.locator('#reportPeriods #uploadedReports')
        self.assertEqual(reports.count(),1)
        self.assertFalse(self.page.locator('#reports').is_visible())
        self.assertEqual(self.page.locator('#reportPeriods #reportCalendar').count(),1)
        navigation=self.page.locator('#reportPeriods .calendar-toolbar').evaluate('''root=>[...root.querySelectorAll('button')].map(node=>({y:node.getBoundingClientRect().y,height:node.getBoundingClientRect().height}))''')
        self.assertAlmostEqual(navigation[0]['y'],navigation[1]['y'],delta=1)
        self.assertAlmostEqual(navigation[0]['height'],navigation[1]['height'],delta=1)
        self.assertIn('2 периода',reports.locator('summary').inner_text())
        reports.locator('summary').focus()
        self.page.keyboard.press('Space')
        self.assertTrue(self.page.locator('#reports').is_visible())
        self.assertEqual(reports.locator('.report-row').count(),2)
        self.page.locator('#calendarType').select_option('month')
        self.assertEqual(self.page.locator('#reportCalendar>button').count(),12)
        self.assertEqual(reports.locator('.report-row').count(),2)
        self.page.locator('#calendarType').select_option('week')
        self.page.locator('#reportCalendar [data-month=\"2026-10\"]').click()
        self.page.locator('#calendarDetails details').last.locator('summary').click()
        self.page.get_by_role('button',name='Контроль качества: 26.10.2026',exact=False).click()
        self.assertEqual(self.page.locator('#qualityPeriod').input_value(),'week:2026-10-26')
        self.page.once('dialog',lambda dialog:dialog.dismiss())
        reports.locator('button').first.click()
        self.assertEqual(self.page.evaluate('state.reports.length'),2)
        self.page.once('dialog',lambda dialog:dialog.accept())
        reports.locator('button').first.click()
        self.page.wait_for_function('state.reports.length===1')
        self.assertIn('1 период',reports.locator('summary').inner_text())
        self.assertEqual(reports.locator('.report-row').count(),1)
        self.page.reload()
        self.page.wait_for_function('state.db!==null')
        self.assertEqual(self.page.evaluate('state.reports.length'),1)

    def test_analytics_headings_controls_and_collapsed_help(self):
        self.load_analytics_fixture()
        self.assertEqual(self.page.locator('#overviewHeading').inner_text(),'Итоги за период')
        self.assertEqual(self.page.locator('#structurePanel>summary').inner_text(),'Состав работ и доли')
        self.assertEqual(self.page.locator('#usualPanel>summary').inner_text(),'Отклонения от обычного уровня')
        self.assertEqual(self.page.locator('#regularityPanel>summary').inner_text(),'Частота выполнения работ')
        self.assertFalse(self.page.locator('.overview-footnote').is_visible())
        self.page.locator('#overviewPanel .quick-help>summary').click()
        self.assertTrue(self.page.locator('.overview-footnote').is_visible())
        self.page.locator('#chartBreakdown').select_option('organizations')
        self.assertFalse(self.page.locator('#chartReadGuide').is_visible())
        self.page.locator('#chartNotes>summary').click()
        self.assertTrue(self.page.locator('#chartReadGuide').is_visible())
        self.page.locator('#chartBreakdown').select_option('works')
        self.assertFalse(self.page.locator('#chartNotes').is_visible())
        for width in [320,390,1024,1440]:
            self.page.set_viewport_size({'width':width,'height':900})
            for design in ['modern','classic','compact','neon','glass','corporate','paper','industrial']:
                for size in ['standard','large','xlarge']:
                    result=self.page.evaluate('''({design,size})=>{
                      Object.assign(document.documentElement.dataset,{design,uiSize:size});
                      const selectors=['#overviewHeading','#chartHeading','#comparisonHeading','#structurePanel>summary','#usualPanel>summary','#regularityPanel>summary'];
                      const headings=selectors.map(selector=>getComputedStyle(document.querySelector(selector)).fontSize);
                      return {headings,overflow:document.documentElement.scrollWidth>innerWidth};
                    }''',{'design':design,'size':size})
                    self.assertEqual(len(set(result['headings'])),1,(width,design,size,result))
                    self.assertFalse(result['overflow'],(width,design,size))
        self.page.set_viewport_size({'width':1440,'height':900})
        self.page.evaluate('Object.assign(document.documentElement.dataset,{design:"modern",uiSize:"standard"})')
        self.page.locator('#chartBreakdown').select_option('organizations')
        positions=self.page.locator('#chartPanel .chart-view-controls').evaluate('''root=>[...root.querySelectorAll('select,.multi-dropdown>summary')].filter(node=>node.getClientRects().length).map(node=>({y:node.getBoundingClientRect().y,height:node.getBoundingClientRect().height,font:getComputedStyle(node).fontSize}))''')
        self.assertGreaterEqual(len(positions),3)
        self.assertLessEqual(max(item['y'] for item in positions)-min(item['y'] for item in positions),1)
        self.assertEqual(len(set(item['font'] for item in positions)),1)
        self.assertLessEqual(max(item['height'] for item in positions)-min(item['height'] for item in positions),1)

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

    def test_partial_calendar_direction_colors_and_average_explanation(self):
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
        for key in ['up','high']:self.assertEqual(result[key],'numeric-rise')
        for key in ['down','low']:self.assertEqual(result[key],'numeric-fall')
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
        open_view(self.page,'upload')
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

