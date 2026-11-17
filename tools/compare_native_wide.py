"""Reference parity on small real XLSX fixtures. Not a whole-app/Windows claim."""
import argparse,io,json,subprocess,sys,tempfile,zipfile,xml.etree.ElementTree as ET
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx

def altered(payload,kind):
 with zipfile.ZipFile(io.BytesIO(payload)) as z:parts={i.filename:z.read(i.filename) for i in z.infolist()}
 if kind in ['1904','1904-style1']:
  book=ET.fromstring(parts['xl/workbook.xml']);book.insert(0,ET.Element('workbookPr',{'date1904':'true'}));parts['xl/workbook.xml']=ET.tostring(book)
  parts['xl/styles.xml']=b'<styleSheet><cellXfs count="1"><xf numFmtId="14"/></cellXfs></styleSheet>' if kind=='1904' else b'<styleSheet><cellXfs count="2"><xf/><xf numFmtId="14"/></cellXfs></styleSheet>'
  for name,data in list(parts.items()):
   if name.startswith('xl/worksheets/'):
    sheet=ET.fromstring(data)
    for row in sheet.findall('.//{*}row'):
     if row.attrib.get('r')=='18':
      for c in row.findall('{*}c'):
       v=c.find('{*}v')
       if v is not None:
        v.text=str(int(v.text)-1462)
        if kind=='1904-style1':c.attrib['s']='1'
    parts[name]=ET.tostring(sheet)
 elif kind in ['duplicate','bad-total','shared','formula','uncached','ambiguous-year','split-quarter','suffix-space','tail-boundary']:
  sheet=ET.fromstring(parts['xl/worksheets/sheet1.xml'])
  if kind=='duplicate':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='18');texts=[c.find('.//{*}t') for c in row.findall('{*}c')];texts[2].text=texts[1].text
  elif kind=='tail-boundary':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='17');row.find('.//{*}t').text='aQ1 '+'x'*60;c=ET.SubElement(row,'c',r='B17',t='inlineStr');ET.SubElement(ET.SubElement(c,'is'),'t').text='next'
  elif kind=='split-quarter':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='17');row.find('.//{*}t').text='Q';c=ET.SubElement(row,'c',r='B17',t='inlineStr');ET.SubElement(ET.SubElement(c,'is'),'t').text='1'
  elif kind=='suffix-space':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='18')
   for t in row.findall('.//{*}t'):
    if t.text and t.text.startswith('2024-'):
     year,month,day=t.text.split('-');t.text=f'{int(day)}.{int(month)}.{year}\ufeffг.'
  elif kind=='ambiguous-year':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='17');row.find('.//{*}t').text='Отчёт за 2024 2025 годы'
  elif kind=='bad-total':
   row=next(r for r in sheet.findall('.//{*}row') if r.attrib.get('r')=='20');v=row.findall('{*}c')[-2].find('{*}v');v.text=str(float(v.text)+1)
  elif kind=='shared':
   strings=[]
   for c in sheet.findall('.//{*}c'):
    t=c.find('.//{*}t')
    if t is not None:
     value=t.text or '';strings.append(value);ref=c.attrib['r'];c.clear();c.attrib.update({'r':ref,'t':'s'});ET.SubElement(c,'v').text=str(len(strings)-1)
   sst=ET.Element('sst')
   for text in strings:ET.SubElement(ET.SubElement(sst,'si'),'t').text=text
   parts['xl/sharedStrings.xml']=ET.tostring(sst)
  else:
   for c in sheet.findall('.//{*}c'):
    if c.attrib.get('r')=='B21':
     c.insert(0,ET.Element('f'));c[0].text='1/2'
     if kind=='uncached':
      v=c.find('{*}v')
      if v is not None:c.remove(v)
  parts['xl/worksheets/sheet1.xml']=ET.tostring(sheet)
 out=io.BytesIO()
 with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
  for n,v in parts.items():z.writestr(n,v)
 return out.getvalue()
