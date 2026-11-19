'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const source=fs.readFileSync(path.join(__dirname,'../native-updates.js'),'utf8');
function fixture({configured=true,available=true,approve=false,fail=false}={}){
 const ids={};
 for(const id of ['sqlUpdateCheck','sqlUpdateInstall','sqlUpdateNotes','sqlUpdateStatus','sqlUpdateDialog','sqlUpdateNotesText','sqlUpdateClose']){
  ids[id]={hidden:id==='sqlUpdateInstall'||id==='sqlUpdateNotes',disabled:false,events:{},textContent:'',
    addEventListener(name,fn){this.events[name]=fn;},showModal(){this.open=true;},close(){this.open=false;}};
 }
 const ops={reads:0,saves:0,updating:false};
 const calls={check:0,install:0,closed:0};
 const update={version:'1.5.2',body:'Текст <script>alert(1)</script>',close:async()=>calls.closed++,
  downloadAndInstall:async callback=>{calls.install++;if(fail)throw Error('Invalid signature');callback({event:'Started',data:{contentLength:100}});callback({event:'Progress',data:{chunkLength:100}});callback({event:'Finished'});}};
 const window={tauriOperations:ops,confirm:()=>approve,__TAURI__:{
  core:{invoke:async name=>{assert.equal(name,'updater_configured');return configured;}},
  updater:{check:async()=>{calls.check++;return available?update:null;}}
 }};
 vm.runInNewContext(source,{window,document:{getElementById:id=>ids[id]}});
 return {ids,ops,calls,window,ready:window.nativeSqlUpdaterReady};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('signed SQL mode checks on launch but requires confirmation to install',async()=>{
 const ui=fixture();await ui.ready;
 assert.equal(ui.calls.check,1);
 assert.equal(ui.ids.sqlUpdateInstall.hidden,false);
 ui.ids.sqlUpdateInstall.events.click();await settle();
 assert.equal(ui.calls.install,0);
 assert.equal(ui.ops.updating,false);
 const yes=fixture({approve:true});await yes.ready;
 yes.ids.sqlUpdateInstall.events.click();await settle();
 assert.equal(yes.calls.install,1);
 assert.equal(yes.ops.updating,false);
 assert.match(yes.ids.sqlUpdateStatus.textContent,/установщику/);
});
test('unsigned SQL mode does not check network and never offers installation',async()=>{
 const ui=fixture({configured:false});await ui.ready;
 assert.equal(ui.calls.check,0);
 assert.equal(ui.ids.sqlUpdateInstall.hidden,true);
 assert.match(ui.ids.sqlUpdateStatus.textContent,/недоступны/);
});
test('SQL mode prevents installation during tracked SQLite operations',async()=>{
 const ui=fixture({approve:true});await ui.ready;
 ui.ops.reads=1;
 ui.ids.sqlUpdateInstall.events.click();await settle();
 assert.equal(ui.calls.install,0);
 assert.match(ui.ids.sqlUpdateStatus.textContent,/Дождитесь/);
 ui.ops.reads=0;ui.ops.saves=1;
 ui.ids.sqlUpdateInstall.events.click();await settle();
 assert.equal(ui.calls.install,0);
});
test('invalid signature remains visible and does not leave updater locked',async()=>{
 const ui=fixture({approve:true,fail:true});await ui.ready;
 ui.ids.sqlUpdateInstall.events.click();await settle();
 assert.match(ui.ids.sqlUpdateStatus.textContent,/Invalid signature/);
 assert.equal(ui.ops.updating,false);
 assert.equal(ui.ids.sqlUpdateCheck.disabled,false);
});
test('signed notes use textContent and cannot execute HTML',async()=>{
 const ui=fixture();await ui.ready;
 ui.ids.sqlUpdateNotes.events.click();
 assert.equal(ui.ids.sqlUpdateNotesText.textContent,'Текст <script>alert(1)</script>');
 assert.equal(ui.ids.sqlUpdateDialog.open,true);
 ui.ids.sqlUpdateClose.events.click();
 assert.equal(ui.ids.sqlUpdateDialog.open,false);
});
