'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=fs.readFileSync(path.join(__dirname,'../unified-horizontal-bars.js'),'utf8');
class Element{
 constructor(name){this.tagName=name.toUpperCase();this.children=[];this.attributes={};this.dataset={};this.listeners={};this.style={minWidth:'',setProperty(){}};this.clientWidth=800;this._text='';}
 set textContent(v){this._text=String(v);this.children=[];}
 get textContent(){return this._text+this.children.map(c=>typeof c==='string'?c:c.textContent||'').join('');}
 setAttribute(k,v){this.attributes[k]=String(v);}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=children;this._text='';}
 addEventListener(name,fn){this.listeners[name]=fn;}
}
function all(node){return [node,...node.children.flatMap(c=>c instanceof Element?all(c):[])];}
function fixture(){
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),unifiedOriginal:{fmt:v=>String(v),displayPeriod:p=>p.start}};
 const document={createElementNS:(_ns,name)=>new Element(name),createElement:name=>new Element(name),createTextNode:text=>{const node=new Element('#text');node.textContent=text;return node;}};
 vm.runInNewContext(script,{window,document});
 return {window,document,root:new Element('div')};
}
test('horizontal chart keeps missing bars absent while zero remains a value',()=>{
 const {window,root}=fixture();
 const periods=[{start:'2026-10-12'},{start:'2026-10-13'},{start:'2026-10-14'}];
 const data=[new Map([['work:A',{amount:0}]]),null,new Map([['work:A',{amount:15}]])];
 window.unifiedHorizontalBars(periods,['work:A'],data,'amount',root,{label:()=> 'A',pointNotice:()=> 'Нет полного отчёта'},()=> 'var(--accent)',()=>{});
 const bars=all(root).filter(node=>node.tagName==='RECT');
 assert.equal(bars.length,2,'missing point must not be rendered as a zero bar');
 assert.match(bars[0].attributes['aria-label'],/2026-10-12: 0/);
 assert.match(root.textContent,/Нет полного отчёта/);
});
test('horizontal chart paginates long periods and keeps the renderer bounded',()=>{
 const {window,root}=fixture();
 const periods=Array.from({length:25},(_,i)=>({start:'2026-10-'+String(i+1).padStart(2,'0')}));
 const data=periods.map(()=>new Map([['A',{count:2}],['B',{count:3}]]));
 const options={label:name=>name},draw=()=>{root.replaceChildren();window.unifiedHorizontalBars(periods,['A','B'],data,'count',root,options,name=>name,draw);};
 draw();
 assert.equal(all(root).filter(node=>node.tagName==='RECT').length,24);
 const host=all(root).find(n=>n.dataset.sqlScrollHost==='chart-periods');
 assert.equal(host.scrollWindow.total,periods.length);
 assert.equal(host.scrollWindow.size,12);
 host.scrollWindow.go(1);
 assert.equal(options.periodPage,1);
 assert.ok(all(root).includes(host),'redraw keeps the pager host');
 assert.equal(all(root).filter(node=>node.tagName==='RECT').length,24);
 host.scrollWindow.go(0);
 assert.equal(options.periodPage,0);
});

test('department number matches the horizontal bar and legend after a missing first period',()=>{
 const {window,root}=fixture(),periods=[{start:'2026-10-12'},{start:'2026-10-13'}];
 const data=[new Map([['Отдел',{count:null}]]),new Map([['Отдел',{count:3}]])];
 window.unifiedHorizontalBars(periods,['Отдел'],data,'count',root,{},()=> 'var(--plot-0)',()=>{},new Map([['Отдел',9]]));
 assert.equal(all(root).filter(node=>node.tagName==='TEXT'&&node.textContent==='9').length,1);
 assert.ok(all(root).some(node=>node.tagName==='SPAN'&&node.textContent==='9 · Отдел'));
 assert.equal(all(root).filter(node=>node.dataset.chartTooltip).length,1);
});
