'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-name-filter.js'),'utf8');
class E{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.value='';this.events={};this.disabled=false;this.className='';}
 append(...elements){this.children.push(...elements);}
 replaceChildren(...elements){this.children=[...elements];}
 addEventListener(type,fn){this.events[type]=fn;}
}
function mount(opt={}){
 const ids=['analysisNameSearch','analysisNameSearchRun','analysisName','analysisNameReset','analysisNameStatus','analysisNameAdd','analysisNameSelected'];
 const e=Object.fromEntries(ids.map(k=>[k,new E(k)]));const requests=[],notify=[];
 const window={desktopAPI:{nativeAnalysisNameOptions:async(revision,search,limit)=>{
  requests.push({revision,search,limit});
  if(opt.response)return opt.response({revision,search,limit});
  return {has_more:false,rows:[
   {name:'Group',kind:'group'},{name:'Работа',kind:'work'}
  ]};
 }},nativeWorkspaceFilterChanged:()=>notify.push(true)};
 const document={getElementById:k=>e[k],createElement:type=>new E(type)};
 vm.runInNewContext(script,{window,document,TextEncoder});
 return {e,window,requests,notify};
}
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('search uses bounded native SQLite results, explicit choice drives all filters',async()=>{
 const {e,window,requests,notify}=mount();
 assert.equal(e.analysisNameSearchRun.disabled,true,'no SQLite revision');
 window.nativeNameFilter.setRevision(12);
 assert.equal(e.analysisNameSearchRun.disabled,false);
 e.analysisNameSearch.value='Gr';
 await window.nativeNameFilter.search();
 assert.equal(requests[0].revision,12);
 assert.equal(requests[0].search,'Gr');
 assert.equal(requests[0].limit,50);
 assert.equal(e.analysisName.children.length,3);
 assert.equal(e.analysisName.children[1].textContent,'Группа: Group');
 assert.equal(e.analysisName.children[2].textContent,'Работа: Работа');
 e.analysisName.value='Group';e.analysisName.events.change();e.analysisNameAdd.events.click();
 assert.equal(window.nativeNameFilter.selectedName(),'Group');
 assert.equal(notify.length,1);
 assert.equal(e.analysisNameReset.disabled,false);
 e.analysisNameSearch.value='missing';
 await window.nativeNameFilter.search();
 assert.equal(window.nativeNameFilter.selectedName(),'Group');
 assert.equal(e.analysisName.children[1].textContent,'Группа: Group','same option stays selected');
 e.analysisNameReset.events.click();
 assert.equal(window.nativeNameFilter.selectedName(),null);
 assert.equal(notify.length,2);
 assert.equal(e.analysisName.value,'');
});
test('new search cannot erase an already applied group outside the result page',async()=>{
 let answer=0;
 const {e,window}=mount({response:()=>++answer===1?
  {rows:[{name:'Group',kind:'group'}],has_more:false}:
  {rows:[{name:'Different',kind:'work'}],has_more:true}});
 window.nativeNameFilter.setRevision(17);
 await window.nativeNameFilter.search();
 e.analysisName.value='Group';e.analysisName.events.change();e.analysisNameAdd.events.click();
 await window.nativeNameFilter.search();
 assert.equal(window.nativeNameFilter.selectedName(),'Group');
 assert.equal(e.analysisNameSelected.children[0].children[0].textContent,'Group');
 assert.match(e.analysisNameStatus.textContent,/уточните запрос/);
 window.nativeNameFilter.setLocked(true);
 assert.equal(e.analysisName.disabled,true);
 assert.equal(e.analysisNameSearchRun.disabled,true);
 window.nativeNameFilter.setLocked(false);
 assert.equal(e.analysisName.disabled,false);
});
test('revision change invalidates options and late native search response',async()=>{
 let resolveOld;
 const {e,window,notify}=mount({response:()=>new Promise(resolve=>{resolveOld=resolve;})});
 window.nativeNameFilter.setRevision(8);
 const call=window.nativeNameFilter.search();
 assert.equal(e.analysisNameSearchRun.disabled,true);
 window.nativeNameFilter.setRevision(9);
 resolveOld({rows:[{name:'Stale',kind:'work'}],has_more:false});
 await call;
 assert.equal(e.analysisName.children.length,1);
 assert.equal(window.nativeNameFilter.selectedName(),null);
 assert.equal(notify.length,0);
 assert.equal(e.analysisNameSearchRun.disabled,false);
});
test('invalid, duplicated and oversized native options are rejected before a selection is created',async()=>{
 for(const rows of [
  Array.from({length:51},(_,i)=>({name:'n'+i,kind:'work'})),
  [{name:'same',kind:'group'},{name:'same',kind:'work'}],
  [{name:'X',kind:'unknown'}],
  [{name:'X'.repeat(600),kind:'group'}]
 ]){
  const {e,window}=mount({response:()=>({rows,has_more:false})});
  window.nativeNameFilter.setRevision(1);
  await window.nativeNameFilter.search();
  assert.equal(e.analysisName.children.length,1);
  assert.match(e.analysisNameStatus.textContent,/недоступен|недоступны/);
 }
});

test('selection spans searches and can remove one exact name without clearing the rest',async()=>{
 let answer=0;
 const {e,window,notify}=mount({response:()=>({rows:[
  {name:++answer===1?'*':'Работа',kind:'work'}
 ],has_more:false})});
 window.nativeNameFilter.setRevision(11);
 for(const name of ['*','Работа']){
  await window.nativeNameFilter.search();
  e.analysisName.value=name;
  e.analysisName.events.change();
  e.analysisNameAdd.events.click();
 }
 assert.deepEqual(Array.from(window.nativeNameFilter.selectedNames()),['*','Работа']);
 const remove=e.analysisNameSelected.children[0].children[1];
 window.nativeNameFilter.setLocked(true);
 assert.equal(remove.disabled,true);
 remove.events.click();
 assert.equal(notify.length,2);
 window.nativeNameFilter.setLocked(false);
 remove.events.click();
 assert.deepEqual(Array.from(window.nativeNameFilter.selectedNames()),['Работа']);
 assert.equal(notify.length,3);
});
test('selection stops at 50 names and reports the limit',async()=>{
 let page=0;
 const {e,window,notify}=mount({response:()=>({
  rows:Array.from({length:50},(_,i)=>({name:'work '+(page*50+i),kind:'work'})),has_more:++page===1
 })});
 window.nativeNameFilter.setRevision(12);
 await window.nativeNameFilter.search();
 for(let i=0;i<50;i++){
  e.analysisName.value='work '+i;
  e.analysisName.events.change();
  e.analysisNameAdd.events.click();
 }
 await window.nativeNameFilter.search();
 e.analysisName.value='work 50';
 e.analysisName.events.change();
 e.analysisNameAdd.events.click();
 assert.equal(window.nativeNameFilter.selectedNames().length,50);
 assert.equal(notify.length,50);
 assert.match(e.analysisNameStatus.textContent,/50/);
});
