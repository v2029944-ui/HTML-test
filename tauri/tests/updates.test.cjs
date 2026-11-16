'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const path=require('node:path');
const {manifest}=require('../release.cjs');
async function fixture({configured=true,available=true}={}){
  const ids={},main={inert:false},events=[];
  class Node{
    constructor(tag,text){this.tag=tag;this.textContent=text;this.children=[];this.listeners={};}
    set id(value){ids[value]=this;} setAttribute(){} append(...nodes){this.children.push(...nodes);}
    addEventListener(name,fn){this.listeners[name]=fn;}
  }
  ids.storage=new Node('section');
  let downloads=0,checks=0,closed=0,error=null,approved=false;
  const update={version:'0.2.0',currentVersion:'0.1.0',close:async()=>closed++,downloadAndInstall:async onEvent=>{
    downloads++;if(error)throw error;
    for(const value of [{event:'Started',data:{contentLength:100}},{event:'Progress',data:{chunkLength:50}},{event:'Finished'}])onEvent(value);
  }};
  const ctx={state:{},elem:(tag,text)=>new Node(tag,text),document:{getElementById:id=>ids[id],querySelector:()=>main},confirm:async()=>approved};
  ctx.window={appReady:Promise.resolve(),__TAURI__:{core:{invoke:async()=>configured},updater:{check:async()=>{checks++;if(error)throw error;return available?update:null;}}}};
  await vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../updates.js'),'utf8'),ctx);
  return {ids,ctx,main,counts:()=>({downloads,checks,closed}),approve:value=>approved=value,fail:value=>error=value};
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
