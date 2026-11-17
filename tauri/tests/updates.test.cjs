'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const path=require('node:path');
const {manifest}=require('../release.cjs');
async function fixture({configured=true,available=true,deferred='',storageFailure=false,version='0.2.0',onConfirm}={}){
  const ids={},events=[];
  class Node{
    constructor(tag,text){this.tag=tag;this.textContent=text;this.children=[];this.listeners={};}
    set id(value){ids[value]=this;} setAttribute(k,v){this[k]=v;} getAttribute(k){return this[k];} append(...nodes){this.children.push(...nodes);} prepend(...nodes){this.children.unshift(...nodes);} showModal(){this.open=true;} close(){this.open=false;}
    addEventListener(name,fn){this.listeners[name]=fn;}
  }
  ids.storage=new Node('section');ids.managementToggle=new Node('summary');const main=new Node('main');main.inert=false;const saved=new Map(deferred?[['work-reports-update-deferred-v1',deferred]]:[]);
  let downloads=0,checks=0,closed=0,error=null,approved=false;
  const update={version,body:'Изменения <script>bad()</script>',currentVersion:'0.1.0',close:async()=>closed++,downloadAndInstall:async onEvent=>{
    downloads++;if(error)throw error;
    for(const value of [{event:'Started',data:{contentLength:100}},{event:'Progress',data:{chunkLength:50}},{event:'Finished'}])onEvent(value);
  }};
  const ctx={localStorage:{getItem:k=>{if(storageFailure)throw Error('blocked');return saved.get(k);},setItem:(k,v)=>{if(storageFailure)throw Error('blocked');saved.set(k,v);}},state:{},elem:(tag,text)=>new Node(tag,text),document:{getElementById:id=>ids[id],querySelector:()=>main},confirm:async()=>{if(onConfirm)await onConfirm(ctx);return approved;}};
  ctx.window={tauriOperations:{saves:0,reads:0,updating:false},appReady:Promise.resolve(),__TAURI__:{core:{invoke:async()=>configured},updater:{check:async()=>{checks++;if(error)throw error;return available?update:null;}}}};
  await vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../updates.js'),'utf8'),ctx);
  return {ids,ctx,main,saved,counts:()=>({downloads,checks,closed}),approve:value=>approved=value,fail:value=>error=value};
}
test('updater checks on launch, cancellation never downloads, approval installs',async()=>{
  const f=await fixture();assert.equal(f.counts().checks,1);assert.equal(f.ids.installAppUpdate.hidden,false);
  await f.ids.installAppUpdate.listeners.click();assert.equal(f.counts().downloads,0);assert.equal(f.ctx.state.appUpdating,false);
  f.approve(true);await f.ids.installAppUpdate.listeners.click();assert.equal(f.counts().downloads,1);
  assert.equal(f.main.inert,false);assert.equal(f.ctx.state.appUpdating,false);
});
test('unsigned build and no newer version never offer installation',async()=>{
  const unsigned=await fixture({configured:false});assert.equal(unsigned.counts().checks,0);assert.equal(unsigned.ids.installAppUpdate.hidden,true);
  const current=await fixture({available:false});assert.equal(current.ids.installAppUpdate.hidden,true);
});
test('network/signature errors are visible and recover without locking the interface',async()=>{
  const f=await fixture();f.fail(Error('Invalid signature'));f.approve(true);
  await f.ids.installAppUpdate.listeners.click();assert.match(f.ids.appUpdateStatus.textContent,/не установлено.*Invalid signature/);
  assert.equal(f.main.inert,false);assert.equal(f.ctx.state.appUpdating,false);assert.equal(f.ids.installAppUpdate.disabled,false);
  await f.ids.checkAppUpdate.listeners.click();assert.match(f.ids.appUpdateStatus.textContent,/Не удалось проверить/);
  assert.equal(f.ids.checkAppUpdate.disabled,false);
  f.fail(null);await f.ids.checkAppUpdate.listeners.click();assert.equal(f.ids.installAppUpdate.hidden,false);
});
test('the visible Analytics notice reports an import blocker and an invalid signature',async()=>{
  const f=await fixture();f.approve(true);
  // Even after deferral, a failed installation attempt must be visible outside Settings.
  f.ids.deferAppUpdate.listeners.click();assert.equal(f.ids.appUpdateNotice.hidden,true);
  f.ctx.state.excelImports=1;
  await f.ids.installAppUpdate.listeners.click();
  assert.equal(f.counts().downloads,0);
  assert.equal(f.ids.appUpdateNotice.hidden,false);
  assert.equal(f.ids.appUpdateNoticeStatus.hidden,false);
  assert.match(f.ids.appUpdateNoticeStatus.textContent,/Завершите импорт/);
  f.ctx.state.excelImports=0;f.fail(Error('Invalid signature'));
  await f.ids.noticeInstallUpdate.listeners.click();
  assert.equal(f.ids.appUpdateNotice.hidden,false);
  assert.equal(f.ids.appUpdateNoticeStatus.hidden,false);
  assert.match(f.ids.appUpdateNoticeStatus.textContent,/не установлено.*Invalid signature/);
  assert.equal(f.main.inert,false);
  assert.equal(f.ctx.state.appUpdating,false);
  f.fail(null);await f.ids.noticeInstallUpdate.listeners.click();
  assert.match(f.ids.appUpdateNoticeStatus.textContent,/Обновление передано установщику/);
  assert.equal(f.ids.appUpdateNoticeStatus.hidden,false);
});
test('updater does not start during an active import',async()=>{
  const f=await fixture();f.ctx.state.desktopSyncing=true;f.approve(true);
  await f.ids.installAppUpdate.listeners.click();assert.equal(f.counts().downloads,0);
});
test('release manifest points at a distinct HTTPS Tauri installer',()=>{
  const m=manifest('0.2.0','tauri-v0.2.0','signed-value\n');assert.equal(m.version,'0.2.0');
  assert.equal(m.platforms['windows-x86_64'].signature,'signed-value');
  assert.match(m.platforms['windows-x86_64'].url,/^https:\/\/github.com\/v2029944-ui\/HTML-test\/releases\/download\/tauri-v0.2.0\/.*Tauri.*\.exe$/);
  assert.throws(()=>manifest('0.2.0','tauri-updates','sig'));assert.throws(()=>manifest('0.2.0','tag',''));
});

