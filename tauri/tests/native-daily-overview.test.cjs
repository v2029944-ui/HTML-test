'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-daily-overview.js'),'utf8');
class E{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.className='';this.value='';this.attributes={};this.events={};}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=[...nodes];}
 setAttribute(key,value){this.attributes[key]=String(value);}
 addEventListener(name,fn){this.events[name]=fn;}
}
function setup(){
 const ids=['dailySourceKpis','dailySourceRows','dailySourceChart','dailySourceStatus','dailySourceMetric',
 'dailySourceDivisionRows','dailySourceDivisionStatus'];
 const elements=Object.fromEntries(ids.map(id=>[id,new E(id)]));
 elements.dailySourceMetric.value='amount';
 const window={},document={getElementById:id=>elements[id],createElement:tag=>new E(tag),
  createElementNS:(ns,tag)=>{assert.equal(ns,'http://www.w3.org/2000/svg');return new E(tag);}};
 vm.runInNewContext(script,{window,document});
 return {e:elements,api:window.nativeDailyOverview};
}
const summary=(complete=false,available=2)=>({
 complete,periods:3,organizations:1,missing:complete?0:1,available_reports:available,
 total:complete?{count:3,amount:20,average:20/3}:{count:null,amount:null,average:null},
 available_total:available?{count:2,amount:11,average:5.5}:{count:null,amount:null,average:null}
});
const point=(start,count,amount,missing=0)=>({
 start,periods:1,missing,
 total:missing?{count:null,amount:null,average:null}:{count,amount,average:null}
});
const timeline=points=>({granularity:'period',points});
function find(root,tag){
 return root.children.flatMap(child=>[...(child.tag===tag?[child]:[]),...find(child,tag)]);
}
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('observed-only KPI never claims full coverage, zero is real and missing gap breaks chart',()=>{
 const {e,api}=setup();
 api.show(summary(),timeline([
  point('2024-02-28',0,0),
  point('2024-02-29',0,0,1),
  point('2024-03-01',2,11)
 ]));
 assert.equal(e.dailySourceKpis.children.length,5);
 assert.equal(e.dailySourceKpis.children[0].children[0].textContent,'2');
 assert.match(e.dailySourceKpis.children[0].children[1].textContent,/имеющиеся данные/);
 assert.equal(e.dailySourceKpis.children[1].children[0].textContent,'11');
 assert.equal(e.dailySourceRows.children.length,3);
 assert.equal(e.dailySourceRows.children[0].children[4].textContent,'0');
 assert.equal(e.dailySourceRows.children[1].children[4].textContent,'—');
 const svg=e.dailySourceChart.children[0];
 assert.equal(svg.tag,'svg');
 assert.equal(find(svg,'path').length,0,'gap must not be bridged');
 assert.equal(find(svg,'circle').length,2);
 assert.match(e.dailySourceStatus.textContent,/2 подтверждённых точек из 3/);
 e.dailySourceMetric.value='count';e.dailySourceMetric.events.change();
 assert.match(e.dailySourceStatus.textContent,/Количество \/ объём/);
});
test('missing all reports yields unknown KPI and no fabricated SVG',()=>{
 const {e,api}=setup();
 api.show(summary(false,0),timeline([
  point('2024-02-28',0,0,1),
  point('2024-02-29',0,0,1)
 ]));
 assert.equal(e.dailySourceKpis.children[1].children[0].textContent,'—');
 assert.equal(e.dailySourceChart.children.length,0);
 assert.match(e.dailySourceStatus.textContent,/Нет полностью подтверждённых точек/);
});
test('monthly-aggregated trend can show signed changes and exact table rows',()=>{
 const {e,api}=setup();
 api.show(summary(),{granularity:'month',points:[
  {start:'2024-01',periods:31,missing:0,total:{count:8,amount:-30,average:null}},
  {start:'2024-02',periods:29,missing:0,total:{count:9,amount:12,average:null}}
 ]});
 assert.equal(e.dailySourceRows.children.length,2);
 assert.equal(e.dailySourceRows.children[0].children[4].textContent,'-30');
 assert.match(e.dailySourceStatus.textContent,/календарные месяцы/);
 assert.equal(find(e.dailySourceChart.children[0],'path').length,1);
});
test('malformed completeness and oversized trends erase old values',()=>{
 const {e,api}=setup();
 api.show(summary(),timeline([point('2024-02-28',1,10)]));
 const invalid=summary();invalid.total.amount=0;
 api.show(invalid,timeline([point('2024-02-28',1,10)]));
 assert.equal(e.dailySourceChart.children.length,0);
 assert.equal(e.dailySourceKpis.children.length,0);
 assert.match(e.dailySourceStatus.textContent,/неизвестное значение/i);
 api.show(summary(),timeline(Array.from({length:125},(_,i)=>point('2024-02-28',i,i))));
 assert.equal(e.dailySourceChart.children.length,0);
 api.clear('Выборка изменена');
 assert.equal(e.dailySourceStatus.textContent,'Выборка изменена');
});

test('daily divisions use the exact selected members and hide incomplete values',()=>{
 const {e,api}=setup();
 api.show(summary(),timeline([point('2024-02-28',0,0)]));
 api.showOrganizations([
  {org:'B',complete:false,missing:1,total:{count:null,amount:null,average:null}},
  {org:'A',complete:true,missing:0,total:{count:0,amount:0,average:null}}
 ],['A','B']);
 assert.equal(e.dailySourceDivisionRows.children.length,2);
 assert.equal(e.dailySourceDivisionRows.children[0].children[0].textContent,'A');
 assert.equal(e.dailySourceDivisionRows.children[0].children[3].textContent,'0');
 assert.equal(e.dailySourceDivisionRows.children[1].children[3].textContent,'—');
 assert.match(e.dailySourceDivisionStatus.textContent,/С полным дневным охватом: 1/);
 assert.equal(e.dailySourceChart.children.length,1,'divisions must not overwrite existing chart');
});
test('daily divisions reject duplicates, invented organizations, and forged incomplete totals',()=>{
 const {e,api}=setup();
 api.show(summary(),timeline([point('2024-02-28',0,0)]));
 api.showOrganizations([{org:'A',complete:false,missing:2,total:{count:0,amount:1,average:null}}],['A']);
 assert.equal(e.dailySourceDivisionRows.children.length,0);
 assert.match(e.dailySourceDivisionStatus.textContent,/недоступно/);
 api.showOrganizations([{org:'B',complete:true,missing:0,total:{count:1,amount:2}}],['A']);
 assert.equal(e.dailySourceDivisionRows.children.length,0);
 assert.match(e.dailySourceDivisionStatus.textContent,/недоступно/);
 api.showOrganizations([
  {org:'A',complete:true,missing:0,total:{count:1,amount:2}},
  {org:'A',complete:true,missing:0,total:{count:1,amount:2}}
 ],['A','B']);
 assert.equal(e.dailySourceDivisionRows.children.length,0);
 api.clear('Новая выборка');
 assert.equal(e.dailySourceDivisionRows.children.length,0);
 assert.match(e.dailySourceDivisionStatus.textContent,/после расчёта/);
});
