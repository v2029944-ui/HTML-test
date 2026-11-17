"""Opt-in local XLSX profiling, without saving reports or changing import limits.

Run: python tests/profile_xlsx.py --works-per-org 125 500 --output /tmp/xlsx-profile.json
An optional --excel PATH profiles a private legacy workbook; its data are not exported.
RSS sampling is Linux-only and includes the browser/Playwright child processes,
including shared pages; it is not private memory or a Windows WebView2 measurement.
"""
import argparse
import base64
import io
import json
import os
import threading
import time
import zipfile
from pathlib import Path
from playwright.sync_api import sync_playwright
from test_daily_wide import wide_xlsx

ROOT = Path(__file__).resolve().parents[1]


def child_rss_kib():
    if not Path('/proc').exists():
        return None
    processes = {}
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = dict(line.split(':', 1) for line in (entry / 'status').read_text().splitlines() if ':' in line)
            processes[int(entry.name)] = (int(fields['PPid']), int(fields.get('VmRSS', '0 kB').split()[0]))
        except (OSError, KeyError, ValueError):
            continue
    owned = {os.getpid()}
    while True:
        expanded = owned | {pid for pid, (parent, _) in processes.items() if parent in owned}
        if expanded == owned:
            break
        owned = expanded
    return sum(rss for pid, (_, rss) in processes.items() if pid in owned and pid != os.getpid())


def profile(browser, payload, kind, label):
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        expanded = sum(entry.file_size for entry in archive.infolist())
        largest = max(entry.file_size for entry in archive.infolist())
    context = browser.new_context()
    context.route('**/*', lambda route: route.abort())
    page = context.new_page()
    try:
        # Same in-memory offline origin as the browser suite (file:// navigation
        # may be disabled by the managed test browser's enterprise policy).
        page.route('https://xlsx-profile.test/', lambda route: route.fulfill(
            content_type='text/html', body=(ROOT / 'index.html').read_text()))
        page.goto('https://xlsx-profile.test/')
        page.evaluate('window.appReady')
        session = context.new_cdp_session(page)
        session.send('Performance.enable')
        baseline = child_rss_kib()
        samples = [baseline] if baseline is not None else []
        stop = threading.Event()

        def sample():
            while not stop.wait(.1):
                value = child_rss_kib()
                if value is not None:
                    samples.append(value)

        monitor = threading.Thread(target=sample, daemon=True)
        monitor.start()
        try:
            result = page.evaluate('''({encoded,kind})=>{
              const bytes=Uint8Array.from(atob(encoded),c=>c.charCodeAt(0));
              const start=performance.now();let readMs=null;
              try{
                const book=XLSX.read(bytes);readMs=performance.now()-start;
                const parsed=kind==='daily'?parseDailyWorkbook(book,{fileName:'Synthetic.xlsx'}).reports:
                  book.SheetNames.filter(name=>readTripleColumns(book.Sheets[name]).length).flatMap(name=>parseSheet(book.Sheets[name]));
                return {readMs,parseMs:performance.now()-start-readMs,stats:book.readStats,
                  reports:parsed.length,rows:parsed.reduce((n,r)=>n+r.rows.length,0),
                  count:parsed.reduce((n,r)=>n+r.count,0),amount:parsed.reduce((n,r)=>n+r.amount,0),
                  warnings:parsed.reduce((n,r)=>n+r.warnings.length,0),
                  catalog:kind==='legacy'?detectCatalogWorkbook(book).length:0};
              }catch(e){return {error:e.message,readMs,totalMs:performance.now()-start};}
            }''', {'encoded': base64.b64encode(payload).decode(), 'kind': kind})
        finally:
            stop.set()
            monitor.join()
        metrics = {m['name']: m['value'] for m in session.send('Performance.getMetrics')['metrics']}
        result.update(label=label, compressedBytes=len(payload), expandedBytes=expanded,
                      largestXmlBytes=largest, sampledChildRssBaselineKiB=baseline,
                      sampledChildRssPeakKiB=max(samples) if samples else None,
                      jsHeapUsedBytesAfter=metrics.get('JSHeapUsedSize'))
        return result
    finally:
        context.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--works-per-org', nargs='+', type=int, default=[125])
    parser.add_argument('--excel', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--chromium', default='/usr/bin/chromium')
    args = parser.parse_args()
    if any(n < 1 or n > 500 for n in args.works_per_org):
        parser.error('Use 1..500 works per organization; this profiler does not disable safety guards.')
    results = []
    with sync_playwright() as pw:
        for n in args.works_per_org:
            payload = wide_xlsx(2024, works_per_org=n)
            browser = pw.chromium.launch(executable_path=args.chromium, headless=True)
            try:
                result = profile(browser, payload, 'daily', f'synthetic-year-2024-{n*4}-services')
                if 'error' not in result:
                    assert result['reports'] == 366
                    assert result['rows'] == n*4*366
                    assert result['count'] == n*2*366
                    assert result['amount'] == n*20*366
                results.append(result)
                print(json.dumps(result, ensure_ascii=False), flush=True)
            finally:
                browser.close()
        if args.excel:
            browser = pw.chromium.launch(executable_path=args.chromium, headless=True)
            try:
                result = profile(browser, args.excel.read_bytes(), 'legacy', 'private-legacy-workbook')
                results.append(result)
                print(json.dumps(result, ensure_ascii=False), flush=True)
            finally:
                browser.close()
    args.output.write_text(json.dumps({'chromium': 'local Linux Chromium', 'results': results}, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    main()
