"""Generate original-schema data before the separately measured Rust process."""
import json,pathlib,sqlite3,sys
root=pathlib.Path(sys.argv[1]);n=int(sys.argv[2]);root.mkdir(parents=True,exist_ok=False)
f=root/'settings.json'
with f.open('w') as out:
 out.write('{"orgNames":["A","B","C","D"],"unknown":{"nested":[1,{"keep":true}]},"groups":[{"id":"dup","name":"Empty","works":[],"unknown":[1,2]},{"id":"dup","name":"Big","metadata":{"keep":"unchanged"},"works":[')
 for i in range(n):
  if i:out.write(',')
  out.write(json.dumps(f'member-{i}'))
 out.write(']}],"catalog":[')
 for i in range(n):
  if i:out.write(',')
  out.write(json.dumps({'work':f'catalog-{i}','organization':'A' if i%2==0 else 'B','unknown':{'flags':[1,{'x':True}]}},separators=(',',':')))
 out.write(']}')
c=sqlite3.connect(root/'reports.sqlite3');c.executescript('CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL);CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);')
r={'id':'keep','type':'day','start':'2026-11-01','end':'2026-11-01','amount':30,'organizations':['A','B'],'rows':[{'organization':'A','work':'from report','count':2,'amount':30}]}
c.execute("INSERT INTO reports VALUES('keep','day','2026-11-01','2026-11-01',NULL,?)",(json.dumps(r).encode(),));c.execute("INSERT INTO kv VALUES('settings',?)",(f.read_text(),));c.commit();c.close();size=f.stat().st_size;f.unlink();(root/'probe.json').write_text(json.dumps({'items':n,'settings_bytes':size}));print(json.dumps({'items':n,'settings_bytes':size}))
