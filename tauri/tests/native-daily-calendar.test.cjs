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
 'dailyRollupPeriodBack','dailyRollupPeriodNext','dailyRollupBack','dailyRollupNext','dailyRollupPageInfo','dailyRollupRows','dailyRollupScope','dailyRollupStatus',
 'dailyRollupDetailPanel','dailyRollupDetailTitle','dailyRollupDetailClose','dailyRollupDetailStatus',
 'dailyRollupDetailBack','dailyRollupDetailNext','dailyRollupDetailPage','dailyRollupDetailRows'];
 const nodes=Object.fromEntries(ids.map(id=>[id,new Element(id)]));
 nodes.dailyRollupGranularity.value='day';
 const calls=[];
 const api={
  nativeAnalysisIndexPage:async(rev,kind,cursor,limit)=>{calls.push({op:'index',rev,kind,cursor,limit});return {processed:3,done:true,next:null};},
  nativeAnalysisIndexSettings:async rev=>{calls.push({op:'settings',rev});},
  nativeAnalysisRange:async(rev,kind,from,to,orgs,selectedName,selectedNames)=>{
   calls.push({op:'range',rev,kind,from,to,orgs,selectedName,selectedNames});
   const result={complete:false,periods:7,organizations:orgs.length,missing:1,
    available_reports:6,available_total:{count:2,amount:11,average:5.5},
    total:{count:null,amount:null,average:null}};
   if(options.delayedOverview)return new Promise(resolve=>{options.resolveOverview=()=>resolve(result);});
   return result;
  },
  nativeAnalysisTimeline:async(rev,kind,from,to,orgs,selectedName,grouping,selectedNames)=>{
   calls.push({op:'timeline',rev,kind,from,to,orgs,selectedName,selectedNames});
   return {granularity:'period',points:[]};
  },
  nativeAnalysisOrganizations:async(rev,kind,from,to,orgs,selectedName,selectedNames)=>{
   calls.push({op:'divisions',rev,kind,from,to,orgs,selectedName,selectedNames});
   if(options.failDivisions)throw Error('Division data unavailable');
   return orgs.map(org=>({org,complete:true,missing:0,
    total:{count:0,amount:0,average:null}}));
  },
  nativeDailyRollup:async(rev,from,to,kind,orgs,after,limit,selectedName,selectedNames)=>{
   calls.push({op:'page',rev,from,to,kind,orgs,after,limit,selectedName,selectedNames});
   if(options.fail)throw Error('Synthetic index failure');
   if(options.page)return options.page({rev,from,to,kind,orgs,after,limit});
   return {granularity:kind,points:[
    point(from,1,2,0,0,69),
    point('2024-03-01',1,1,1,1,10)
   ],next_cursor:null};
  }
 };
 const overview=[];
 const window={desktopAPI:api,analizUserError:e=>String(e.message||e),
 nativeDailyOverview:{
  show:(summary,timeline)=>overview.push({op:'show',summary,timeline}),
  showOrganizations:(divisions,organizations)=>overview.push({op:'divisions',divisions,organizations}),
  organizationsError:message=>overview.push({op:'divisionsError',message}),
  clear:message=>overview.push({op:'clear',message}),
  error:message=>overview.push({op:'error',message})
 }};
 const document={getElementById:id=>nodes[id],createElement:tag=>new Element(tag)};
 vm.runInNewContext(selection,{window,document});
 vm.runInNewContext(calendar,{window,document,TextEncoder});
 return {nodes,window,calls,overview};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
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
 window.nativeDailyCalendar.setContext(11,['A'],['Group','Работа']);
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
 const filtered=calls.filter(x=>['range','timeline','divisions','page'].includes(x.op));
 assert.deepEqual(filtered.map(x=>x.op).sort(),['divisions','page','page','range','timeline']);
 for(const call of filtered){
  assert.equal(call.selectedName,null,call.op);
  assert.deepEqual(Array.from(call.selectedNames),['Group','Работа'],call.op);
 }
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


