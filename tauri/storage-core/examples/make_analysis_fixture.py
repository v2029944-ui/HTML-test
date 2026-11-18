"""Fixture generation completes before the independently measured Rust process."""
import datetime,json,pathlib,sqlite3,sys
root=pathlib.Path(sys.argv[1]);n=int(sys.argv[2]);history=int(sys.argv[3]);root.mkdir(parents=True,exist_ok=False)
c=sqlite3.connect(root/'reports.sqlite3');c.executescript('CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL);CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);')
last=datetime.date(2026,11,1);first=last-datetime.timedelta(days=history)
for i in range(history):
 d=str(first+datetime.timedelta(days=i));r={'id':f'h{i:06}','type':'day','start':d,'end':d,'organizations':['A','B'],'rows':[{'organization':'A','work':'W0','count':1,'amount':1}]};raw=json.dumps(r,separators=(',',':'));c.execute('INSERT INTO reports VALUES(?,?,?,?,NULL,?)',(r['id'],'day',d,d,raw.encode() if i%2 else raw))
p=root/'huge.json'
with p.open('w') as out:
 out.write('{"id":"huge","type":"day","start":"2026-11-01","end":"2026-11-01","organizations":["A","B"],"metadata":[')
 for i in range(n):
  if i:out.write(',')
  out.write('[1,2,3]')
 out.write('],"rows":[')
 for i in range(n):
  if i:out.write(',')
  out.write(json.dumps({'organization':'A','work':f'W{i}','count':1,'amount':2,'unknown':[1,2,3]},separators=(',',':')))
 out.write(']}')
c.execute("INSERT INTO reports VALUES('huge','day','2026-11-01','2026-11-01',NULL,?)",(p.read_text(),));size=p.stat().st_size;p.unlink()
# Settings ordering is intentionally works-before-name/id. Catalog ignored by metrics.
settings={'catalog':[{'work':f'catalog-{i}','unknown':[1,2,3]} for i in range(n)],'groups':[{'works':[f'W{i}'],'name':f'G{i}','id':f'g{i}'} for i in range(n//10)],'orgNames':['A','B','C','D']}
c.execute("INSERT INTO kv VALUES('settings',?)",(json.dumps(settings,separators=(',',':')),));c.commit();c.close();print(json.dumps({'rows':n,'history':history,'large_json_bytes':size,'from':str(first),'to':str(last)}))
(root/'probe.json').write_text(json.dumps({'rows':n,'history':history,'from':str(first),'to':str(last)}))