def native_cells(issue):return sorted(v for v in (issue.get('work_cell'),issue.get('count_cell'),issue.get('amount_cell')) if v)
def run():
 p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--case');a=p.parse_args()
 cases=[('year ISO',wide_xlsx(2024),{}),('four quarters',wide_xlsx(2024,quarter_sheets=True),{}),('numeric dates',wide_xlsx(2025,serial=True),{}),('1904 default date style',altered(wide_xlsx(1904,serial=True),'1904'),{}),('1904 style after default xf',altered(wide_xlsx(1904,serial=True),'1904-style1'),{}),('short year explicit',wide_xlsx(2024,short=True,include_year=False),{'year':2024}),('adjacent ambiguous header years',altered(wide_xlsx(2024,short=True,include_year=False),'ambiguous-year'),{}),('uncached formula',altered(wide_xlsx(2024),'uncached'),{}),('ambiguous year',wide_xlsx(2024,short=True,include_year=False),{}),('blank historical day',wide_xlsx(2024,blank_day='2024-02-05'),{}),('source totals',altered(wide_xlsx(2024),'bad-total'),{}),('duplicate days',altered(wide_xlsx(2024),'duplicate'),{}),('shared strings',altered(wide_xlsx(2024),'shared'),{}),('cached formula',altered(wide_xlsx(2024),'formula'),{}),('sheet subset',wide_xlsx(2024,quarter_sheets=True),{'sheets':['Q3','Q1']})]
 cases.append(('cropped tail boundary',altered(wide_xlsx(2024),'tail-boundary'),{}))
 cases.extend([('split quarter cells',altered(wide_xlsx(2024),'split-quarter'),{}),('year suffix FEFF',altered(wide_xlsx(2024),'suffix-space'),{})])
 cases.append(('manual columns',wide_xlsx(2024),{'sheets':['Лист'],'mappings':{'Лист':{'work_col':1,'header_row':19,'first_day_col':2,'total_count_col':734,'total_amount_col':735}}}))
 with tempfile.TemporaryDirectory() as tmp,sync_playwright() as pw:
  browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True);page=browser.new_page();page.route('https://wide-reference.test/',lambda r:r.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()));page.goto('https://wide-reference.test/');page.wait_for_function('state.db!==null')
  payload=b'';page.route('https://wide-reference.test/input.xlsx',lambda r:r.fulfill(body=payload,content_type='application/octet-stream'))
  for name,payload,options in cases:
   if a.case and a.case not in name:continue
   legacy_options=dict(options)
   if 'mappings' in options:
    names={'work_col':'workCol','header_row':'headerRow','first_day_col':'firstDayCol','total_count_col':'totalCountCol','total_amount_col':'totalAmountCol'}
    legacy_options['mappings']={sheet:{names[k]:v-1 if v is not None else None for k,v in mapping.items()} for sheet,mapping in options['mappings'].items()}
   old=page.evaluate('''async options=>{try{const buffer=await(await fetch('/input.xlsx')).arrayBuffer();const out=parseDailyWorkbook(XLSX.read(new Uint8Array(buffer)),{...options,fileName:'fixture'});return {ok:true,reports:out.reports.map(r=>({start:r.start,end:r.end,count:r.count,amount:r.amount,rows:r.rows,totals:r.totals,issues:r.issues,hasMismatch:r.hasMismatch,explicitZero:!!r.explicitZero,sourceControlDate:r.sourceControlDate||null,coverage:r.sourceCoverage,sheetName:r.sheetName})),sheets:out.sheets};}catch(e){return {ok:false,error:e.message}}}''',legacy_options)
   source=Path(tmp)/'fixture.xlsx';source.write_bytes(payload);result=subprocess.run([a.binary,str(source),str(Path(tmp)/'stage'),'wide',json.dumps(options)],text=True,capture_output=True)
   if not old['ok']:
    assert result.returncode!=0,(name,'native accepted invalid book',old);print('PASS',name,'both reject');continue
   assert result.returncode==0,(name,result.stderr)
   records=[json.loads(line) for line in result.stdout.split('\n') if line];reports=[r['data'] for r in records if r['record']=='report'];assert len(reports)==len(old['reports']),(name,len(reports),len(old['reports']))
   for r,expected in zip(reports,old['reports']):
    related=lambda typ:[x['data'] for x in records if x['record']==typ and x.get('reportId')==r['id']]
    rows=[{k:x[k] for k in ['organization','work','count','amount']} for x in related('row')]
    assert (r['start'],r['end'],r['count'],r['amount'],r['has_mismatch'],rows)==(expected['start'],expected['end'],expected['count'],expected['amount'],expected['hasMismatch'],expected['rows']),(name,r['start'],'financial mismatch')
    prov=related('provenance')[0];assert (prov['explicit_zero'],prov['coverage'],prov['source_control_date'],prov['sheet_name'])==(expected['explicitZero'],expected['coverage'],expected['sourceControlDate'],expected['sheetName']),(name,'provenance',prov,expected)
    totals=[(t['kind'],t['organization'],t['source_row'],t['count'],t['amount']) for t in related('total')];wanted=[(t['type'],t.get('name'),t['row'],t['count'],t['amount']) for t in expected['totals']];assert totals==wanted,(name,'controls',totals,wanted)
    actual=[(i['kind'],i['source_row'],i['organization'],native_cells(i)) for i in related('issue')];wanted=[(i['type'],i['row'],i.get('organization'),sorted(i['cells'])) for i in expected['issues']];assert actual==wanted,(name,r['start'],'diagnostics',actual,wanted)
   source_issues=[x['data'] for x in records if x['record']=='sourceIssue']
   for sheet in old['sheets']:
    actual=[(i['kind'],i['date'],i['source_row'],i['organization'],native_cells(i)) for i in source_issues if i['sheet']==sheet['name']]
    expected=[(i['type'],i.get('date'),i['row'],i.get('organization'),sorted(i['cells'])) for i in sheet['issues']];assert actual==expected,(name,'source diagnostics',actual,expected)
   print('PASS',name,len(reports),'days; rows, quantities, sums, controls, provenance and diagnostics')
  browser.close()
if __name__=='__main__':run()
