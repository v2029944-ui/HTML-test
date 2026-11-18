'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');

test('folder watcher does not notify after unsubscribe while scan is in flight',async()=>{
  let calls=0,resolvePending,poll,changes=0;
  const window={__TAURI__:{
    core:{invoke:async name=>{
      if(name!=='list_excel')return {ok:true};
      if(++calls===1)return {folder:'C:/Reports',files:[{name:'Day.xlsx',signature:'old'}]};
      return new Promise(resolve=>{resolvePending=resolve;});
    }},
    dialog:{open:async()=>null}
  }};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{
    window,setInterval:fn=>{poll=fn;return 1;},clearInterval:()=>{}
  });
  const unsubscribe=window.desktopAPI.onFilesChanged(()=>changes++);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(changes,0);
  const pendingPoll=poll();
  assert.equal(calls,2);
  unsubscribe();
  resolvePending({folder:'C:/Reports',files:[{name:'Day.xlsx',signature:'new'}]});
  await pendingPoll;
  assert.equal(changes,0,'Unsubscribed watcher must not emit delayed change');
});
