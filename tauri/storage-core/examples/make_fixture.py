"""Build an original-schema SQLite fixture; run outside the measured backfill process."""
import json
import pathlib
import sqlite3
import sys

root = pathlib.Path(sys.argv[1])
count = int(sys.argv[2])
root.mkdir(parents=True, exist_ok=False)
path = root / "fixture.json"
with path.open("w", encoding="utf-8") as out:
    out.write('{"id":"huge","type":"day","start":"2026-11-01","end":"2026-11-01","metadata":[')
    for index in range(count):
        if index:
            out.write(",")
        out.write("[1,2,3]")
    out.write('],"rows":[')
    row = json.dumps({"organization": "A", "work": "W", "count": 1, "amount": 2, "unknown": [1, 2, 3]}, separators=(",", ":"))
    for index in range(count):
        if index:
            out.write(",")
        out.write(row)
    out.write("]}")
conn = sqlite3.connect(root / "reports.sqlite3")
conn.executescript("""
CREATE TABLE reports(id TEXT PRIMARY KEY, report_type TEXT NOT NULL,
period_start TEXT NOT NULL, period_end TEXT NOT NULL, source_name TEXT, json TEXT NOT NULL);
CREATE TABLE kv(name TEXT PRIMARY KEY, json TEXT NOT NULL);
""")
conn.execute("INSERT INTO reports VALUES('huge','day','2026-11-01','2026-11-01',NULL,?)", (path.read_text(),))
conn.commit()
conn.close()
print(f"rows={count} json_bytes={path.stat().st_size}")
path.unlink()
