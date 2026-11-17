"""Generate bounded-memory XLSX fixtures and profile the native staging executable.
This profiles ingestion only, not report validation, live SQLite application or GUI.
"""
import argparse, json, subprocess, time, zipfile, os, tempfile
from pathlib import Path

def column(n):
    out=''
    while n:n,r=divmod(n-1,26);out=chr(65+r)+out
    return out

def generate(path,cells,empty):
    per_sheet=250_000
    with zipfile.ZipFile(path,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
        sheets=(cells+per_sheet-1)//per_sheet
        z.writestr('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'+''.join(f'<Override PartName="/xl/worksheets/sheet{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' for k in range(1,sheets+1))+'</Types>')
        z.writestr('_rels/.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>')
        z.writestr('xl/workbook.xml','<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'+''.join(f'<sheet name="Sheet{s}" sheetId="{s}" r:id="rId{s}"/>' for s in range(1,sheets+1))+'</sheets></workbook>')
        z.writestr('xl/_rels/workbook.xml.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'+''.join(f'<Relationship Id="rId{s}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{s}.xml"/>' for s in range(1,sheets+1))+'</Relationships>')
        z.writestr('xl/styles.xml','<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="0"/></cellXfs></styleSheet>')
        for s,start in enumerate(range(0,cells,per_sheet),1):
            with z.open(f'xl/worksheets/sheet{s}.xml','w') as f:
                f.write(b'<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>')
                for i in range(min(per_sheet,cells-start)):
                    row,col=divmod(i,50)
                    if col==0:f.write(f'<row r="{row+1}">'.encode())
                    addr=f'{column(col+1)}{row+1}'
                    if empty and i%10:f.write(f'<c r="{addr}" s="1"/>'.encode())
                    elif i%10==0:f.write(f'<c r="{addr}" t="inlineStr"><is><t>work {i}</t></is></c>'.encode())
                    else:f.write(f'<c r="{addr}"><v>{i/10:.1f}</v></c>'.encode())
                    if col==49 or i==min(per_sheet,cells-start)-1:f.write(b'</row>')
                f.write(b'</sheetData></worksheet>')

def run():
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--sizes',type=int,nargs='+',default=[500_000,1_500_000,3_000_000]);p.add_argument('--repeats',type=int,default=2);args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    results=[]
    for size in args.sizes:
        for empty in [False,True]:
            path=args.output/f'{size}-{"formatted" if empty else "values"}.xlsx';generate(path,size,empty)
            with zipfile.ZipFile(path) as z:components={i.filename:i.file_size for i in z.infolist()}
            for repeat in range(args.repeats):
                start=time.monotonic()
                with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
                    proc=subprocess.Popen([args.binary,str(path),str(args.output/'staging')],stdout=out,stderr=err)
                    _,status,usage=os.wait4(proc.pid,0);proc.returncode=os.waitstatus_to_exitcode(status)
                    out.seek(0);err.seek(0);stdout=out.read().decode();stderr=err.read().decode()
                record={'size':size,'formatted':empty,'repeat':repeat,'cache':'first invocation; OS cache not flushed' if repeat==0 else 'repeat','source_bytes':path.stat().st_size,'unique_text_values':min(size,250_000)//10,'numeric_values':0 if empty else size-size//10,'inline_text_values':size//10,'empty_formatted_cells':size-size//10 if empty else 0,'components':components,'wall_seconds':time.monotonic()-start,'exit_code':proc.returncode,'output':stdout,'error':stderr,'peak_rss_kib':usage.ru_maxrss,'user_seconds':usage.ru_utime,'system_seconds':usage.ru_stime}
                results.append(record);(args.output/'results.json').write_text(json.dumps(results,indent=2));print(size,empty,repeat,proc.returncode,round(record['wall_seconds'],2),flush=True)
if __name__=='__main__':run()
