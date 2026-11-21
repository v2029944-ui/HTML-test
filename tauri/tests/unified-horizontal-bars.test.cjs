'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=fs.readFileSync(path.join(__dirname,'../unified-horizontal-bars.js'),'utf8');
const page=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8');
const axes=vm.runInNewContext(page.slice(page.indexOf('function chartNiceScale('),page.indexOf('const defaultHideZeroRankingRows'))+';({chartNiceScale,chartTickLabel})');
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
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),unifiedOriginal:{...axes,fmt:v=>String(v),displayPeriod:p=>p.start}};
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
test('horizontal pages use a shared scale and zero stays on the nonnegative axis',()=>{
 const {window,root}=fixture(),periods=Array.from({length:14},(_,i)=>({start:String(i+1)}));
 const data=periods.map((_,i)=>new Map([['A',{count:i===13?100:20}]]));
 const options={},draw=()=>{root.replaceChildren();window.unifiedHorizontalBars(periods,['A'],data,'count',root,options,()=>'',draw);};
 draw();const width=all(root).find(n=>n.tagName==='RECT').attributes.width;
 all(root).find(n=>n.dataset.sqlScrollHost==='chart-periods').scrollWindow.go(1);
 assert.equal(all(root).find(n=>n.tagName==='RECT').attributes.width,width);
 root.replaceChildren();window.unifiedHorizontalBars(periods.slice(0,1),['A'],[new Map([['A',{count:0}]])],'count',root,{},()=>'',()=>{});
 const ticks=all(root).filter(n=>n.tagName==='TEXT'&&n.attributes.y==='16');
 assert.equal(ticks[0].textContent,'0');
 assert.equal(new Set(ticks.map(n=>n.textContent)).size,ticks.length);
});
test('undefined averages in complete reports identify a zero divisor without calling it a missing report',()=>{
 const {window,root}=fixture(),periods=[{start:'1'},{start:'2'}];
 const data=periods.map(()=>new Map([['A',{count:0,amount:15,average:null}]]));
 window.unifiedHorizontalBars(periods,['A'],data,'average',root,{},()=>'',()=>{});
 assert.doesNotMatch(root.textContent,/Нет полных данных|Пропуски/);
 assert.match(root.textContent,/нулевом количестве|количество равно нулю/);
 assert.equal(all(root).filter(n=>n.tagName==='RECT').length,0);
});
