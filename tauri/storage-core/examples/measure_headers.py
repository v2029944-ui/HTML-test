"""Measure the separate Rust probe; check original JSON integrity afterwards."""
import hashlib
import pathlib
import sqlite3
import subprocess
import sys
import time

root = pathlib.Path(sys.argv[1])
binary = pathlib.Path(__file__).resolve().parents[1] / "target/debug/examples/headers"
start = time.monotonic()
subprocess.run([str(binary), str(root)], check=True)
print(f"elapsed_seconds={time.monotonic() - start:.3f}")
conn = sqlite3.connect(root / "reports.sqlite3")
raw = conn.execute("SELECT json FROM reports").fetchone()[0]
assert hashlib.sha256(raw.encode()).hexdigest() == (root / "expected-json-sha256").read_text()
assert conn.execute("SELECT COUNT(*) FROM reports").fetchone()[0] == 1
assert conn.execute("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('reports_by_source','reports_by_period')").fetchone()[0] == 0
print("original_json_sha256_preserved; report_count=1; oversized_lookup_indexes_not_created")
