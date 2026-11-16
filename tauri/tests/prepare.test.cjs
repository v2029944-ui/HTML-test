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
