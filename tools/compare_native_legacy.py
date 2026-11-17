"""Small control-fixture parity check. Does not assert whole-app or UI parity."""
import argparse,json,subprocess,sys,tempfile,calendar,datetime,zipfile,io,xml.etree.ElementTree as ET
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tests'))
from test_import_controls import daily_workbook_bytes
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);a=p.parse_args()
html=(ROOT/'index.html').read_text()
js=html[html.index('function number('):html.index('function request(')]+html[html.index('function orgName('):html.index('// Shared daily matrix importer:')]
node="""const vm=require('node:vm'),fs=require('node:fs');const input=JSON.parse(fs.readFileSync(0,'utf8'));const context={state:{settings:{orgNames:['СМУ16','АВТО','ТРАК','ЦЕНТР']}},window:{XLSX:{utils:{sheet_to_json:x=>x}}},Intl};vm.createContext(context);vm.runInContext(input.code,context);context.matrix=input.matrix;process.stdout.write(JSON.stringify(vm.runInContext('parseSheet(matrix)',context)));"""
blob=daily_workbook_bytes()
with zipfile.ZipFile(io.BytesIO(blob)) as z:
 sheet=ET.fromstring(z.read('xl/worksheets/sheet1.xml'))
 matrix=[]
 for row in sheet.findall('.//{*}row'):
  out=[]
  for cell in row.findall('{*}c'):
   t=cell.find('.//{*}t');v=cell.find('{*}v');out.append(t.text if t is not None else float(v.text) if v is not None else None)
  matrix.append(out)
