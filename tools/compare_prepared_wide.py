"""Completed prepared/live document parity against the actual old JavaScript parser.
Small fixtures only; Python/reference may materialize fixture documents. Rust never does.
"""
import argparse, io, json, subprocess, sys, tempfile, zipfile, xml.etree.ElementTree as ET
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tests'))
from test_daily_wide import wide_xlsx
sys.path.insert(0,str(ROOT/'tools'))
from compare_native_wide import altered

def change(payload, kind):
    with zipfile.ZipFile(io.BytesIO(payload)) as z: parts={i.filename:z.read(i.filename) for i in z.infolist()}
    name='xl/worksheets/sheet2.xml' if kind=='future-sheet' else 'xl/worksheets/sheet1.xml'
    sheet=ET.fromstring(parts[name])
    for row in sheet.findall('.//{*}row'):
        ri=int(row.attrib['r'])
        if ri<20:continue
        cells=row.findall('{*}c')
        if kind=='invalid-source' and ri==20:
            cell=cells[-2];address=cell.attrib['r'];cell.clear();cell.attrib.update({'r':address,'t':'inlineStr'})
            ET.SubElement(ET.SubElement(cell,'is'),'t').text='bad'
        elif kind=='zero-day':
            is_total=ri==28
            for c in list(cells):
                ref=c.attrib['r']
                if ref in ['B'+str(ri),'C'+str(ri)]:
                    if ri in [21,23,25,27]:row.remove(c)
                    else:c.find('{*}v').text='0'
            for cell,delta in [(cells[-2],2 if is_total else .5),(cells[-1],20 if is_total else 5)]:cell.find('{*}v').text=str(float(cell.find('{*}v').text)-delta)
        elif kind=='future-sheet':
            for c in cells[1:-2]:row.remove(c)
            for c in cells[-2:]:c.find('{*}v').text='0'
    parts[name]=ET.tostring(sheet)
    out=io.BytesIO()
    with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
        for name,data in parts.items():z.writestr(name,data)
    return out.getvalue()

def outside_coverage_fixture():
    with zipfile.ZipFile(io.BytesIO(wide_xlsx(2024))) as z:
        parts={i.filename:z.read(i.filename) for i in z.infolist()}
    sheet=ET.fromstring(parts['xl/worksheets/sheet1.xml'])
    def column(address):
        out=0
        for char in address.rstrip('0123456789'):out=out*26+ord(char)-64
        return out
    for row in sheet.findall('.//{*}row'):
        ri=int(row.attrib['r'])
        if ri==17:row.find('.//{*}t').text='Работа Q1 2024'
        if ri<18:continue
        for cell in list(row):
            col=column(cell.attrib['r'])
            if 5<col<734:row.remove(cell);continue
            if col>=734:
                cell.attrib['r']=('F' if col==734 else 'G')+str(ri)
                if ri>=20:
                    cell.find('{*}v').text=str((4 if ri==28 else 1) if col==734 else (40 if ri==28 else 10))
            if ri==18 and col in [2,4]:cell.find('.//{*}t').text='2024-04-01' if col==2 else '2024-04-02'
    merges=sheet.find('{*}mergeCells');merges.clear()
    for ref in ['A18:A19','B18:C18','D18:E18','F18:G18']:ET.SubElement(merges,'mergeCell',ref=ref)
    parts['xl/worksheets/sheet1.xml']=ET.tostring(sheet)
    out=io.BytesIO()
    with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
        for name,data in parts.items():z.writestr(name,data)
    return out.getvalue()

def diagnostic(issue):
    return {k:issue.get(k) for k in ['type','row','organization','scope','date','cells','expected','actual']}
