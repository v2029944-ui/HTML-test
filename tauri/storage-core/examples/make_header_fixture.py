"""Original-schema oversized header fixture; finish before measuring the Rust process."""
import hashlib
import json
import pathlib
import sqlite3
import sys

root = pathlib.Path(sys.argv[1])
size = int(sys.argv[2])
column = sys.argv[3]
assert column in ("id", "report_type", "period_start", "period_end", "source_name")
root.mkdir(parents=True, exist_ok=False)
fields = {"id": "r1", "report_type": "week", "period_start": "2026-11-02", "period_end": "2026-11-08", "source_name": "source.xlsx"}
fields[column] = "z" * size
report = {"id": fields["id"], "type": fields["report_type"], "start": fields["period_start"], "end": fields["period_end"], "desktopSource": fields["source_name"], "rows": []}
raw = json.dumps(report, separators=(",", ":"))
conn = sqlite3.connect(root / "reports.sqlite3")
# No optional secondary indexes: exercises first-open index creation safeguards.
conn.executescript("""CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,
period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL);
CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);""")
conn.execute("INSERT INTO reports VALUES(?,?,?,?,?,?)", (*fields.values(), raw))
conn.commit()
conn.close()
(root / "expected-json-sha256").write_text(hashlib.sha256(raw.encode()).hexdigest())
print(f"column={column} field_bytes={size} json_bytes={len(raw.encode())}")