old=json.loads(subprocess.run(['node','-e',node],input=json.dumps({'code':js,'matrix':matrix}),text=True,capture_output=True,check=True).stdout)
with tempfile.TemporaryDirectory() as tmp:
 source=Path(tmp)/'control.xlsx';source.write_bytes(blob)
 for kind,start in [('week','2026-11-09'),('day','2026-11-10'),('month','2024-01-01')]:
  result=subprocess.run([a.binary,str(source),str(Path(tmp)/'stage'),kind,start,'xl/worksheets/sheet1.xml'],text=True,capture_output=True,check=True)
  records=[json.loads(line) for line in result.stdout.split('\n') if line];reports=[r['data'] for r in records if r['record']=='report'];assert len(reports)==len(old)
  for i,(r,expected) in enumerate(zip(reports,old)):
   if kind=='month':
    initial=datetime.date.fromisoformat(start);m=initial.year*12+initial.month-1+i;first=datetime.date(m//12,m%12+1,1);end=first.replace(day=calendar.monthrange(first.year,first.month)[1])
   else:first=datetime.date.fromisoformat(start)+datetime.timedelta(days=i*(7 if kind=='week' else 1));end=first+datetime.timedelta(days=6 if kind=='week' else 0)
   assert (r['start'],r['end'])==(str(first),str(end))
   assert r['count']==expected['count'] and r['amount']==expected['amount'] and r['has_mismatch']==expected['hasMismatch']
   rows=[x['data'] for x in records if x['record']=='row' and x['reportId']==r['id']]
   assert [{k:x[k] for k in ['organization','work','count','amount']} for x in rows]==expected['rows']
   assert not [x for x in records if x['record']=='issue' and x['reportId']==r['id']] and not expected['issues']
  print(f'PASS {kind}: {len(reports)} periods; dates, all detail rows and rounded totals equal old parseSheet')
 # A broken quantity in the first block must preserve diagnostic kind and exact original coordinates.
 matrix[2][1]='oops'
 broken=ET.fromstring(zipfile.ZipFile(io.BytesIO(blob)).read('xl/worksheets/sheet1.xml'))
 target=next(c for c in broken.findall('.//{*}c') if c.attrib.get('r')=='B3');target.clear();target.attrib.update({'r':'B3','t':'inlineStr'});ET.SubElement(ET.SubElement(target,'is'),'t').text='oops'
 new=io.BytesIO()
 with zipfile.ZipFile(io.BytesIO(blob)) as original,zipfile.ZipFile(new,'w',zipfile.ZIP_DEFLATED) as output:
  for info in original.infolist():output.writestr(info.filename,ET.tostring(broken) if info.filename=='xl/worksheets/sheet1.xml' else original.read(info.filename))
 source.write_bytes(new.getvalue())
 expected=json.loads(subprocess.run(['node','-e',node],input=json.dumps({'code':js,'matrix':matrix}),text=True,capture_output=True,check=True).stdout)
 result=subprocess.run([a.binary,str(source),str(Path(tmp)/'stage'),'week','2026-11-09','xl/worksheets/sheet1.xml'],text=True,capture_output=True,check=True)
 records=[json.loads(line) for line in result.stdout.split('\n') if line]
 for i,r in enumerate([x['data'] for x in records if x['record']=='report']):
  actual=[x['data'] for x in records if x['record']=='issue' and x['reportId']==r['id']]
  canonical=[(x['kind'],x['source_row'],x['organization'],sorted(v for v in [x['work_cell'],x['count_cell'],x['amount_cell']] if v)) for x in actual]
  reference=[(x['type'],x['row'],x['organization'],sorted(x['cells'])) for x in expected[i]['issues']]
  assert canonical==reference,(canonical,reference)
  assert (r['count'],r['amount'],r['has_mismatch'])==(expected[i]['count'],expected[i]['amount'],expected[i]['hasMismatch'])
 print('PASS invalid quantity: diagnostic kinds, source rows/organizations/cells and rounded totals equal old parseSheet')

 # Focused ordinary-format regressions, evaluated by the real legacy parseSheet.
 def compare_edge(name, matrix):
  sheet=ET.Element('worksheet'); data=ET.SubElement(sheet,'sheetData')
  for ri,row in enumerate(matrix,1):
   element=ET.SubElement(data,'row',r=str(ri))
   for ci,value in enumerate(row):
    if value is None:continue
    cell=ET.SubElement(element,'c',r=f'{chr(65+ci)}{ri}',t='inlineStr')
    ET.SubElement(ET.SubElement(cell,'is'),'t').text=str(value)
  with zipfile.ZipFile(source,'w',zipfile.ZIP_DEFLATED) as output:
   output.writestr('xl/worksheets/sheet1.xml',ET.tostring(sheet,encoding='utf-8'))
  reference=json.loads(subprocess.run(['node','-e',node],input=json.dumps({'code':js,'matrix':matrix}),text=True,capture_output=True,check=True).stdout)[0]
  result=subprocess.run([a.binary,str(source),str(Path(tmp)/'stage'),'day','2026-11-13','xl/worksheets/sheet1.xml'],text=True,capture_output=True,check=True)
  records=[json.loads(line) for line in result.stdout.split('\n') if line]
  report=next(x['data'] for x in records if x['record']=='report')
  rows=[x['data'] for x in records if x['record']=='row']
  assert [{k:x[k] for k in ['organization','work','count','amount']} for x in rows]==reference['rows'],name
  assert (report['count'],report['amount'],report['has_mismatch'])==(reference['count'],reference['amount'],reference['hasMismatch']),name
  canonical=[(x['data']['kind'],x['data']['source_row'],x['data']['organization'],sorted(v for v in [x['data']['work_cell'],x['data']['count_cell'],x['data']['amount_cell']] if v)) for x in records if x['record']=='issue']
  expected=[(x['type'],x['row'],x['organization'],sorted(x['cells'])) for x in reference['issues']]
  assert canonical==expected,(name,canonical,expected)
  print(f'PASS {name}: detail rows, totals, mismatch and diagnostic coordinates equal old parseSheet')
 header=['Работа','Количество','Сумма']
 for separator in ['\ufeff','\u0085']:
  compare_edge(f'total label U+{ord(separator):04X}',[header,['СМУ16','1','1'],['Работа1','1','1'],[f'Итого{separator}период','1','1']])
 for name,q,amount in [
  ('large odd scaled integers','4503599627.370497','45035996273704.97'),
  ('negative large odd scaled integers','-4503599627.370497','-45035996273704.97'),
  ('negative ties','-0.0000015','-1.125'),
  ('negative zero','-0.0000001','-0.001'),
  ('ordinary decimals','1.2345678','1.005'),
 ]:
  compare_edge(name,[header,['СМУ16',q,amount],['Работа1',q,amount],['Итого',q,amount]])
 for name,q,amount in [
  ('unnamed invalid count, absent amount','oops',None),
  ('unnamed absent count, invalid amount',None,'oops'),
  ('unnamed both invalid','oops','bad'),
  ('unnamed valid count, absent amount','1',None),
  ('unnamed absent count, valid amount',None,'1'),
  ('unnamed invalid count, valid amount','oops','1'),
 ]:
  compare_edge(name,[header,['СМУ16','1','1'],[None,q,amount],['Работа1','1','1'],['Итого','1','1']])
