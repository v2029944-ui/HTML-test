"""Cancel large native read/conversion phases; no live database or UI involved."""
import argparse, json, subprocess, time
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--fixtures', required=True, type=Path)
p.add_argument('--output', required=True, type=Path)
a = p.parse_args()
records = []
for size in [500_000, 1_500_000, 3_000_000]:
    for variant in ['values', 'formatted']:
        source = a.fixtures / f'wide-{size}-{variant}.xlsx'
        for phase in ['read', 'conversion']:
            start = time.monotonic()
            child = subprocess.run([a.binary, str(source), str(a.fixtures / 'cancel-staging'), phase], capture_output=True, text=True)
            if child.returncode:
                raise RuntimeError(child.stderr)
            record = dict(size=size, variant=variant, wall_seconds=time.monotonic()-start, **json.loads(child.stdout))
            records.append(record)
            a.output.write_text(json.dumps(records, indent=2))
            print(json.dumps(record), flush=True)
