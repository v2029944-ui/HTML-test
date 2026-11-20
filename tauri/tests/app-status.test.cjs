'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const bridge=fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8');
const filename=path.join(__dirname,'../app-status.js');
const statusSource=()=>fs.existsSync(filename)?fs.readFileSync(filename,'utf8'):'';
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};}
function setup({mounts=true,version=deferred(),operations=null,observerThrows=false}={}){
 const ids={},listeners={},events=[],pending=[];
 function node(tag='div'){
  return {tagName:tag.toUpperCase(),children:[],attributes:{},hidden:false,textContent:'',className:'',
   append(...children){this.children.push(...children);for(const child of children)if(child.id)ids[child.id]=child;},
   setAttribute(k,v){this.attributes[k]=String(v);},removeAttribute(k){delete this.attributes[k];}};
 }
 const main=node('main'),header=node('header'),headerActions=node('div');header.append(headerActions);
 if(mounts)for(const id of ['appVersion','appActivity','appActivityText']){ids[id]=node();ids[id].id=id;}
 const document={getElementById:id=>ids[id]||null,createElement:node,
  querySelector:selector=>selector==='#sqlMain'||selector==='main'?main:selector==='header .header-actions'?headerActions:selector==='header'?header:null};
 const window={__TAURI__:{core:{invoke(command){const request=deferred();pending.push({command,...request});return request.promise;}},dialog:{open:async()=>null},app:{getVersion:()=>version.promise}},
  addEventListener(name,fn){(listeners[name]??=[]).push(fn);},
  dispatchEvent(event){events.push(event);for(const fn of listeners[event.type]||[])fn(event);if(observerThrows)throw Error('observer failed');},tauriOperations:operations};
 class CustomEvent{constructor(type,init){this.type=type;this.detail=init.detail;}}
 const context={window,document,CustomEvent,setInterval(){},clearInterval(){}};
 function loadBridge(){vm.runInNewContext(bridge,context);}
 function loadStatus(){vm.runInNewContext(statusSource(),context);}
 return{window,document,ids,main,headerActions,events,pending,version,loadBridge,loadStatus};
}
const flush=()=>new Promise(resolve=>setImmediate(resolve));
test('installed version comes from runtime independently of available updater version',async()=>{
 const ui=setup();ui.window.availableUpdate={version:'99.0.0'};ui.loadStatus();
 ui.version.resolve('1.6.2');await flush();
 assert.equal(ui.ids.appVersion.textContent,'Версия 1.6.2');
});
test('missing or failed runtime version is truthful and does not throw',async()=>{
 const ui=setup();ui.window.__TAURI__.app=null;ui.loadStatus();await flush();
 assert.match(ui.ids.appVersion.textContent,/недоступна/i);
 const failed=setup();failed.loadStatus();failed.version.reject(Error('IPC failed'));await flush();
 assert.match(failed.ids.appVersion.textContent,/недоступна/i);
});
test('bridge activity remains visible until concurrent reads and writes settle, including rejection',async()=>{
 const ui=setup();ui.loadBridge();ui.loadStatus();ui.version.resolve('1.6.2');
 assert.equal(ui.ids.appActivity.hidden,true);
 const read=ui.window.desktopAPI.nativeReportPage(1,{});
 const save=ui.window.desktopAPI.save({});
 assert.equal(ui.ids.appActivity.hidden,false);
 assert.equal(ui.main.attributes['aria-busy'],'true');
 assert.match(ui.ids.appActivityText.textContent,/Сохран|Загруз/i);
 assert.equal(ui.events.length,2);
 assert.equal(ui.events[1].detail.reads,1);assert.equal(ui.events[1].detail.saves,1);
 ui.pending[0].resolve([]);await read;
 assert.equal(ui.ids.appActivity.hidden,false);
 assert.match(ui.ids.appActivityText.textContent,/Сохран/i);
 const rejected=assert.rejects(save,/disk full/);ui.pending[1].reject(Error('disk full'));await rejected;
 assert.equal(ui.ids.appActivity.hidden,true);
 assert.equal(ui.main.attributes['aria-busy'],'false');
 assert.equal(ui.events.length,4);
 assert.equal(ui.events[3].detail.reads,0);assert.equal(ui.events[3].detail.saves,0);
});
test('status initializes from pending operations that began before it loaded',()=>{
 const ui=setup({operations:{reads:2,saves:0,updating:false}});ui.loadStatus();
 assert.equal(ui.ids.appActivity.hidden,false);assert.match(ui.ids.appActivityText.textContent,/Загруз/i);
 assert.equal(ui.main.attributes['aria-busy'],'true');
 ui.version.resolve('1.6.2');
});
test('observer exceptions never replace original IPC errors or corrupt operation counts',async()=>{
 const ui=setup({observerThrows:true});ui.loadBridge();
 const read=ui.window.desktopAPI.nativeRevision();
 assert.equal(ui.pending.length,1);assert.equal(ui.window.tauriOperations.reads,1);
 const rejected=assert.rejects(read,/original database error/);ui.pending[0].reject(Error('original database error'));await rejected;
 assert.equal(ui.window.tauriOperations.reads,0);assert.equal(ui.events.length,2);
 ui.window.tauriOperations.updating=true;
 await assert.rejects(ui.window.desktopAPI.nativeRevision(),/обновления/);
 assert.equal(ui.events.length,2);assert.equal(ui.pending.length,1);
});
test('classic shell receives accessible version and activity mounts without HTML injection',async()=>{
 const ui=setup({mounts:false});ui.loadStatus();ui.version.resolve('1.6.2');await flush();
 assert.equal(ui.ids.appVersion?.textContent,'Версия 1.6.2');
 assert.equal(ui.ids.appActivity?.attributes.role,'status');
 assert.equal(ui.ids.appActivity?.attributes['aria-live'],'polite');
 assert.equal(ui.ids.appActivity?.hidden,true);
 assert.ok(ui.ids.appActivityText);assert.equal(ui.headerActions.children.includes(ui.ids.appVersion),true);
});
test('no DOM mounts or Tauri API does not throw',()=>{
 assert.doesNotThrow(()=>vm.runInNewContext(statusSource(),{window:{},document:{getElementById:()=>null,querySelector:()=>null}}));
});

test('dismissing an activity notification preserves the operation and resets for the next one',async()=>{
 const ui=setup();ui.loadBridge();ui.loadStatus();
 const read=ui.window.desktopAPI.nativeRevision();
 const dismiss=ui.ids.appActivity.children.find(n=>n.className==='status-dismiss');
 dismiss.onclick();
 assert.equal(ui.ids.appActivity.hidden,true);
 assert.equal(ui.main.attributes['aria-busy'],'true');
 assert.equal(ui.window.tauriOperations.reads,1);
 ui.pending[0].resolve(1);await read;
 assert.equal(ui.main.attributes['aria-busy'],'false');
 const next=ui.window.desktopAPI.nativeRevision();
 assert.equal(ui.ids.appActivity.hidden,false);
 ui.pending[1].resolve(1);await next;
});
