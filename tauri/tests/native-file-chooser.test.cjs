'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8');
function fixture(confirm){
 const timers=[],observers=[],calls=[],listeners={},document={activeElement:null,addEventListener:(name,fn)=>{(listeners[name]??=[]).push(fn);}};
 const trigger={tagName:'LABEL',isConnected:true,disabled:false,attributes:new Map(),control:{disabled:false},clicks:0,events:{},
  closest(selector){return selector==='label'?this:this;},getClientRects(){return [{}];},
  getAttribute(name){return this.attributes.has(name)?this.attributes.get(name):null;},
  setAttribute(name,value){this.attributes.set(name,String(value));},removeAttribute(name){this.attributes.delete(name);},
  hasAttribute(name){return this.attributes.has(name);},addEventListener(name,fn){this.events[name]=fn;},
  focus(){document.activeElement=this;},click(){this.clicks++;}};
 const window={document,confirm,__TAURI__:{core:{invoke:async(command,args)=>{calls.push({command,args});return null;}},dialog:{open:async()=>null,save:async()=>null}},dispatchEvent(){}};
 class CustomEvent{constructor(type,value){this.type=type;this.detail=value.detail;}}
 class MutationObserver{constructor(callback){this.callback=callback;this.active=true;observers.push(this);}observe(){}disconnect(){this.active=false;}}
 const context={window,document,CustomEvent,MutationObserver,
  setTimeout:(fn,delay=0)=>{const timer={fn,delay,active:true};timers.push(timer);return timer;},
  clearTimeout:timer=>{if(timer)timer.active=false;},setInterval:()=>1,clearInterval(){}};
 vm.runInNewContext(script,context);
 const fire=(name,extra={})=>{const event={target:trigger,preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.stopped=true;},...extra};for(const fn of listeners[name]||[])fn(event);return event;};
 return {api:window.desktopAPI,window,document,trigger,calls,timers,fire,context,
  mutate:()=>{for(const observer of observers)if(observer.active)observer.callback();},
  flush:(delay=0)=>{let timer;while((timer=timers.find(t=>t.active&&t.delay<=delay))){timer.active=false;timer.fn();}}};
}
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
test('cancelled native picker restores its visible label after controller unlock and supports keyboard reopen',async()=>{
 const f=fixture(),answer=deferred();f.window.__TAURI__.dialog.open=()=>answer.promise;f.fire('click');f.trigger.control.disabled=true;
 const pending=f.api.chooseWideXlsx();assert.equal(f.trigger.getAttribute('aria-busy'),'true');assert.equal(f.trigger.getAttribute('aria-disabled'),'true');
 answer.resolve(null);assert.equal(await pending,null);assert.equal(f.document.activeElement,null);f.trigger.control.disabled=false;f.flush();
 assert.equal(f.document.activeElement,f.trigger);assert.equal(f.trigger.getAttribute('aria-disabled'),null);assert.equal(f.trigger.getAttribute('tabindex'),'0');
 const key=f.fire('keydown',{key:'Enter'});assert.equal(key.prevented,true);assert.equal(f.trigger.clicks,1);
});
test('native picker refuses concurrent dialogs and keeps unrelated clicks from entering controllers',async()=>{
 const f=fixture(),answer=deferred();let opens=0;f.window.__TAURI__.dialog.open=()=>{opens++;return answer.promise;};f.fire('click');const pending=f.api.chooseWideXlsx();
 await assert.rejects(f.api.chooseGroupsImport(),/открытый диалог/);const event=f.fire('click');assert.equal(event.prevented,true);assert.equal(event.stopped,true);assert.equal(opens,1);
 answer.resolve(null);await pending;f.flush();f.window.__TAURI__.dialog.open=async()=>null;await f.api.chooseGroupsImport();f.flush();assert.equal(f.document.activeElement,f.trigger);
});
test('save cancellation and dialog rejection retain original errors and restore focus without IPC writes',async()=>{
 const f=fixture();f.fire('click');await f.api.chooseSqliteBackupPath('backup.sqlite3');f.flush();assert.equal(f.document.activeElement,f.trigger);assert.equal(f.calls.length,0);
 f.document.activeElement=null;f.window.__TAURI__.dialog.open=async()=>{throw Error('native chooser failed');};
 await assert.rejects(f.api.chooseRestoreBackupPath(),/native chooser failed/);f.flush();assert.equal(f.document.activeElement,f.trigger);assert.equal(f.trigger.getAttribute('aria-busy'),null);
});
test('cancelled folder chooser never changes the folder and detached openers are not refocused',async()=>{
 const f=fixture();f.fire('click');await f.api.chooseFolder();f.trigger.isConnected=false;f.flush();assert.equal(f.calls.length,0);assert.equal(f.document.activeElement,null);
});
test('a successful picker does not override the controllers next-step focus',async()=>{
 const f=fixture();f.fire('click');f.window.__TAURI__.dialog.open=async()=>'/synthetic/source.json';assert.equal(await f.api.chooseMetricJson(),'/synthetic/source.json');f.flush();assert.equal(f.document.activeElement,null);
});
test('native confirmation restores a cancelled action after unlock and preserves the accepted route',async()=>{
 const answer=deferred(),f=fixture(()=>answer.promise);f.fire('click');
 f.trigger.disabled=true;const pending=f.window.confirm('Подтвердить действие?');
 answer.resolve(false);assert.equal(await pending,false);f.trigger.disabled=false;f.flush();
 assert.equal(f.document.activeElement,f.trigger);assert.equal(f.calls.length,0);
 const accepted=fixture(()=>true);accepted.fire('click');
 assert.equal(accepted.window.confirm('Подтвердить действие?'),true);
 accepted.flush();assert.equal(accepted.document.activeElement,null);
});
test('real folder controller restores focus only after its asynchronous cancellation reload unlocks',async()=>{
 const f=fixture(),reload=deferred(),listing={folder:null,revision:12,files:[],rules:{}};
 f.trigger.tagName='BUTTON';delete f.trigger.control;
 const ids=['sourceFolderChoose','sourceFolderRefresh','sourceFolderLocation','sourceFolderStatus','sourceFolderRows','sourceFolderCount','sourceRuleFile','sourceRuleFormat','sourceRuleType','sourceRuleStart','sourceRuleSheet','sourceRuleYear','sourceRuleSave','sourceRuleStatus'];
 const node=()=>({disabled:false,value:'',textContent:'',events:{},children:[],
  addEventListener(name,fn){this.events[name]=fn;},setAttribute(){},replaceChildren(...items){this.children=items;},append(...items){this.children.push(...items);}});
 const nodes=Object.fromEntries(ids.map(id=>[id,id==='sourceFolderChoose'?f.trigger:node()]));
 f.document.getElementById=id=>nodes[id];f.document.createElement=node;
 let lists=0;
 f.window.__TAURI__.core.invoke=async command=>{f.calls.push(command);assert.equal(command,'list_excel');return ++lists<=2?listing:reload.promise;};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-folder-status.js'),'utf8'),f.context);
 await f.window.nativeFolderReady;f.fire('click');const pending=f.trigger.events.click();
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(lists,3);assert.equal(f.trigger.disabled,true);f.flush();assert.equal(f.document.activeElement,null);
 reload.resolve(listing);await pending;assert.equal(f.trigger.disabled,false);f.mutate();
 assert.equal(f.document.activeElement,f.trigger);assert.equal(f.calls.every(command=>command==='list_excel'),true);
});
test('file, backup, group and metric cancellation waits for each controllers real async release',async()=>{
 for(const method of ['chooseWideXlsx','chooseRestoreBackupPath','chooseSqliteBackupPath','chooseGroupsImport','chooseGroupsExport','chooseMetricJson','chooseMetricXlsx']){
  const f=fixture(),release=deferred();f.fire('click');f.trigger.control.disabled=true;
  const pending=(async()=>{try{await f.api[method]();await release.promise;}finally{f.trigger.control.disabled=false;}})();
  await new Promise(resolve=>setImmediate(resolve));f.flush();assert.equal(f.document.activeElement,null,method);
  release.resolve();await pending;f.mutate();assert.equal(f.document.activeElement,f.trigger,method);
 }
});
test('a new click, pointer action or keyboard action cancels delayed restoration',async()=>{
 for(const event of ['click','pointerdown','keydown']){
  const f=fixture();f.fire('click');f.trigger.control.disabled=true;await f.api.chooseWideXlsx();f.flush();
  const current={closest:()=>current};f.document.activeElement=current;
  f.fire(event,{target:current,key:'Tab'});f.trigger.control.disabled=false;f.mutate();f.flush();
  assert.equal(f.document.activeElement,current,event);
 }
});
test('controller next-step focus is preserved while cancelled picker release is pending',async()=>{
 const f=fixture();f.fire('click');f.trigger.control.disabled=true;await f.api.chooseWideXlsx();f.flush();
 const next={};f.document.activeElement=next;f.trigger.control.disabled=false;f.mutate();assert.equal(f.document.activeElement,next);
});
test('restoration waits for an aria-disabled label to be unlocked by its controller',async()=>{
 const f=fixture();f.fire('click');f.trigger.setAttribute('aria-disabled','true');await f.api.chooseGroupsImport();f.flush();
 assert.equal(f.document.activeElement,null);f.trigger.removeAttribute('aria-disabled');f.mutate();assert.equal(f.document.activeElement,f.trigger);
});
