'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const {prepareHtml}=require('../prepare.cjs');

test('generated frontend preserves original HTML and inserts bridge before inline startup',()=>{
  const html='<!doctype html><html><head></head><body><script>window.appReady=main();</script></body></html>';
  const out=prepareHtml(html);
  assert.equal(html.includes('tauri-bridge'),false);
  assert.ok(out.indexOf('tauri-bridge.js')<out.indexOf('window.appReady=main();'));
  assert.ok(out.indexOf('tauri-desktop.js')>out.indexOf('window.appReady=main();'));
  assert.throws(()=>prepareHtml(out),/уже присутствуют/);
});

test('bridge passes load, save and configured Excel directory operations to native commands',async()=>{
  const calls=[],window={__TAURI__:{
    core:{invoke:async(name,args)=>{calls.push([name,args]);return name==='list_excel'?{folder:'C:/Reports',files:[]}:name==='get_info'?{dataFile:'C:/Db/reports.sqlite3'}:{ok:true};}},
    dialog:{open:async()=> 'C:/Reports'}
  }};
  const context={window,setInterval:()=>11,clearInterval:()=>{}};
  const text=fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8');
  vm.runInNewContext(text,context);
  assert.equal(typeof window.desktopAPI.load,'function');
  await window.desktopAPI.load();
  await window.desktopAPI.save({reports:[],settings:{}});
  await window.desktopAPI.chooseFolder();
  await window.desktopAPI.setRule('Анализ.xlsx',{type:'week',start:'2026-10-19'});
  const methods=calls.map(x=>x[0]);
  assert.deepEqual(methods,['load_data','save_data','set_folder','set_rule']);
});

test('native command bridge is absent from the original browser-only HTML',()=>{
  const root=path.resolve(__dirname,'../..');
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');
  assert.equal(html.includes('tauri-bridge.js'),false);
  assert.ok(html.includes('indexedDB.open(DB_NAME,DB_VERSION)'));
});

test('Tauri build copies the current HTML-test interface and current Excel importer without divergence',()=>{
  const os=require('node:os');
  const root=path.resolve(__dirname,'../..');
  const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'tauri-parity-'));
  try{
    const {prepare}=require('../prepare.cjs');
    const prepared=prepare({base:root,output:tmp});
    const source=fs.readFileSync(path.join(root,'index.html'),'utf8');
    const desktop=fs.readFileSync(path.join(root,'desktop/renderer.js'),'utf8');
    const built=fs.readFileSync(path.join(tmp,'index.html'),'utf8');
    const adapted=fs.readFileSync(path.join(tmp,'tauri-desktop.js'),'utf8');
    assert.equal(built,prepareHtml(source),'Tauri must use the complete latest HTML');
    assert.equal(prepared.sourceBytes,Buffer.byteLength(source,'utf8'));
    assert.equal(adapted,desktop.replace(
      'Резервная копия: reports.json.bak. Данные не находятся в браузере.',
      'Используется SQLite. Исходный reports.json сохраняется после переноса; делайте отдельные резервные копии.'
    ),'Tauri must use the complete latest Excel folder importer');
    assert.ok(fs.existsSync(path.join(tmp,'tauri-bridge.js')));
  }finally{fs.rmSync(tmp,{recursive:true,force:true});}
});

test('all HTML-test reporting period types are supported by native Tauri SQLite',()=>{
  const root=path.resolve(__dirname,'../..');
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');
  const rust=fs.readFileSync(path.join(root,'tauri/src-tauri/src/storage.rs'),'utf8');
  const htmlList=html.match(/const REPORT_TYPES\s*=\s*\[([^\]]+)\]/);
  const rustList=rust.match(/const REPORT_TYPES:\s*\[&str;\s*\d+\]\s*=\s*\[([^\]]+)\]/);
  assert.ok(htmlList,'Cannot locate the HTML reporting types');
  assert.ok(rustList,'Cannot locate the native reporting types');
  const values=m=>[...m[1].matchAll(/['"]([^'"]+)['"]/g)].map(x=>x[1]).sort();
  assert.deepEqual(values(rustList),values(htmlList),'SQLite is missing a type supported by HTML');
  assert.ok(values(htmlList).includes('day'));
  assert.match(rust,/REPORT_TYPES\.contains\(&rule\.kind\.as_str\(\)\)/);
  assert.match(rust,/REPORT_TYPES\.contains\(&x\)/);
});

test('watched-folder importer suggests correct dates for daily, weekly and monthly Excel',()=>{
  const root=path.resolve(__dirname,'../..');
  const renderer=fs.readFileSync(path.join(root,'desktop/renderer.js'),'utf8');
  const fn=renderer.match(/function suggestedRule\(name\)\{([\s\S]*?)\n  \}\n  function dateInput/);
  assert.ok(fn,'Cannot find the shared importer date guess');
  const suggest=new Function('name',fn[1]);
  assert.equal(suggest('День-2026-11-16.xlsx').type,'day');
  assert.equal(suggest('Дневной-2026-11-16.xlsx').start,'2026-11-16');
  assert.equal(suggest('week_2026-11-09.xlsx').type,'week');
  assert.equal(suggest('Месяц-2026-11.xlsx').start,'2026-11-01');
  assert.match(renderer,/shiftPeriod\(rule\.type,rule\.start,i\)/);
});
