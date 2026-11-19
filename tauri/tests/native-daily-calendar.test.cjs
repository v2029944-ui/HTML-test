'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const selection=fs.readFileSync(path.join(__dirname,'../native-period-selection.js'),'utf8');
const calendar=fs.readFileSync(path.join(__dirname,'../native-daily-calendar.js'),'utf8');
class Element{
 constructor(tag){this.tag=tag;this.value='';this.type='date';this.textContent='';this.children=[];this.events={};this.className='';this.disabled=false;this.hidden=false;this.attributes={};}
 append(...items){this.children.push(...items);}
 replaceChildren(...items){this.children=[...items];}
 addEventListener(name,fn){this.events[name]=fn;}
 setAttribute(name,value){this.attributes[name]=String(value);}
}
const point=(start,days,reports,missing,count,amount)=>({
 start,days,available_reports:reports,missing,
 complete:missing===0&&reports>0,
 total:missing===0&&reports>0?{count,amount}:{count:null,amount:null},
 available_total:reports>0?{count,amount}:{count:null,amount:null}
});
function mount(options={}){
 const ids=['analysisFrom','analysisTo','comparisonBeforeFrom','comparisonBeforeTo','analysisPeriodHelp',
 'dailyRollupGranularity','dailyRollupFrom','dailyRollupTo','dailyRollupPrepare','dailyRollupRun',
 'dailyRollupBack','dailyRollupNext','dailyRollupPageInfo','dailyRollupRows','dailyRollupScope','dailyRollupStatus',
 'dailyRollupDetailPanel','dailyRollupDetailTitle','dailyRollupDetailClose','dailyRollupDetailStatus',
 'dailyRollupDetailBack','dailyRollupDetailNext','dailyRollupDetailPage','dailyRollupDetailRows'];
 const nodes=Object.fromEntries(ids.map(id=>[id,new Element(id)]));
 nodes.dailyRollupGranularity.value='day';
 const calls=[];
 const api={
  nativeAnalysisIndexPage:async(rev,kind,cursor,limit)=>{calls.push({op:'index',rev,kind,cursor,limit});return {processed:3,done:true,next:null};},
  nativeAnalysisIndexSettings:async rev=>{calls.push({op:'settings',rev});},
  nativeDailyRollup:async(rev,from,to,kind,orgs,after,limit)=>{
   calls.push({op:'page',rev,from,to,kind,orgs,after,limit});
   if(options.fail)throw Error('Synthetic index failure');
   if(options.page)return options.page({rev,from,to,kind,orgs,after,limit});
   return {granularity:kind,points:[
    point(from,1,2,0,0,69),
    point('2024-03-01',1,1,1,1,10)
   ],next_cursor:null};
  }
 };
 const window={desktopAPI:api,analizUserError:e=>String(e.message||e)};
 const document={getElementById:id=>nodes[id],createElement:tag=>new Element(tag)};
 vm.runInNewContext(selection,{window,document});
 vm.runInNewContext(calendar,{window,document});
 return {nodes,window,calls};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('native daily calendar module is bundled before SQL workspace and not classic HTML',()=>{
 const root=path.resolve(__dirname,'../..'),output=fs.mkdtempSync(path.join(os.tmpdir(),'native-daily-calendar-'));
 try{
  prepare({base:root,output});
  const html=fs.readFileSync(path.join(output,'native-workspace.html'),'utf8');
  assert.equal(fs.readFileSync(path.join(output,'tauri-native-daily-calendar.js'),'utf8'),calendar);
  assert.ok(html.indexOf('tauri-native-period-selection.js')<html.indexOf('tauri-native-daily-calendar.js'));
  assert.ok(html.indexOf('tauri-native-daily-calendar.js')<html.indexOf('tauri-native-workspace.js'));
  assert.match(html,/id="sqlDailyRollups"/);
  assert.ok(!fs.readFileSync(path.join(root,'index.html'),'utf8').includes('tauri-native-daily-calendar.js'));
 }finally{fs.rmSync(output,{recursive:true,force:true});}
});
test('daily rollup selects ISO week full bounds with exact chosen organizations and revision',async()=>{
 const {nodes,window,calls}=mount();
 window.nativeDailyCalendar.setContext(42,['A','B']);
 assert.equal(nodes.dailyRollupPrepare.disabled,false);
 nodes.dailyRollupGranularity.value='week';
 nodes.dailyRollupGranularity.events.change();
 assert.equal(nodes.dailyRollupFrom.type,'week');
 nodes.dailyRollupFrom.value='2024-W09';
 nodes.dailyRollupTo.value='2024-W09';
 nodes.dailyRollupPrepare.events.click();await settle();
 assert.equal(nodes.dailyRollupRun.disabled,false);
 nodes.dailyRollupRun.events.click();await settle();
 const q=calls.find(c=>c.op==='page');
 assert.equal(q.rev,42);assert.equal(q.kind,'week');
 assert.equal(q.from,'2024-02-26');assert.equal(q.to,'2024-03-03');
 assert.equal(q.orgs.join(','),'A,B');
 assert.equal(q.limit,60);
 assert.ok(calls.some(c=>c.op==='index'&&c.kind==='day'));
 assert.equal(nodes.dailyRollupRows.children.length,2);
 assert.equal(nodes.dailyRollupRows.children[1].children[5].textContent,'10');
 assert.equal(nodes.dailyRollupRows.children[1].children[6].textContent,'Неполный · по имеющимся данным');
 assert.match(nodes.dailyRollupStatus.textContent,/Неполных: 1/);
});
test('year and month pickers use calendar boundaries and never add month source reports',async()=>{
 const {nodes,window,calls}=mount({page:({kind,from})=>({
  granularity:kind,points:[point(from,kind==='year'?366:29,3,kind==='year'?363:26,5,249)],next_cursor:null
 })});
 window.nativeDailyCalendar.setContext(3,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupGranularity.value='year';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024';nodes.dailyRollupTo.value='2024';
 nodes.dailyRollupRun.events.click();await settle();
 let q=calls.filter(c=>c.op==='page').at(-1);
 assert.equal(q.from,'2024-01-01');assert.equal(q.to,'2024-12-31');
 assert.equal(nodes.dailyRollupRows.children[0].children[0].textContent,'2024');
 nodes.dailyRollupGranularity.value='month';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024-02';nodes.dailyRollupTo.value='2024-02';
 nodes.dailyRollupRun.events.click();await settle();
 q=calls.filter(c=>c.op==='page').at(-1);
 assert.equal(q.from,'2024-02-01');assert.equal(q.to,'2024-02-29');
 assert.equal(nodes.dailyRollupRows.children[0].children[0].textContent,'2024-02');
});
test('keyset next/back keeps page state and changing orgs clears prior values',async()=>{
 const {nodes,window,calls}=mount({page:({after})=>!after?
  {granularity:'day',points:[point('2024-02-28',1,1,0,0,0)],
   next_cursor:'2024-02-28'}:
  {granularity:'day',points:[point('2024-02-29',1,1,0,-3,-30)],next_cursor:null}
 });
 window.nativeDailyCalendar.setContext(7,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupFrom.value='2024-02-28';nodes.dailyRollupTo.value='2024-02-29';
 nodes.dailyRollupRun.events.click();await settle();
 assert.equal(nodes.dailyRollupRows.children[0].children[5].textContent,'0');
 assert.equal(nodes.dailyRollupNext.disabled,false);
 nodes.dailyRollupNext.events.click();await settle();
 assert.equal(nodes.dailyRollupRows.children[0].children[5].textContent,'-30');
 assert.equal(nodes.dailyRollupPageInfo.textContent,'Страница 2');
 nodes.dailyRollupBack.events.click();await settle();
 assert.equal(nodes.dailyRollupPageInfo.textContent,'Страница 1');
 assert.equal(nodes.dailyRollupRows.children[0].children[5].textContent,'0');
 window.nativeDailyCalendar.setContext(7,['B']);
 assert.equal(nodes.dailyRollupRows.children.length,0);
 assert.equal(nodes.dailyRollupNext.disabled,true);
 assert.equal(nodes.dailyRollupRun.disabled,false);
 assert.equal(calls.filter(c=>c.op==='page').length,3);
});
test('invalid or oversized daily rollup responses cannot appear as confirmed numbers',async()=>{
 const {nodes,window}=mount({page:()=>({granularity:'day',points:[
  point('2024-02-28',1,0,1,0,0),...Array.from({length:60},(_,i)=>point('2024-02-28',1,1,0,i,i))
 ],next_cursor:null})});
 window.nativeDailyCalendar.setContext(3,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupFrom.value='2024-02-28';nodes.dailyRollupTo.value='2024-02-29';
 nodes.dailyRollupRun.events.click();await settle();
 assert.equal(nodes.dailyRollupRows.children.length,0);
 assert.match(nodes.dailyRollupStatus.textContent,/Сводка недоступна/);
});

test('month detail is scoped to calendar and displays exact day evidence with close action',async()=>{
 const {window,nodes,calls}=mount({page:({kind,from,to})=>{
  if(kind==='day')return {granularity:kind,points:[
   point('2024-02-28',1,1,0,0,0),
   point('2024-02-29',1,0,1,0,0)
  ],next_cursor:null};
  return {granularity:kind,points:[point('2024-02-01',29,3,26,4,24)],next_cursor:null};
 }});
 window.nativeDailyCalendar.setContext(11,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupGranularity.value='month';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024-02';nodes.dailyRollupTo.value='2024-02';
 nodes.dailyRollupRun.events.click();await settle();
 const tr=nodes.dailyRollupRows.children[0];
 assert.equal(tr.children[7].children[0].textContent,'Показать дни');
 tr.children[7].children[0].events.click();await settle();
 const q=calls.filter(x=>x.op==='page').at(-1);
 assert.equal(q.kind,'day');
 assert.equal(q.from,'2024-02-01');
 assert.equal(q.to,'2024-02-29');
 assert.equal(q.orgs.join(','),'A');
 assert.equal(nodes.dailyRollupDetailPanel.hidden,false);
 assert.equal(nodes.dailyRollupDetailRows.children.length,2);
 assert.equal(nodes.dailyRollupDetailRows.children[0].children[4].textContent,'0');
 assert.equal(nodes.dailyRollupDetailRows.children[1].children[4].textContent,'—');
 assert.equal(nodes.dailyRollupDetailRows.children[1].children[5].textContent,'Отчётов нет');
 nodes.dailyRollupDetailClose.events.click();
 assert.equal(nodes.dailyRollupDetailPanel.hidden,true);
 assert.equal(nodes.dailyRollupDetailRows.children.length,0);
 assert.equal(nodes.dailyRollupRows.children.length,1,'closing only the detail retains month summary');
});
test('year detail uses a separate 60-day keyset pager and clears on scope change',async()=>{
 let pageIndex=0;
 const {window,nodes,calls}=mount({page:({kind,after,from,to})=>{
  if(kind==='year')return {granularity:'year',points:[
   point('2024-01-01',366,100,266,400,1000)
  ],next_cursor:null};
  pageIndex++;
  if(after===null)return {granularity:'day',points:[
   point('2024-01-01',1,1,0,0,0)
  ],next_cursor:'2024-01-01'};
  return {granularity:'day',points:[
   point('2024-01-02',1,1,0,-2,-9)
  ],next_cursor:null};
 }});
 window.nativeDailyCalendar.setContext(12,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupGranularity.value='year';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024';nodes.dailyRollupTo.value='2024';
 nodes.dailyRollupRun.events.click();await settle();
 nodes.dailyRollupRows.children[0].children[7].children[0].events.click();await settle();
 let request=calls.filter(x=>x.op==='page').at(-1);
 assert.equal(request.from,'2024-01-01');
 assert.equal(request.to,'2024-12-31');
 assert.equal(nodes.dailyRollupDetailNext.disabled,false);
 nodes.dailyRollupDetailNext.events.click();await settle();
 assert.equal(nodes.dailyRollupDetailPage.textContent,'Страница 2');
 assert.equal(nodes.dailyRollupDetailRows.children[0].children[4].textContent,'-9');
 nodes.dailyRollupDetailBack.events.click();await settle();
 assert.equal(nodes.dailyRollupDetailPage.textContent,'Страница 1');
 assert.equal(nodes.dailyRollupDetailRows.children[0].children[4].textContent,'0');
 window.nativeDailyCalendar.setContext(12,['B']);
 assert.equal(nodes.dailyRollupDetailPanel.hidden,true);
 assert.equal(nodes.dailyRollupRows.children.length,0);
 assert.equal(nodes.dailyRollupDetailRows.children.length,0);
 assert.ok(pageIndex>=3);
});
test('stale day detail cannot reappear after calendar filter changes',async()=>{
 let resolveDetail;
 const {window,nodes}=mount({page:({kind,from})=>{
  if(kind==='year')return {granularity:'year',points:[
   point('2024-01-01',366,1,365,1,4)
  ],next_cursor:null};
  return new Promise(resolve=>{resolveDetail=resolve;});
 }});
 window.nativeDailyCalendar.setContext(9,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupGranularity.value='year';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024';nodes.dailyRollupTo.value='2024';
 nodes.dailyRollupRun.events.click();await settle();
 nodes.dailyRollupRows.children[0].children[7].children[0].events.click();
 assert.equal(typeof resolveDetail,'function');
 nodes.dailyRollupDetailClose.events.click();
 resolveDetail({granularity:'day',points:[point('2024-01-01',1,1,0,0,4)],next_cursor:null});
 await settle();
 assert.equal(nodes.dailyRollupDetailPanel.hidden,true);
 assert.equal(nodes.dailyRollupDetailRows.children.length,0);
});
