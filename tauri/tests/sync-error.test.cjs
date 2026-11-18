'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');

function scheduleHarness(failAt){
  const source=fs.readFileSync(path.join(__dirname,'../folder-importer.js'),'utf8');
  const start=source.indexOf('  function scheduleSync(){');
  assert.ok(start>=0,'Folder watcher scheduling function must exist');
  const end=source.indexOf('\n  }',start);
  assert.ok(end>start,'Folder watcher scheduling function must close');
  let fire,cleared=false;
  const calls=[],notices=[];
  const context={scheduleTimer:null,
    clearTimeout:()=>{cleared=true;},
    setTimeout:(callback,delay)=>{assert.equal(delay,1800);fire=callback;return 17;},
    showFiles:async()=>{calls.push('showFiles');if(failAt==='showFiles')throw Error('Excel folder unavailable');},
    syncFiles:async()=>{calls.push('syncFiles');if(failAt==='syncFiles')throw Error('Excel sync unavailable');},
    message:(message,warning)=>notices.push({message,warning})
  };
  const schedule=vm.runInNewContext('('+source.slice(start,end+4).trim()+')',context);
  schedule();
  return {fire,calls,notices,cleared};
}

for(const stage of ['showFiles','syncFiles']){
  test('scheduled Excel folder refresh handles async '+stage+' failure',async()=>{
    const h=scheduleHarness(stage);
    assert.ok(h.cleared);
    await assert.doesNotReject(h.fire());
    assert.equal(h.notices.length,1);
    assert.equal(h.notices[0].warning,true);
    assert.match(h.notices[0].message,/Не удалось проверить изменения в папке Excel/);
    assert.match(h.notices[0].message,/unavailable/);
    assert.deepEqual(h.calls,stage==='showFiles'?['showFiles']:['showFiles','syncFiles']);
  });
}

test('successful scheduled Excel folder refresh keeps original operation order',async()=>{
  const h=scheduleHarness(null);
  await assert.doesNotReject(h.fire());
  assert.deepEqual(h.calls,['showFiles','syncFiles']);
  assert.equal(h.notices.length,0);
});

test('cancelled asynchronous Excel import clears pending-import counter',async()=>{
  const source=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8');
  const start=source.indexOf('async function withExcelImport(');
  assert.ok(start>=0);
  const end=source.indexOf('\n}',start);
  assert.ok(end>start,'withExcelImport must have a closing brace');
  const context={state:{}};
  const importGuard=vm.runInNewContext('('+source.slice(start,end+2).trim()+')',context);
  await assert.rejects(importGuard(()=>Promise.reject(Error('Импорт отменён'))),/Импорт отменён/);
  assert.equal(context.state.excelImports,0);
});
