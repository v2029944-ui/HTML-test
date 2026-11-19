'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-navigation.js'),'utf8');
const routes=['analysis','reports','groups','excel','metrics','backup'];
function fake(){
 const links=routes.map(name=>({
   textContent:name,getAttribute(key){return key==='data-nav-link'?name:null;},
   attributes:{},setAttribute(k,v){this.attributes[k]=v;},
   removeAttribute(k){delete this.attributes[k];}
 }));
 const panels=['analysis','backup','reports','groups','analysis','excel','excel','excel','metrics'].map(name=>({
   hidden:false,getAttribute(key){return key==='data-nav-panel'?name:null;}
 }));
 const status={textContent:''},events={};
 const window={location:{hash:''},addEventListener:(kind,handler)=>events[kind]=handler};
 vm.runInNewContext(script,{window,document:{
   querySelectorAll:selector=>selector==='[data-nav-link]'?links:selector==='[data-nav-panel]'?panels:[],
   getElementById:id=>id==='nativeViewStatus'?status:null
 }});
 return {window,links,panels,status,events};
}
test('six SQL sections remain in one application without full-history bootstrap',()=>{
 const root=path.resolve(__dirname,'../..'),tmp=fs.mkdtempSync(path.join(os.tmpdir(),'native-nav-'));
 try{
  prepare({base:root,output:tmp});
  const html=fs.readFileSync(path.join(tmp,'native-workspace.html'),'utf8');
  for(const key of routes){
   assert.match(html,new RegExp('data-nav-link="'+key+'"'));
   assert.match(html,new RegExp('data-nav-panel="'+key+'"'));
  }
  for(const id of ['metricReports','reportRows','groupRows','divisionRows','sourceRuleSave',
                    'legacyChoose','wideChoose','metricKindSelect','restoreSchedule',
                    'sqlUpdateCheck','legacyWorkspaceLink']){
   assert.match(html,new RegExp('id="'+id+'"'));
  }
  assert.match(html,/tauri-native-navigation\\.js/);
  assert.equal(fs.readFileSync(path.join(tmp,'tauri-native-navigation.js'),'utf8'),script);
  assert.ok(!script.includes('desktopAPI.load('));
  assert.ok(!script.includes('load_data'));
  assert.ok(!fs.readFileSync(path.join(root,'index.html'),'utf8').includes('tauri-native-navigation.js'));
 }finally{fs.rmSync(tmp,{recursive:true,force:true});}
});
test('navigation defaults to analytics and keeps the other SQL sections in the DOM',()=>{
 const ui=fake();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,2);
 assert.equal(ui.links[0].attributes['aria-current'],'page');
 assert.match(ui.status.textContent,/Аналитика|analysis/i);
 ui.window.location.hash='#excel';ui.events.hashchange();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,3);
 assert.ok(ui.panels.filter(p=>!p.hidden).every(p=>p.getAttribute('data-nav-panel')==='excel'));
 assert.equal(ui.links[3].attributes['aria-current'],'page');
 assert.equal(ui.links[0].attributes['aria-current'],undefined);
 ui.window.location.hash='#backup';ui.events.hashchange();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,1);
 assert.equal(ui.panels.find(p=>!p.hidden).getAttribute('data-nav-panel'),'backup');
});
test('unknown deep links fall back to analytics, no SQL controls are destroyed',()=>{
 const ui=fake();
 ui.window.location.hash='#not-a-view';ui.events.hashchange();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,2);
 assert.equal(ui.panels.length,9);
 assert.equal(ui.window.nativeWorkspaceNavigation.active(),'analysis');
 assert.equal(ui.window.nativeWorkspaceNavigation.select('groups'),'groups');
 assert.equal(ui.panels.filter(p=>!p.hidden)[0].getAttribute('data-nav-panel'),'groups');
});