test('daily source KPIs and graph use exactly the same source, dates and organizations as calendar',async()=>{
 const {nodes,window,calls,overview}=mount({page:({kind,from})=>({
  granularity:kind,points:[point(from,7,2,5,0,0)],next_cursor:null
 })});
 window.nativeDailyCalendar.setContext(8,['A','B']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupGranularity.value='week';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024-W09';nodes.dailyRollupTo.value='2024-W09';
 nodes.dailyRollupRun.events.click();await settle();await settle();
 const pageCall=calls.find(c=>c.op==='page');
 const summary=calls.find(c=>c.op==='range');
 const timeline=calls.find(c=>c.op==='timeline');
 assert.equal(summary.kind,'day');
 assert.equal(timeline.kind,'day');
 assert.equal(summary.rev,pageCall.rev);
 assert.equal(summary.from,pageCall.from);
 assert.equal(timeline.to,pageCall.to);
 assert.equal(summary.orgs.join(','),'A,B');
 assert.equal(timeline.orgs.join(','),'A,B');
 assert.equal(overview.find(x=>x.op==='show').summary.available_total.amount,11);
 assert.equal(overview.at(-1).op,'divisions');
 assert.equal(overview.at(-1).organizations.join(','),'A,B');
 const divisionCall=calls.find(x=>x.op==='divisions');
 assert.equal(divisionCall.kind,'day');
 assert.equal(divisionCall.from,pageCall.from);
 assert.equal(divisionCall.to,pageCall.to);
 assert.equal(divisionCall.rev,pageCall.rev);
 assert.equal(nodes.dailyRollupRows.children.length,1);
});
test('late day-source KPI response cannot restore results after changing organizations',async()=>{
 const opts={delayedOverview:true,page:({kind,from})=>({
  granularity:kind,points:[point(from,1,1,0,0,0)],next_cursor:null
 })};
 const {nodes,window,calls,overview}=mount(opts);
 window.nativeDailyCalendar.setContext(19,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupFrom.value='2024-02-28';
 nodes.dailyRollupTo.value='2024-02-28';
 nodes.dailyRollupRun.events.click();await settle();
 assert.equal(typeof opts.resolveOverview,'function');
 window.nativeDailyCalendar.setContext(19,['B']);
 opts.resolveOverview();await settle();
 assert.ok(!overview.some(x=>x.op==='show'));
 assert.equal(overview.at(-1).op,'clear');
 assert.equal(calls.filter(x=>x.op==='timeline').length,0);
});

test('division query failure preserves verified day KPI and chart',async()=>{
 // One selected organization can provide only one report per calendar day.
 const {nodes,window,overview}=mount({failDivisions:true,
  page:({kind,from})=>({
   granularity:kind,
   points:[point(from,1,1,0,0,0)],
   next_cursor:null
  })
 });
 window.nativeDailyCalendar.setContext(20,['A']);
 nodes.dailyRollupPrepare.events.click();await settle();
 nodes.dailyRollupFrom.value='2024-02-28';nodes.dailyRollupTo.value='2024-02-28';
 nodes.dailyRollupRun.events.click();await settle();await settle();
 assert.ok(overview.some(x=>x.op==='show'));
 assert.equal(overview.at(-1).op,'divisionsError');
 assert.match(overview.at(-1).message,/недоступны/);
});
test('calendar period navigation crosses leap February and invalidates old summaries',()=>{
 const {nodes,window}=mount();
 window.nativeDailyCalendar.setContext(12,['A']);
 nodes.dailyRollupGranularity.value='month';nodes.dailyRollupGranularity.events.change();
 nodes.dailyRollupFrom.value='2024-01';nodes.dailyRollupTo.value='2024-01';
 nodes.dailyRollupPeriodNext.events.click();
 assert.equal(nodes.dailyRollupFrom.value,'2024-02');assert.equal(nodes.dailyRollupTo.value,'2024-02');
 nodes.dailyRollupPeriodNext.events.click();
 assert.equal(nodes.dailyRollupTo.value,'2024-03');
 nodes.dailyRollupPeriodBack.events.click();assert.equal(nodes.dailyRollupTo.value,'2024-02');
});
