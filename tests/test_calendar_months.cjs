const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const assert=require('node:assert/strict');
const {test}=require('node:test');
const html=fs.readFileSync(path.join(__dirname,'../index.html'),'utf8');
let app=[...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)].at(-1)[1].replace(/\n(?:window\.appReady=)?main\(\);\s*$/,'\n');
class Node {
  constructor(tag){this.tag=tag;this.children=[];this.dataset={};this.attributes={};this.listeners={};this.style={setProperty(){}};this.value='';}
  append(...nodes){for(const node of nodes){this.children.push(node);if(node&&typeof node==='object')node.parent=this;}}
  replaceChildren(...nodes){this.children=[];this.append(...nodes);}
  setAttribute(name,value){this.attributes[name]=value;}
  addEventListener(name,callback){this.listeners[name]=callback;}
  querySelectorAll(tag){return this.children.flatMap(node=>node instanceof Node?[...(node.tag===tag?[node]:[]),...node.querySelectorAll(tag)]:[]);}
  remove(){if(this.parent)this.parent.children=this.parent.children.filter(node=>node!==this);}
  scrollIntoView(){}
}
const nodes=new Map();
const context=vm.createContext({console,window:{matchMedia:()=>({matches:false})},document:{getElementById:id=>{if(!nodes.has(id))nodes.set(id,new Node('div'));return nodes.get(id);},createElement:tag=>new Node(tag),createElementNS:(_ns,tag)=>new Node(tag)}});
vm.runInContext(app,context);
const run=code=>vm.runInContext(code,context);
run(`function calendarFixture(date,organizations=['A'],extra={}){return {id:date,type:'day',start:date,end:date,organizations,rows:organizations.map(organization=>({organization,work:'Работа',count:2,amount:10})),totals:[],warnings:[],...extra};}`);

