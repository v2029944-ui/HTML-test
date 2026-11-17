"""Compare the unchanged bounded browser XLSX reader with a generated fixture.
Measures XLSX.read only; no report conversion and no Windows/WebView2 claim.
"""
import argparse,json,re,time
from pathlib import Path
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('--html',type=Path,default=Path('index.html'));a=p.parse_args()
script=re.findall(r'<script>(.*?)</script>',a.html.read_text(),re.S)[0]
with sync_playwright() as pw:
    browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True)
    page=browser.new_page();page.add_script_tag(content=script)
    # Route the local fixture through Playwright; no external network request.
    page.route('https://fixture.test/input.xlsx',lambda route:route.fulfill(path=str(a.source),content_type='application/octet-stream',headers={'Access-Control-Allow-Origin':'*'}))
    result=page.evaluate('''async()=>{const bytes=new Uint8Array(await(await fetch('https://fixture.test/input.xlsx')).arrayBuffer());const start=performance.now();try{const book=XLSX.read(bytes);return {ok:true,milliseconds:performance.now()-start,sheets:book.SheetNames.length,stats:book.readStats};}catch(error){return {ok:false,milliseconds:performance.now()-start,error:error.message};}}''')
    print(json.dumps(result,ensure_ascii=False));browser.close()
