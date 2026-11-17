"""Profile raw staging plus daily-wide conversion, without GUI or live application.

Fixture generation is outside the measured Rust child. First run is not a cold
disk-cache measurement. VmHWM is read inside the child, avoiding inherited parent
ru_maxrss after generating large fixtures. XML statistics use iterparse/row clear.
"""
import argparse, io, json, subprocess, sys, time, zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tests'))
from test_daily_wide import wide_xlsx


def col(n):
    out = ''
    while n:
        n, r = divmod(n - 1, 26)
        out = chr(65 + r) + out
    return out


def generate(path, target, formatted):
    # Four quarter components keep each XML below the unchanged 48 MiB cap.
    # Approximately 2,976 cells per work; all counts verified from actual XML.
    works = 1 if formatted else max(1, (target - 12_000) // 2976)
    payload = wide_xlsx(2024, quarter_sheets=True, works_per_org=works)
    with zipfile.ZipFile(io.BytesIO(payload)) as z:
        base = {i.filename: z.read(i.filename) for i in z.infolist()}
    cells = sum(v.count(b'<c ') for k, v in base.items() if '/worksheets/' in k)
    if cells > target:
        raise ValueError((cells, target))
    # Formatting-only worksheets are intentionally not selected as daily reports.
    # They exercise empty-cell raw ingestion, without inventing business data.
    extra = target - cells
    count = (extra + 374_999) // 375_000
    wb = base['xl/workbook.xml'].decode().replace('</sheets>', ''.join(
        f'<sheet name="Formatting{i}" sheetId="{i+4}" r:id="rId{i+4}"/>'
        for i in range(1, count + 1)) + '</sheets>')
    rel = base['xl/_rels/workbook.xml.rels'].decode().replace('</Relationships>', ''.join(
        f'<Relationship Id="rId{i+4}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/format{i}.xml"/>'
        for i in range(1, count + 1)) + '</Relationships>')
    base['xl/workbook.xml'] = wb.encode()
    base['xl/_rels/workbook.xml.rels'] = rel.encode()
    types = base['[Content_Types].xml'].decode().replace('</Types>', ''.join(
        f'<Override PartName="/xl/worksheets/format{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        for i in range(1, count + 1)) + '</Types>')
    base['[Content_Types].xml'] = types.encode()
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
        for name, value in base.items():
            z.writestr(name, value)
        for i in range(1, count + 1):
            n = min(extra, 375_000)
            extra -= n
            with z.open(f'xl/worksheets/format{i}.xml', 'w') as out:
                out.write(b'<worksheet><sheetData>')
                for cell in range(n):
                    r, c = divmod(cell, 1250)
                    if c == 0:
                        out.write(f'<row r="{r+1}">'.encode())
                    out.write(f'<c r="{col(c+1)}{r+1}" s="0"/>'.encode())
                    if c == 1249 or cell == n - 1:
                        out.write(b'</row>')
                out.write(b'</sheetData></worksheet>')
    return works


def stats(path):
    result = dict(cells=0, value_cells=0, empty_formatted_cells=0)
    unique = set()  # fixture statistics only, not part of Rust memory evidence
    with zipfile.ZipFile(path) as z:
        result['components'] = {i.filename: i.file_size for i in z.infolist()}
        for name in z.namelist():
            if '/worksheets/' not in name:
                continue
            with z.open(name) as src:
                for event, node in ET.iterparse(src, events=['end']):
                    tag = node.tag.rsplit('}', 1)[-1]
                    if tag == 'c':
                        result['cells'] += 1
                        values = [x.text for x in node.iter() if x.tag.rsplit('}', 1)[-1] in ['t', 'v'] and x.text is not None]
                        result['value_cells'] += bool(values)
                        result['empty_formatted_cells'] += not values and 's' in node.attrib
                        if node.attrib.get('t') == 'inlineStr':
                            unique.add(''.join(values))
                        node.clear()
                    elif tag == 'row':
                        node.clear()
    result.update(source_bytes=path.stat().st_size, unpacked_bytes=sum(result['components'].values()), unique_strings=len(unique))
    return result


def generate_stress(path, diagnostics):
    """Maximum-height reports plus 50k disk-backed styles; optional issue storm."""
    with zipfile.ZipFile(io.BytesIO(wide_xlsx(2024))) as z:
        base = {i.filename: z.read(i.filename) for i in z.infolist()}
    base['xl/styles.xml'] = b'<styleSheet><cellXfs>' + b'<xf numFmtId="0"/>' * 50_000 + b'</cellXfs></styleSheet>'
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
        for name, value in base.items():
            if '/worksheets/' not in name:
                z.writestr(name, value)
        with z.open('xl/worksheets/sheet1.xml', 'w') as out:
            out.write(b'<worksheet><sheetData>')
            def row(r, values):
                from xml.sax.saxutils import escape
                out.write(f'<row r="{r}">'.encode())
                for i, value in enumerate(values, 1):
                    out.write(f'<c r="{col(i)}{r}" t="inlineStr"><is><t>{escape(str(value))}</t></is></c>'.encode())
                out.write(b'</row>')
            row(1, ['Работа', '2024-02-28', '', '2024-02-29', ''])
            row(2, ['', 'Количество', 'Сумма', 'Количество', 'Сумма'])
            row(3, ['СМУ16', 59996, 59996, 59996, 59996])
            for r in range(4, 60000):
                value = 'oops' if diagnostics and r > 4 else 1
                row(r, ['Work '+str(r)+'x'*64, value, 1, value, 1])
            row(60000, ['Итого', 59996, 59996, 59996, 59996])
            out.write(b'</sheetData><mergeCells><mergeCell ref="A1:A2"/><mergeCell ref="B1:C1"/><mergeCell ref="D1:E1"/></mergeCells></worksheet>')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--binary', required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--sizes', nargs='+', type=int, default=[500_000, 1_500_000, 3_000_000])
    p.add_argument('--variants', nargs='+', choices=['values', 'formatted'], default=['values', 'formatted'])
    p.add_argument('--repeats', type=int, default=2)
    p.add_argument('--stress-only', action='store_true')
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    records = []
    jobs = [(s, v) for s in args.sizes for v in args.variants] if not args.stress_only else [(300000, 'large-report'), (300000, 'diagnostics')]
    for size, variant in jobs:
            path = args.output / f'wide-{size}-{variant}.xlsx'
            if args.stress_only:
                generate_stress(path, variant == 'diagnostics')
                works = 59996
            else:
                works = generate(path, size, variant == 'formatted')
            info = stats(path)
            assert info['cells'] == size
            for repeat in range(args.repeats):
                start = time.monotonic()
                proc = subprocess.run([args.binary, str(path), str(args.output / 'staging'), 'wide', '{}', '--summary-only'], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
                record = dict(target=size, variant=variant, works_per_organization=works, repeat=repeat,
                              cache='first invocation; OS cache not flushed' if repeat == 0 else 'repeat; OS cache not flushed',
                              wall_seconds=time.monotonic()-start, exit_code=proc.returncode, **info)
                lines = proc.stderr.strip().splitlines()
                record['metrics'] = json.loads(lines[-1]) if proc.returncode == 0 else None
                record['error'] = proc.stderr if proc.returncode else None
                records.append(record)
                (args.output / 'results.json').write_text(json.dumps(records, indent=2))
                print(size, variant, repeat, proc.returncode, round(record['wall_seconds'], 3), record['metrics'], flush=True)
                if proc.returncode:
                    raise RuntimeError(proc.stderr)


if __name__ == '__main__':
    main()