test('12 month cards render even with no reports, and opening a leap February creates just 29 days',()=>{
  run(`state.reports=[];state.settings.orgNames=['A'];$('calendarType').value='all';calendarYear=2024;calendarSelectedMonth=null;renderReportCalendar();`);
  assert.equal(nodes.get('reportCalendar').children.length,12);
  assert.equal(nodes.get('calendarDetails').children.length,0);
  nodes.get('reportCalendar').children[1].listeners.click();
  assert.equal(nodes.get('calendarDetails').querySelectorAll('button').filter(node=>node.dataset.date).length,29);
  assert.equal(nodes.get('reportCalendar').children[1].attributes['aria-expanded'],'true');
});
test('confirmed gaps exclude future days and preserve unknown coverage as neutral',()=>{
  const result=run(`(()=>{const reports=[calendarFixture('2024-02-01',['A'],{sourceCoverage:{start:'2024-02-01',end:'2024-02-29'}})];return {known:calendarMonthModel(reports,2024,1,['A'],'2024-02-03'),unknown:calendarMonthModel([],2024,1,['A'],'2024-03-01')};})()`);
  assert.equal(result.known.missing,2);assert.equal(result.known.kind,'error');
  assert.equal(result.unknown.missing,0);assert.equal(result.unknown.kind,'missing');assert.equal(result.unknown.count,null);
});
test('all leap days with all organizations prove completeness; zero remains available',()=>{
  const result=run(`(()=>{const reports=Array.from({length:29},(_,i)=>calendarFixture('2024-02-'+String(i+1).padStart(2,'0'),['A'],{rows:[{organization:'A',work:'Работа',count:0,amount:0}]}));return calendarMonthModel(reports,2024,1,['A'],'2024-03-01');})()`);
  assert.equal(result.kind,'complete');assert.equal(result.loaded,29);assert.equal(result.count,0);assert.equal(result.amount,0);
});
test('daily sums never include monthly sources or apportioned cross-month weeks',()=>{
  const result=run(`(()=>{const reports=[calendarFixture('2024-02-01'),calendarFixture('2024-02-02'),calendarFixture('2024-01-29',['A'],{type:'week',end:'2024-02-04',amount:999}),calendarFixture('2024-02-01',['A'],{type:'month',end:'2024-02-29',amount:10000})];return calendarMonthModel(reports,2024,1,['A'],'2024-03-01');})()`);
  assert.equal(result.count,4);assert.equal(result.amount,20);assert.equal(result.weekly.length,1);assert.equal(result.monthly.length,1);assert.equal(result.kind,'warn');
});
test('duplicate same-organization days are excluded, and disjoint organizations follow analytics semantics',()=>{
  const result=run(`(()=>{const day=calendarFixture('2024-02-01');return {duplicate:calendarMonthModel([day,{...day,id:'copy'}],2024,1,['A'],'2024-03-01'),split:calendarMonthModel([day,calendarFixture('2024-02-01',['B'])],2024,1,['A','B'],'2024-03-01')};})()`);
  assert.equal(result.duplicate.amount,null);assert.equal(result.duplicate.kind,'error');assert.equal(result.duplicate.daily.get('2024-02-01').conflict,true);
  assert.equal(result.split.amount,20);assert.equal(result.split.daily.get('2024-02-01').complete,true);
});
test('serious source issues, unknown warnings, and missing organizations have distinct statuses',()=>{
  const result=run(`(()=>{const get=extra=>calendarMonthModel([calendarFixture('2024-02-01',['A'],extra)],2024,1,['A'],'2024-03-01');return {error:get({issues:[{type:'invalid',reason:'Лист1!B4: неверное число'}]}),warning:get({warnings:['Проверьте источник']}),partial:calendarMonthModel([calendarFixture('2024-02-01')],2024,1,['A','B'],'2024-03-01')};})()`);
  assert.equal(result.error.kind,'error');assert.equal(result.warning.kind,'warn');assert.equal(result.partial.kind,'error');
});
test('day expansion exposes Excel filename, sheet, organizations, and quality navigation',()=>{
  run(`state.settings.orgNames=['A'];state.reports=[calendarFixture('2024-02-01',['A'],{fileName:'Год.xlsx',sheetName:'I квартал'})];calendarYear=2024;calendarSelectedMonth=1;renderReportCalendar();`);
  const root=nodes.get('calendarDetails'),day=root.querySelectorAll('button').find(node=>node.dataset.date==='2024-02-01');day.listeners.click();
  const text=node=>typeof node==='object'?String(node.textContent||'')+' '+node.children.map(text).join(' '):String(node);
  assert.match(text(root),/Год.xlsx/);assert.match(text(root),/I квартал/);assert.match(text(root),/Подразделения \(1\): A/);
  assert.ok(root.querySelectorAll('button').some(node=>String(node.textContent).startsWith('Контроль качества:')));
});
test('weekly overlap is an actual conflict and no daily total is manufactured',()=>{
  const result=run(`calendarMonthModel([calendarFixture('2024-02-01',['A'],{type:'week',end:'2024-02-07'}),calendarFixture('2024-02-02',['A'],{type:'week',end:'2024-02-08'})],2024,1,['A'],'2024-03-01')`);
  assert.equal(result.kind,'error');assert.equal(result.amount,null);assert.equal(result.loaded,0);
});

test('local day diagnostics and source-period diagnostics have separate quality actions',()=>{
 run(`state.settings.orgNames=['A'];state.reports=[calendarFixture('2024-01-01'),calendarFixture('2024-02-15',['A'],{sourceControlDate:'2024-01-01',issues:[{type:'invalid',reason:'Local error'}]})];renderQuality=()=>{};`);
 const block=run(`calendarSourceDetails(state.reports[1])`),buttons=block.querySelectorAll('button');
 assert.equal(buttons.length,2);buttons[0].listeners.click();assert.equal(nodes.get('qualityPeriod').value,'day:2024-02-15');
 buttons[1].listeners.click();assert.equal(nodes.get('qualityPeriod').value,'day:2024-01-01');
});