def normalize_control(t):return {k:t.get(k) for k in ['type','row','name','count','amount']}
def run():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);parser.add_argument('--case');args=parser.parse_args()
    cases=[('full leap year',wide_xlsx(2024),{},'2026-11-17'),('actual dates outside declared quarter',outside_coverage_fixture(),{},'2026-11-17'),('four quarters',wide_xlsx(2024,quarter_sheets=True),{},'2026-11-17'),('selected sheet order',wide_xlsx(2024,quarter_sheets=True),{'sheets':['Q3','Q1']},'2026-11-17'),('source total mismatch',altered(wide_xlsx(2024),'bad-total'),{},'2026-11-17'),('nullable source control',change(wide_xlsx(2024),'invalid-source'),{},'2026-11-17'),('explicit zero and historical blank',change(wide_xlsx(2024,blank_day='2024-02-05'),'zero-day'),{},'2026-11-17'),('future-only sheet',change(wide_xlsx(2024,quarter_sheets=True),'future-sheet'),{'sheets':['Q1','Q2']},'2024-03-31')]
    with sync_playwright() as pw:
        browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
        page=browser.new_page();page.route('https://prepared.test/',lambda r:r.fulfill(content_type='text/html',body=(ROOT/'index.html').read_text()));page.goto('https://prepared.test/');page.wait_for_function('state.db!==null')
        payload=b'';page.route('https://prepared.test/input.xlsx',lambda r:r.fulfill(body=payload,content_type='application/octet-stream'))
        for name,payload,options,today in cases:
            if args.case and args.case not in name:continue
            old=page.evaluate('''async ({options,today})=>{reportToday=()=>today;const b=await(await fetch('/input.xlsx')).arrayBuffer();return parseDailyWorkbook(XLSX.read(new Uint8Array(b)),{...options,fileName:'fixture.xlsx'});}''',{'options':options,'today':today})
            if name=='actual dates outside declared quarter':
                assert len(old['reports'])==2 and [r['start'] for r in old['reports']]==['2024-04-01','2024-04-02']
                assert old['reports'][0]['sourceCoverage']=={'start':'2024-01-01','end':'2024-03-31'}
                assert len(old['reports'][0]['issues'])==91 and all(i['type']=='missing-day' for i in old['reports'][0]['issues'])
            with tempfile.TemporaryDirectory(prefix='c3-golden-') as tmp:
                source=Path(tmp)/'fixture.xlsx';source.write_bytes(payload)
                root=Path(tmp)/'worker';result=subprocess.run([args.binary,str(source),str(root),'emit',today,json.dumps(options)],text=True,capture_output=True)
                assert result.returncode==0,(name,result.stderr)
                summary=json.loads(result.stdout);documents=[json.loads(p.read_text()) for p in sorted((root/'documents').glob('*.json'),key=lambda p:int(p.stem.split('-')[-1]))]
                assert len(documents)==len(old['reports']),(name,'report count')
                for doc,expected in zip(documents,old['reports']):
                    for field in ['type','start','end','count','amount','hasMismatch','sheetName','fileName','sourceFormat','sourceBlock','sourceCol','sourceCountCol','sourceAmountCol','sourceCoverage']:
                        assert doc[field]==expected[field],(name,expected['start'],field,doc[field],expected[field])
                    assert doc['sourceControlDate']==expected.get('sourceControlDate'),(name,'source control date')
                    assert doc['explicitZero']==bool(expected.get('explicitZero')),(name,'zero proof')
                    assert [{k:r[k] for k in ['organization','work','count','amount']} for r in doc['rows']]==expected['rows'],(name,'financial rows')
                    assert [normalize_control(t) for t in doc['totals']]==[normalize_control(t) for t in expected['totals']],(name,'controls')
                    assert [diagnostic(i) for i in doc['issues']]==[diagnostic(i) for i in expected['issues']],(name,expected['start'],'issues',doc['issues'],expected['issues'])
                    # Native preserves known zero-control organization presence; old byOrg omits zero-only orgs.
                    organizations=list(dict.fromkeys(t.get('name') for t in expected['totals'] if t['type']=='org')) if doc['explicitZero'] else expected['organizations']
                    assert doc['organizations']==organizations,(name,'organization presence')
                    # Exercise the actual old cell-label consumer, not just key existence.
                    labels=page.evaluate('''issues=>issues.map(i=>qualityCellLabel(i))''',doc['issues'])
                    assert labels==[', '.join(i.get('cells') or []) or '—' for i in expected['issues']],(name,'legacy cell labels')
                all_source=[i for sheet in old['sheets'] for i in sheet['issues']]
                saved=documents[0]['sourceIssues'];assert [diagnostic(i) for i in saved]==[diagnostic(i) for i in all_source],(name,'complete source diagnostics')
                relevant=sum(not i.get('date') or i['date']<=today for i in all_source)
                assert [i['relevant'] for i in saved]==[not i.get('date') or i['date']<=today for i in all_source],(name,'relevance')
                assert summary['source_diagnostics']['issues']==len(all_source) and summary['source_diagnostics']['relevant']==relevant
                assert sum(len(d['issues']) for d in documents)==summary['quality']['issues'],(name,'quality counted once')
                print('PASS',name,len(documents),'complete documents; financial/control/quality/provenance, source relevance, legacy labels and streamed backfill',flush=True)
        browser.close()
if __name__=='__main__':run()
