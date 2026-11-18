'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
function bridge(){
 const pending=[],window={__TAURI__:{core:{invoke:()=>new Promise((resolve,reject)=>pending.push({resolve,reject}))},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 return {api:window.desktopAPI,ops:window.tauriOperations,pending};
}
test('native saves are counted until completion, including parallel failures',async()=>{
 const f=bridge(),one=f.api.save({}),two=f.api.setRule('a.xlsx',{});assert.equal(f.ops.saves,2);
 f.pending[0].resolve({});await one;assert.equal(f.ops.saves,1);
 f.pending[1].reject(Error('disk'));await assert.rejects(two,/disk/);assert.equal(f.ops.saves,0);
});
test('native Excel reads block updates until their promise settles',async()=>{
 const f=bridge(),read=f.api.readExcel('a.xlsx');assert.equal(f.ops.reads,1);f.pending[0].resolve({bytes:[]});await read;assert.equal(f.ops.reads,0);
 f.ops.updating=true;await assert.rejects(f.api.readExcel('b.xlsx'),/обновления/);await assert.rejects(f.api.save({}),/обновления/);assert.equal(f.pending.length,1);
});
test('shared Excel lifetime guard restores counters on error and refuses import during update',async()=>{
 const html=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8'),start=html.indexOf('async function withExcelImport('),end=html.indexOf('\n}',start)+2;
 const context={state:{}},run=vm.runInNewContext('('+html.slice(start,end)+')',context);let resolve;
 const pending=run(()=>new Promise(r=>resolve=r));assert.equal(context.state.excelImports,1);resolve();await pending;assert.equal(context.state.excelImports,0);
 await assert.rejects(run(()=>{throw Error('parse');}),/parse/);assert.equal(context.state.excelImports,0);
 context.state.appUpdating=true;await assert.rejects(run(()=>{}),/обновления/);
});

test('native report browsing holds a read lease, forwards bounded cursor, and blocks updates',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const revision=window.desktopAPI.nativeRevision();
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[0].command,'native_read_revision');
 pending[0].resolve(7);assert.equal(await revision,7);
 const page=window.desktopAPI.nativeReportPage(7,{report_type:'day'},{start:'2026-11-01',id:'r1'},25);
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[1].command,'native_report_page');
 assert.equal(calls[1].args.revision,7);
 assert.equal(calls[1].args.filter.report_type,'day');
 assert.equal(calls[1].args.after.id,'r1');
 assert.equal(calls[1].args.limit,25);
 pending[1].reject(Error('Stale storage revision'));
 await assert.rejects(page,/Stale/);
 assert.equal(window.tauriOperations.reads,0);
 window.tauriOperations.updating=true;
 await assert.rejects(window.desktopAPI.nativeRevision(),/обновления/);
 assert.equal(calls.length,2);
});
