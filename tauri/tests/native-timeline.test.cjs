'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const source=fs.readFileSync(path.join(__dirname,'../native-timeline.js'),'utf8');
class Element {
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.value='';this.className='';this.attrs={};this.events={};}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=[...children];}
 setAttribute(name,value){this.attrs[name]=value;}
 addEventListener(event,handler){this.events[event]=handler;}
}
function mount(){
 const ids=['hideEmptyPeriods','timelineChart','timelineRows','timelineStatus','timelineMetric','timelineMode'];
 const elements=Object.fromEntries(ids.map(id=>[id,new Element('div')]));
 elements.timelineMetric.value='amount';elements.timelineMode.value='line';
 const window={nativePeriodVisibility:require('../native-period-visibility.js')},document={
   getElementById:id=>elements[id],
   createElement:tag=>new Element(tag),
   createElementNS:(ns,tag)=>{assert.equal(ns,'http://www.w3.org/2000/svg');return new Element(tag);}
 };
 vm.runInNewContext(source,{window,document});
 return {window,e:elements};
}
function all(node,tag){
 return node.children.flatMap(child=>[
   ...(child.tag===tag?[child]:[]),...all(child,tag)
 ]);
}
const point=(start,amount,count,missing=0)=>({
 start,periods:1,missing,
 total:missing?{amount:null,count:null}:{amount,count,average:null}
});
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('timeline never joins missing observations or displays them as zero',()=>{
 const {window,e}=mount();
 window.nativeTimeline.show({granularity:'period',points:[
   point('2024-02-28',20,2),point('2024-02-29',0,0,1),point('2024-03-01',-10,-1)
 ]});
 assert.equal(e.timelineRows.children.length,3);
 assert.equal(e.timelineRows.children[1].children[4].textContent,'—');
 assert.equal(e.timelineRows.children[0].children[4].textContent,'20');
 const svg=e.timelineChart.children[0];
 assert.equal(svg.tag,'svg');
 assert.equal(all(svg,'circle').length,2);
 assert.equal(all(svg,'path').length,0,'missing data must interrupt the line');
 assert.match(e.timelineStatus.textContent,/2 из 3/);
 e.timelineMode.value='bar';e.timelineMode.events.change();
 assert.equal(all(e.timelineChart.children[0],'rect').length,2);
 assert.ok(all(e.timelineChart.children[0],'rect').some(b=>b.attrs.class.includes('negative')));
 e.timelineMetric.value='count';e.timelineMetric.events.change();
 assert.match(e.timelineStatus.textContent,/Количество \/ объём/);
 window.nativeTimeline.clear('Фильтр изменён');
 assert.equal(e.timelineChart.children.length,0);
 assert.equal(e.timelineRows.children.length,0);
});
test('single period uses a zero-based bar and valid zero is not missing',()=>{
 const {window,e}=mount();
 window.nativeTimeline.show({granularity:'period',points:[point('2024-03-01',0,0)]});
 assert.equal(all(e.timelineChart.children[0],'rect').length,1);
 assert.match(e.timelineStatus.textContent,/1 из 1/);
});
test('untrusted labels are added as text and oversized results are rejected',()=>{
 const {window,e}=mount();
 window.nativeTimeline.show({granularity:'month',points:[point('<img src=x>',4,2)]});
 assert.match(e.timelineStatus.textContent,/Невозможно построить/);
 assert.equal(e.timelineChart.children.length,0);
 window.nativeTimeline.show({granularity:'month',points:Array.from({length:125},(_,i)=>point('2024-01',i,i))});
 assert.equal(e.timelineChart.children.length,0);
 window.nativeTimeline.show({granularity:'month',points:[{
   start:'2024-01',periods:30,missing:30,total:{count:null,amount:null}
 }]});
 assert.equal(e.timelineChart.children.length,0);
 assert.equal(e.timelineRows.children.length,1);
 assert.match(e.timelineStatus.textContent,/не показаны нулями/);
});

test('empty-period checkbox rerenders graph and exact table without hiding missing data',()=>{
 const {window,e}=mount();
 const data={granularity:'period',points:[point('2024-01-01',0,0),point('2024-01-02',0,0,1),point('2024-01-03',5,1)]};
 window.nativeTimeline.show(data);
 assert.equal(e.timelineRows.children.length,3);
 e.hideEmptyPeriods.checked=true;e.hideEmptyPeriods.events.change();
 assert.equal(e.timelineRows.children.length,2);
 assert.equal(e.timelineRows.children[0].children[4].textContent,'—');
 assert.equal(all(e.timelineChart.children[0],'circle').length,1);
 assert.equal(data.points.length,3);
 e.hideEmptyPeriods.checked=false;e.hideEmptyPeriods.events.change();
 assert.equal(e.timelineRows.children.length,3);
});

test('hiding all zero timeline points reports confirmed hidden periods',()=>{
 const {window,e}=mount();e.hideEmptyPeriods.checked=true;
 window.nativeTimeline.show({granularity:'period',points:[point('2024-01-01',0,0)]});
 assert.match(e.timelineStatus.textContent,/нулевые и скрыты/);
});

test('annual timeline axis includes years instead of identical January labels',()=>{
 const {window,e}=mount();window.nativeTimeline.show({granularity:'year',points:[point('2023-01-01',3,1),point('2024-01-01',4,1)]});
 const labels=all(e.timelineChart.children[0],'text').map(n=>n.textContent);
 assert.ok(labels.includes('2023'));assert.ok(labels.includes('2024'));
});
