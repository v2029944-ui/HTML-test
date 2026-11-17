"""Measure only the backfill subprocess, independent of fixture construction."""
import pathlib
import resource
import subprocess
import sys
import time

binary = pathlib.Path(__file__).resolve().parents[1] / "target/debug/examples/backfill"
start = time.monotonic()
subprocess.run([str(binary), sys.argv[1], "huge"], check=True)
print(f"elapsed_seconds={time.monotonic() - start:.3f} max_rss_kib={resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss}")