test('Tauri WebView has an explicit local CSP without eval permissions',()=>{
  const config=JSON.parse(fs.readFileSync(path.join(__dirname,'../src-tauri/tauri.conf.json'),'utf8'));
  assert.match(config.app.security.csp,/default-src 'self'/);
  assert.match(config.app.security.csp,/script-src 'self'/);
  assert.match(config.app.security.csp,/connect-src ipc: http:\/\/ipc.localhost/);
  assert.equal(config.app.security.csp.includes('unsafe-eval'),false);
});

test('notification can be deferred per version; badge remains and a manual check reveals it',async()=>{
 const f=await fixture();assert.equal(f.ids.appUpdateNotice.hidden,false);assert.equal(f.ids.appUpdateBadge.hidden,false);
 f.ids.deferAppUpdate.listeners.click();assert.equal(f.ids.appUpdateNotice.hidden,true);
 assert.equal(f.saved.get('work-reports-update-deferred-v1'),'0.2.0');
 const reopened=await fixture({deferred:'0.2.0'});assert.equal(reopened.ids.appUpdateNotice.hidden,true);assert.equal(reopened.ids.appUpdateBadge.hidden,false);
 await reopened.ids.checkAppUpdate.listeners.click();assert.equal(reopened.ids.appUpdateNotice.hidden,false);
 const next=await fixture({deferred:'0.2.0',version:'0.3.0'});assert.equal(next.ids.appUpdateNotice.hidden,false);
});
test('release notes are plain text; missing descriptions and unavailable preferences are handled',async()=>{
 const f=await fixture({storageFailure:true});f.ids.noticeWhatsNew.listeners.click();assert.equal(f.ids.appUpdateNotes.open,true);
 assert.match(f.ids.appUpdateNotes.children[1].textContent,/<script>bad/);f.ids.closeUpdateNotes.listeners.click();assert.equal(f.ids.appUpdateNotes.open,false);
 f.ids.deferAppUpdate.listeners.click();assert.match(f.ids.appUpdateStatus.textContent,/Сохранить отсрочку не удалось/);
});
for(const target of ['excelImports','folderImport','reads','saves'])test('installation is blocked by '+target,async()=>{
 const f=await fixture();f.approve(true);
 if(['reads','saves'].includes(target))f.ctx.window.tauriOperations[target]=1;else f.ctx.state[target]=1;
 await f.ids.noticeInstallUpdate.listeners.click();assert.equal(f.counts().downloads,0);assert.equal(f.ctx.state.appUpdating,undefined);
});
test('operations are checked again after the confirmation resolves',async()=>{
 const f=await fixture({onConfirm:ctx=>{ctx.window.tauriOperations.saves=1;}});f.approve(true);
 await f.ids.installAppUpdate.listeners.click();assert.equal(f.counts().downloads,0);assert.equal(f.ctx.window.tauriOperations.updating,false);
});
