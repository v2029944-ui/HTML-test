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
