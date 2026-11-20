'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-navigation.js'),'utf8');
const routes=['analysis','reports','groups','excel','metrics','backup'];
function fake(){
 const links=routes.map(name=>({
   textContent:name,getAttribute(key){return key==='data-nav-link'?name:null;},
   attributes:{},events:{},setAttribute(k,v){this.attributes[k]=v;},
   removeAttribute(k){delete this.attributes[k];},
   addEventListener(event,handler){this.events[event]=handler;}
 }));
 const panels=['analysis','backup','reports','groups','excel','excel','excel','metrics'].map(name=>({
   hidden:false,getAttribute(key){return key==='data-nav-panel'?name:null;}
 }));
 const status={textContent:''},events={},scrolls=[];
 const window={location:{hash:''},addEventListener:(kind,handler)=>events[kind]=handler,
   scrollTo:(x,y)=>scrolls.push([x,y])};
 vm.runInNewContext(script,{window,document:{
   querySelectorAll:selector=>selector==='[data-nav-link]'?links:selector==='[data-nav-panel]'?panels:[],
   getElementById:id=>id==='nativeViewStatus'?status:null
 }});
 return {window,links,panels,status,events,scrolls};
}
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('navigation defaults to analytics and keeps the other SQL sections in the DOM',()=>{
 const ui=fake();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,1);
 assert.equal(ui.links[0].attributes['aria-current'],'page');
 assert.match(ui.status.textContent,/Аналитика|analysis/i);
 ui.window.location.hash='#excel';ui.events.hashchange();
 assert.equal(ui.scrolls.length,1);
 assert.equal(ui.panels.filter(p=>!p.hidden).length,3);
 assert.ok(ui.panels.filter(p=>!p.hidden).every(p=>p.getAttribute('data-nav-panel')==='excel'));
 assert.equal(ui.links[3].attributes['aria-current'],'page');
 assert.equal(ui.links[0].attributes['aria-current'],undefined);
 ui.window.location.hash='#backup';ui.events.hashchange();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,1);
 assert.equal(ui.panels.find(p=>!p.hidden).getAttribute('data-nav-panel'),'backup');
});
test('tab clicks switch visible SQL section immediately before delayed hashchange',()=>{
 const ui=fake();
 assert.equal(ui.window.nativeWorkspaceNavigation.active(),'analysis');
 ui.links[3].events.click();
 assert.equal(ui.window.nativeWorkspaceNavigation.active(),'excel');
 assert.equal(ui.panels.filter(p=>!p.hidden).length,3);
 assert.equal(ui.panels.filter(p=>!p.hidden).every(p=>p.getAttribute('data-nav-panel')==='excel'),true);
 assert.equal(ui.links[3].attributes['aria-current'],'page');
 ui.window.location.hash='#excel';ui.events.hashchange();
 assert.equal(ui.window.nativeWorkspaceNavigation.active(),'excel');
 assert.deepEqual(ui.scrolls,[[0,0]]);
});
test('unknown deep links fall back to analytics, no SQL controls are destroyed',()=>{
 const ui=fake();
 ui.window.location.hash='#not-a-view';ui.events.hashchange();
 assert.equal(ui.panels.filter(p=>!p.hidden).length,1);
 assert.equal(ui.panels.length,8);
 assert.equal(ui.window.nativeWorkspaceNavigation.active(),'analysis');
 assert.equal(ui.window.nativeWorkspaceNavigation.select('groups'),'groups');
 assert.equal(ui.panels.filter(p=>!p.hidden)[0].getAttribute('data-nav-panel'),'groups');
});
test('classic management menu closes accessibly and changes retains the shared filter panel',()=>{
 const keys=['analysis','changes','reports'];const eventHandlers={};let focused=false;
 const menu={open:false,contains:target=>target==='inside'};
 const toggle={classList:{toggle(){}},focus(){focused=true;}};
 const links=keys.map(key=>({textContent:key,events:{},getAttribute:()=>key,setAttribute(){},removeAttribute(){},addEventListener(name,fn){this.events[name]=fn;}}));
 const panels=['analysis','changes','analysis changes','reports'].map(key=>({hidden:false,getAttribute:()=>key}));
 const window={location:{hash:''},addEventListener(){}};
 vm.runInNewContext(script,{window,document:{querySelectorAll:q=>q==='[data-nav-link]'?links:panels,getElementById:id=>id==='nativeViewStatus'?{textContent:''}:id==='managementNav'?menu:toggle,addEventListener:(name,fn)=>eventHandlers[name]=fn}});
 menu.open=true;links[1].events.click();assert.equal(menu.open,false);assert.equal(panels[0].hidden,true);assert.equal(panels[1].hidden,false);assert.equal(panels[2].hidden,false);
 menu.open=true;eventHandlers.keydown({key:'Escape'});assert.equal(menu.open,false);assert.equal(focused,true);
 menu.open=true;eventHandlers.click({target:'outside'});assert.equal(menu.open,false);
});
