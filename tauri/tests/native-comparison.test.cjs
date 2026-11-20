'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-comparison.js'),'utf8');
class Element{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.className='';this.value='';this.style={};this.attributes={};this.events={};}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=[...nodes];}
 setAttribute(name,value){this.attributes[name]=String(value);}
 addEventListener(name,fn){this.events[name]=fn;}
}
function mount(){
 const ids=['comparisonStatus','comparisonMetrics','comparisonDivisionRows','comparisonRankRows','comparisonRankStatus','comparisonRankMetric'];
 const e=Object.fromEntries(ids.map(x=>[x,new Element(x)]));
 e.comparisonRankMetric.value='amount';
 const window={},document={getElementById:id=>e[id],createElement:t=>new Element(t)};
 vm.runInNewContext(script,{window,document});
 return {window,e};
}
const m=(base,current,percent=base===0?null:(current-base)/Math.abs(base)*100)=>({
 base,current,delta:current-base,percent,negative:base<0||current<0,smallBase:base===0,notable:true,significant:true
});
const t=(count,amount,complete=true)=>({count:complete?count:null,amount:complete?amount:null,average:null});
function dashboard(){
 return {
  summary:{
   before:{complete:true,total:t(0,40)},
   after:{complete:true,total:t(2,-10)},
   values:2,count:m(0,2),amount:m(40,-10),average:{base:null,current:null,delta:null,percent:null}
  },
  divisions:[
   {organization:'<b>А</b>',before:t(0,40),after:t(2,-10),count:m(0,2),amount:m(40,-10)},
   {organization:'B',before:t(0,0),after:t(0,0),count:m(0,0),amount:m(0,0)}
  ],
  top_amount:[
   {key:'w1',name:'<img src=x>',kind:'work',amount:m(40,-10),count:m(0,2)},
   {key:'w2',name:'Новая',kind:'group',amount:m(0,0),count:m(0,0)}
  ],
  top_count:[
   {key:'w1',name:'Новая',kind:'work',amount:m(0,0),count:m(0,2)}
  ]
 };
}
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('render finite signed changes and undefined percentage when baseline is zero',()=>{
 const {window,e}=mount();
 window.nativeComparison.show(dashboard());
 assert.equal(e.comparisonMetrics.children.length,2);
 const first=e.comparisonMetrics.children[0];
 assert.match(first.children[3].textContent,/\+2/);
 assert.equal(first.children[4].textContent,'Изменение, %: —');
 const amount=e.comparisonMetrics.children[1];
 assert.match(amount.children[3].textContent,/-50/);
 assert.match(amount.children[4].textContent,/-125/);
 assert.equal(e.comparisonDivisionRows.children.length,2);
 assert.equal(e.comparisonDivisionRows.children[0].children[0].textContent,'<b>А</b>');
 assert.equal(e.comparisonRankRows.children.length,2);
 assert.match(e.comparisonRankRows.children[0].children[0].children[0].textContent,/<img src=x>/);
 assert.match(e.comparisonRankRows.children[0].children[1].children[0].className,/decrease/);
 e.comparisonRankMetric.value='count';
 e.comparisonRankMetric.events.change();
 assert.equal(e.comparisonRankRows.children.length,1);
 assert.match(e.comparisonRankStatus.textContent,/по модулю/);
});
test('partial totals are unknown and hide ranking, but complete division remains',()=>{
 const {window,e}=mount();
 const data=dashboard();
 data.summary.after.complete=false;data.summary.count=null;data.summary.amount=null;
 data.top_amount=[];data.top_count=[];
 data.divisions[1].after=t(0,0,false);
 data.divisions[1].count=null;data.divisions[1].amount=null;
 window.nativeComparison.show(data);
 assert.match(e.comparisonStatus.textContent,/есть пропуски/);
 assert.equal(e.comparisonMetrics.children[0].children[3].textContent,'Изменение: —');
 assert.equal(e.comparisonDivisionRows.children[0].children[3].textContent,'-50');
 assert.equal(e.comparisonDivisionRows.children[1].children[3].textContent,'—');
 assert.equal(e.comparisonRankRows.children.length,0);
 assert.match(e.comparisonRankStatus.textContent,/скрыта/);
});
test('malformed and oversized data never leaves stale or misleading comparison',()=>{
 const {window,e}=mount();
 window.nativeComparison.show(dashboard());
 let invalid=dashboard();invalid.summary.after.complete=false;
 window.nativeComparison.show(invalid);
 assert.equal(e.comparisonMetrics.children.length,0);
 assert.match(e.comparisonStatus.textContent,/Невозможно достоверно/);
 invalid=dashboard();
 invalid.top_amount=Array.from({length:13},(_,i)=>({...dashboard().top_amount[0],key:'x'+i}));
 window.nativeComparison.show(invalid);
 assert.equal(e.comparisonRankRows.children.length,0);
 window.nativeComparison.clear('Фильтры изменены');
 assert.equal(e.comparisonDivisionRows.children.length,0);
 assert.equal(e.comparisonStatus.textContent,'Фильтры изменены');
});
