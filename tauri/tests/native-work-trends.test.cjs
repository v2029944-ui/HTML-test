'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-work-trends.js'),'utf8');
class Element{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.value='';this.style={};this.events={};this.attrs={};}
 append(...els){this.children.push(...els);}
 replaceChildren(...els){this.children=[...els];}
 setAttribute(key,value){this.attrs[key]=value;}
 addEventListener(type,handler){this.events[type]=handler;}
}
function mount(){
 const ids=['workTrendChart','workTrendRows','workTrendSelected','workTrendStatus','workTrendMetric'];
 const e=Object.fromEntries(ids.map(id=>[id,new Element(id)]));
 e.workTrendMetric.value='amount';
 const window={},document={
   getElementById:id=>e[id],
   createElement:tag=>new Element(tag),
   createElementNS:(ns,tag)=>{assert.equal(ns,'http://www.w3.org/2000/svg');return new Element(tag);}
 };
 vm.runInNewContext(script,{window,document});
 return {e,window};
}
function find(root,tag){
 return root.children.flatMap(child=>[...(child.tag===tag?[child]:[]),...find(child,tag)]);
}
const point=(start,count,amount,missing=0)=>({
 start,periods:1,missing,total:missing?{count:null,amount:null}:{count,amount}
});
const result=(points,key='group:one',name='Работа <script>')=>({
 key,name,kind:'group',granularity:'period',points
});
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('real zero remains zero while missing periods break work trends',()=>{
 const {window,e}=mount();
 window.nativeWorkTrends.show(result([
   point('2024-02-28',-2,-20),point('2024-02-29',0,0),
   point('2024-03-01',0,0,1),point('2024-03-02',3,12)
 ]));
 assert.equal(e.workTrendRows.children.length,4);
 assert.equal(e.workTrendRows.children[1].children[4].textContent,'0');
 assert.equal(e.workTrendRows.children[2].children[4].textContent,'—');
 assert.equal(e.workTrendSelected.textContent,'Группа: Работа <script>');
 const chart=e.workTrendChart.children[0];
 assert.equal(chart.tag,'svg');
 assert.equal(find(chart,'path').length,1,'missing period must break the line');
 assert.equal(find(chart,'circle').length,3);
 assert.match(e.workTrendStatus.textContent,/подтверждено 3 из 4/);
 e.workTrendMetric.value='count';e.workTrendMetric.events.change();
 assert.match(e.workTrendStatus.textContent,/Количество \/ объём/);
 window.nativeWorkTrends.clear('Выборка изменена');
 assert.equal(e.workTrendRows.children.length,0);
 assert.equal(e.workTrendChart.children.length,0);
});
test('one confirmed zero is a valid bar, but unknown-only series draws no chart',()=>{
 const {window,e}=mount();
 window.nativeWorkTrends.show(result([point('2024-02-28',0,0)]));
 assert.equal(find(e.workTrendChart.children[0],'rect').length,1);
 window.nativeWorkTrends.show(result([point('2024-02-28',0,0,1)]));
 assert.equal(e.workTrendRows.children.length,1);
 assert.equal(e.workTrendChart.children.length,0);
 assert.match(e.workTrendStatus.textContent,/неизвестные значения/i);
});
test('invalid or oversized data clears previous graph and cannot inject labels',()=>{
 const {window,e}=mount();
 window.nativeWorkTrends.show(result([point('2024-02-28',2,10)]));
 window.nativeWorkTrends.show(result([point('2024-02-28',2,10,1)]));
 assert.equal(e.workTrendChart.children.length,0);
 assert.match(e.workTrendStatus.textContent,/неизвестные значения/i);
 window.nativeWorkTrends.show(result([{start:'2024-02-28',periods:1,missing:0,total:{count:null,amount:null}}]));
 assert.equal(e.workTrendChart.children.length,0);
 assert.match(e.workTrendStatus.textContent,/скрыта/);
 window.nativeWorkTrends.show(result(Array.from({length:125},(_,i)=>point('2024-02-28',i,i))));
 assert.equal(e.workTrendChart.children.length,0);
 window.nativeWorkTrends.show(result([point('<svg/onload=alert(1)>',2,10)]));
 assert.equal(e.workTrendChart.children.length,0);
});
