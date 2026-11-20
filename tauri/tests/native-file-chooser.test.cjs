'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8');
function fixture(){
 const timers=[],calls=[],listeners={},document={activeElement:null,addEventListener:(name,fn)=>{(listeners[name]??=[]).push(fn);}};
 const trigger={tagName:'LABEL',isConnected:true,disabled:false,attributes:new Map(),control:{disabled:false},clicks:0,
  closest(selector){return selector==='label'?this:this;},getClientRects(){return [{}];},
  getAttribute(name){return this.attributes.has(name)?this.attributes.get(name):null;},
  setAttribute(name,value){this.attributes.set(name,String(value));},removeAttribute(name){this.attributes.delete(name);},
  hasAttribute(name){return this.attributes.has(name);},addEventListener(){},
  focus(){document.activeElement=this;},click(){this.clicks++;}};
 const window={document,__TAURI__:{core:{invoke:async(command,args)=>{calls.push({command,args});return null;}},dialog:{open:async()=>null,save:async()=>null}},dispatchEvent(){}};
 class CustomEvent{constructor(type,value){this.type=type;this.detail=value.detail;}}
 vm.runInNewContext(script,{window,CustomEvent,setTimeout:fn=>timers.push(fn)});
 const fire=(name,extra={})=>{const event={target:trigger,preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.stopped=true;},...extra};for(const fn of listeners[name]||[])fn(event);return event;};
 return {api:window.desktopAPI,window,document,trigger,calls,timers,fire,flush:()=>{while(timers.length)timers.shift()();}};
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
