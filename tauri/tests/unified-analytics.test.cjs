const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../unified-analytics.js'),'utf8');
const original=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8');
class Element{
 constructor(tag='div',text=''){this.tagName=tag.toUpperCase();this.children=[];this._text=text??'';this.value='';this.dataset={};this.listeners={};this.hidden=false;this.open=false;this.checked=false;this.classList={toggle(){}};}
 append(...children){this.children.push(...children);}replaceChildren(...children){this.children=children;this._text='';}
 get textContent(){return this._text+this.children.map(c=>typeof c==='string'?c:c.textContent).join('');}set textContent(v){this._text=String(v);this.children=[];}
 addEventListener(e,fn){(this.listeners[e]??=[]).push(fn);}dispatch(e){for(const fn of this.listeners[e]||[])fn({target:this});}setAttribute(){}
}
const settle=()=>new Promise(r=>setImmediate(r));
function deferred(){let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};}
function fixture(overrides={}){
 const nodes=new Map();for(const m of original.matchAll(/<([a-z][\w-]*)\b[^>]*\bid="([^"]+)"[^>]*>/gi)){const n=new Element(m[1]);n.id=m[2];nodes.set(n.id,n);}
 for(const id of ['unifiedSource','hideEmptyPeriods','chartOrientation','analysisBasePeriod','workViewMode']){const node=new Element('input');node.id=id;nodes.set(id,node);}
 for(const [id,v]of Object.entries({periodType:'day',unifiedSource:'daily',overviewMode:'single',chartBreakdown:'works',chartOrgScope:'all',chartSeriesLayout:'combined',chartOrientation:'vertical'}))nodes.get(id).value=v;
 const buttons=['line','bar','table'].map(chart=>{const n=new Element('button');n.dataset.chart=chart;return n;}),draws=[],tables=[],published=[],multi=new Map(),calls=[],notices=[];
 let snapshotHook,viewHook,prints=0;
 const totals={count:2,amount:20,average:10},summary={complete:true,periods:1,organizations:1,missing:0,values:1,total:totals,available_reports:1,available_total:totals,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const points=[{start:'2024-01-01',periods:1,missing:0,total:totals},{start:'2024-01-02',periods:1,missing:0,total:totals}];
 const defaults={unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-01'}],min:'2024-01-01',max:'2024-01-02',total_rows:2,next:null}),unifiedNameOptions:async()=>({rows:[{key:'work:W',name:'W',kind:'work'}],next:null,total_rows:1}),nativeAnalysisCompare:async()=>({summary:{before:summary,after:summary},top_count:[],top_amount:[]}),nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,missing:0,total:totals}],nativeAnalysisRanking:async()=>({complete:true,rows:[{key:'work:W',name:'W',kind:'work'}]}),nativeAnalysisTimeline:async()=>({granularity:'period',points}),nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points})};
 const api=new Proxy({...defaults,...overrides},{get(target,key){if(key==='load'||key==='save')throw Error('Forbidden full history');if(!(key in target))throw Error('Unexpected API '+key);return (...args)=>{calls.push({key,args});return target[key](...args);};}});
 const originalHelpers={periodEnd:(start,kind)=>{const d=new Date(start+'T00:00:00Z');if(kind==='week')d.setUTCDate(d.getUTCDate()+6);else if(kind==='month'){d.setUTCMonth(d.getUTCMonth()+1);d.setUTCDate(0);}return d.toISOString().slice(0,10);},displayPeriod:p=>p.start,renderMultiFilter:(id,items,selected,callback)=>multi.set(id,{items,selected,callback})};
 const UI={api,revision:1,snapshot:{revision:1,org_names:['A','B','C','D']},state:{selectedOrganizations:new Set(['A']),selectedMetrics:new Set(['amount','count']),chartMetrics:new Set(['amount','count']),chart:'bar'},$:id=>nodes.get(id),el:(tag,text,cls)=>{const n=new Element(tag,text);n.className=cls;return n;},fmt:n=>Number.isFinite(n)?String(n):'—',notice:(...args)=>notices.push(args),table:(root,metric,caption,headers,rows,options)=>{tables.push({root:root.id,metric,rows,options});root.append(new Element('table',rows.map(r=>r.join('|')).join('\n')));},drawChart:(...args)=>draws.push(args),drawDepartments:(...args)=>draws.push(args),setPeriodInput:(id,kind,date)=>{const node=nodes.get(id);node.dataset.periodType=kind;if(date!==undefined)node.value=date;},range:(kind,from,to,source)=>from&&from<=to?{type:source==='daily'?'day':kind,displayType:kind,source,from,to}:null,publishScope:s=>published.push(s),onSnapshot:fn=>snapshotHook=fn,onView:(view,fn)=>viewHook=fn};
 const window={unifiedUI:UI,unifiedOriginal:originalHelpers,nativePeriodSelection:{fromIsoDate:date=>date,normalize:(_kind,date)=>date},print:()=>prints++};
 vm.runInNewContext(script,{window,document:{querySelectorAll:selector=>selector==='[data-chart]'?buttons:[]},Date,Map,Set,setTimeout,clearTimeout});window.unifiedAnalytics.init();
 return {UI,window,nodes,buttons,draws,tables,published,multi,calls,notices,get prints(){return prints;},bootstrap:async()=>{await snapshotHook(UI.snapshot);await settle();},invalidate:()=>snapshotHook(null)};
}
test('dashboard uses bounded aggregates, publishes exact selected names, and prints the captured SQL summary',async()=>{
 const f=fixture();await f.bootstrap();assert.match(f.nodes.get('overviewKpis').textContent,/20/);assert.equal(f.published.at(-1).allNames,false);assert.deepEqual(Array.from(f.published.at(-1).selectedNames),['W']);assert.equal(f.UI.state.reports,undefined);
 assert.equal(f.calls.find(c=>c.key==='unifiedPeriodOptions').args[4],1);assert.ok(f.calls.filter(c=>c.key==='unifiedNameOptions').every(c=>c.args[3]<=50));
 f.nodes.get('printSummary').dispatch('click');assert.equal(f.prints,1);assert.match(f.nodes.get('printReport').textContent,/Сводка выполненных работ/);
 f.nodes.get('clearAll').dispatch('click');assert.equal(f.published.at(-1).allNames,false);assert.equal(f.published.at(-1).selectedNames.length,0);
 f.nodes.get('selectAll').dispatch('click');assert.equal(f.published.at(-1).allNames,true);assert.equal(f.published.at(-1).selectedNames.length,0);await settle();
});
test('confirmed zero filtering keeps missing points and does not change overview totals',async()=>{
 const points=[{start:'2024-01-01',periods:1,missing:1,total:{count:null,amount:null,average:null}},{start:'2024-01-02',periods:1,missing:0,total:{count:0,amount:0,average:null}}];
 const f=fixture({nativeAnalysisTimeline:async()=>({granularity:'period',points}),nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points})});await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();assert.equal(f.draws.at(-1)[4].data[0],null);assert.equal(f.draws.at(-1)[4].data[1].get('work:W').count,0);
 const overview=f.nodes.get('overviewKpis').textContent;f.nodes.get('hideEmptyPeriods').checked=true;f.nodes.get('hideEmptyPeriods').dispatch('change');await settle();assert.equal(f.draws.at(-1)[0].length,1);assert.equal(f.draws.at(-1)[4].data[0],null);assert.equal(f.nodes.get('overviewKpis').textContent,overview);
});
test('late SQL charts are discarded after scope invalidation and errors clear the old overview',async()=>{
 const wait=deferred();const f=fixture({nativeWorkTimeline:()=>wait.promise});await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();await f.invalidate();wait.resolve({granularity:'period',points:[]});await settle();assert.equal(f.draws.length,0);assert.equal(f.nodes.get('printSummary').disabled,true);
 const bad=fixture({nativeAnalysisCompare:async()=>{throw Error('database failed');}});await bad.bootstrap();assert.equal(bad.nodes.get('overviewKpis').textContent,'');assert.match(bad.nodes.get('overviewAlerts').textContent,/database failed/);assert.equal(bad.nodes.get('printSummary').disabled,true);
});
function descendants(node){return [node,...node.children.flatMap(child=>typeof child==='object'?descendants(child):[])];}
test('incomplete coverage remains visible and opens a missing-org/day matrix',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const partial={...complete,complete:false,missing:1,total:{count:null,amount:null,average:null},import_warning_reports:1};
 const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:complete,after:partial},top_count:[],top_amount:[]}),
  nativeOrganizationTrends:async()=>({series:[{organization:'A',points:[{start:'2024-01-02',missing:1}]}]})
 });await f.bootstrap();
 assert.match(f.nodes.get('overviewAlerts').textContent,/Неполные данные: 1 пропуск/);
 assert.doesNotMatch(f.nodes.get('overviewAlerts').textContent,/замечание известно/);
 const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 assert.ok(details);details.open=true;details.dispatch('toggle');await settle();
 assert.match(details.textContent,/Подразделение/);assert.match(details.textContent,/02\.01\.2024/);
 assert.match(details.textContent,/Нет/);assert.match(f.nodes.get('overviewAlerts').textContent,/Проверить данные/);
 assert.match(f.nodes.get('overviewKpis').textContent,/—/);
 f.nodes.get('printSummary').dispatch('click');assert.match(f.nodes.get('printReport').textContent,/Частичный итог/);
});
test('pending and empty sources immediately publish a null scope',async()=>{
 const wait=deferred();let reads=0;const f=fixture({unifiedPeriodOptions:()=>++reads===1?Promise.resolve({min:'2024-01-01',max:'2024-01-02',total_rows:2}):wait.promise});await f.bootstrap();assert.ok(f.published.at(-1));f.nodes.get('periodType').value='month';f.nodes.get('periodType').dispatch('change');assert.equal(f.published.at(-1),null);
 wait.resolve({min:null,max:null,total_rows:0});await settle();assert.equal(f.published.at(-1),null);assert.match(f.nodes.get('overviewAlerts').textContent,/нет отчётов/);
});
test('printed complete summaries include both captured driver tables',async()=>{
 const full={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:1};
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:full,after:full},top_count:[{name:'Count driver',kind:'work',count:{base:1,current:2,delta:1}}],top_amount:[{name:'Amount driver',kind:'group',amount:{base:10,current:20,delta:10}}]})});await f.bootstrap();f.nodes.get('printSummary').dispatch('click');const text=f.nodes.get('printReport').textContent;assert.match(text,/Count driver/);assert.match(text,/Amount driver/);assert.match(text,/исходных отчётах есть замечания/);
});
test('quantity and average labels never claim fractional volume is a count of jobs',async()=>{
 const f=fixture();await f.bootstrap();
 const kpis=f.nodes.get('overviewKpis').textContent;
 assert.match(kpis,/Количество \/ объём/);
 assert.match(kpis,/Сумма \/ количество \(объём\)/);
 assert.doesNotMatch(kpis,/Количество выполненных работ|Средняя сумма на работу/);
 assert.match(f.nodes.get('overviewInsights').textContent,/Дробное количество может означать объём/);
 f.nodes.get('printSummary').dispatch('click');
 const printed=f.nodes.get('printReport').textContent;
 assert.match(printed,/Количество \/ объём/);
 assert.match(printed,/сверять с исходными Excel/);
 assert.doesNotMatch(printed,/Средняя сумма на работу/);
});


test('charts default to columns and switch orientation without changing data grouping',async()=>{
 const f=fixture();await f.bootstrap();
 assert.equal(f.UI.state.chart,'bar');
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(f.draws.at(-1)[4].chart,'bar');assert.equal(f.draws.at(-1)[4].orientation,'vertical');
 f.nodes.get('chartOrientation').value='horizontal';f.nodes.get('chartOrientation').dispatch('change');await settle();
 assert.equal(f.draws.at(-1)[4].chart,'bar');assert.equal(f.draws.at(-1)[4].orientation,'horizontal');
});

test('missing daily reports offer existing weekly imports without inventing zeros',async()=>{
 const f=fixture({unifiedPeriodOptions:async(_revision,kind,grouping)=>{
  if(kind==='day'&&grouping==='week')return {rows:[],min:null,max:null,total_rows:0,next:null};
  if(kind==='week')return {rows:[{start:'2024-01-01',end:'2024-01-07'}],min:'2024-01-01',max:'2024-01-01',total_rows:1,next:null};
  throw Error('Unexpected period type');
 }});
 f.nodes.get('periodType').value='week';await f.bootstrap();await settle();
 assert.match(f.nodes.get('overviewAlerts').textContent,/Дневных отчётов пока нет/);
 assert.match(f.nodes.get('overviewAlerts').textContent,/Открыть недельные отчёты/);
 const btn=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='BUTTON');
 assert.ok(btn);btn.dispatch('click');await settle();
 assert.equal(f.nodes.get('unifiedSource').value,'imported');
 assert.equal(f.nodes.get('periodType').value,'week');
});

test('shared baseline changes the SQL comparison and is published to extended sections',async()=>{
 const f=fixture();await f.bootstrap();
 const comparisons=()=>f.calls.filter(item=>item.key==='nativeAnalysisCompare');
 assert.equal(f.nodes.get('analysisBasePeriod').value,'2024-01-01');
 assert.equal(comparisons().at(-1).args[2],'2024-01-01');
 f.nodes.get('analysisBasePeriod').value='2023-12-30';
 f.nodes.get('analysisBasePeriod').dispatch('change');await settle();
 assert.equal(comparisons().at(-1).args[2],'2023-12-30');
 assert.equal(f.published.at(-1).baseFrom,'2023-12-30');
 assert.equal(f.published.at(-1).baseTo,'2023-12-30');
 assert.match(f.nodes.get('overviewInsights').textContent,/базовым периодом/);
});

test('a weekly imported period shows its full calendar dates, not a repeated Monday',async()=>{
 const f=fixture({unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-07'}],min:'2024-01-01',max:'2024-01-01',total_rows:1,next:null})});
 f.nodes.get('periodType').value='week';f.nodes.get('unifiedSource').value='imported';
 await f.bootstrap();assert.match(f.nodes.get('overviewPeriodText').textContent,/01\.01\.2024 — 07\.01\.2024/);
});

test('missing-month matrix accepts YYYY-MM native period labels without rendering invalid dates',async()=>{
 const full={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const f=fixture({
  unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-31'}],min:'2024-01-01',max:'2024-01-01',total_rows:1,next:null}),
  nativeAnalysisCompare:async()=>({summary:{before:full,after:{...full,complete:false,missing:1,total:{amount:null,count:null,average:null}}}}),
  nativeOrganizationTrends:async()=>({series:[{organization:'A',points:[{start:'2024-01',missing:1}]}]})
 });
 f.nodes.get('periodType').value='month';f.nodes.get('unifiedSource').value='imported';
 await f.bootstrap();const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 assert.ok(details);details.open=true;details.dispatch('toggle');await settle();
 assert.match(details.textContent,/01\.01\.2024 — 31\.01\.2024/);
});

test('daily missing-report matrix pages a long period without a whole-history query',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const queries=[];const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:complete,after:{...complete,complete:false,missing:1,total:{count:null,amount:null,average:null}}}}),
  nativeOrganizationTrends:async(_revision,_kind,from,to)=>{
   queries.push([from,to]);return {series:[{organization:'A',points:[{start:from,missing:1}]}]};
  }
 });await f.bootstrap();
 f.nodes.get('overviewMode').value='range';
 f.nodes.get('rangeStart').value='2024-01-01';
 f.nodes.get('rangeEnd').value='2024-02-15';
 f.nodes.get('overviewMode').dispatch('change');await settle();
 const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 assert.ok(details);details.open=true;details.dispatch('toggle');await settle();
 assert.equal(queries.length,1);assert.deepEqual(queries[0],['2024-01-01','2024-01-31']);
 assert.match(details.textContent,/Страница 1 из 2/);
 const next=descendants(details).find(n=>n.tagName==='BUTTON'&&n.textContent==='Позднее →');
 assert.ok(next);next.dispatch('click');await settle();
 assert.equal(queries.length,2);assert.deepEqual(queries[1],['2024-02-01','2024-02-15']);
 assert.match(details.textContent,/Страница 2 из 2/);
 assert.match(details.textContent,/Нет/);
});

test('composition view stops hidden chart queries and discards an in-flight trend',async()=>{
 const pending=deferred();let reads=0;
 const f=fixture({nativeWorkTimeline:()=>{reads++;return pending.promise;}});await f.bootstrap();
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(reads,1);
 f.nodes.get('workViewMode').value='composition';f.nodes.get('workViewMode').dispatch('change');
 pending.resolve({granularity:'period',points:['2024-01-01','2024-01-02'].map(start=>({start,missing:0,total:{count:2,amount:20,average:10}}))});await settle();
 assert.equal(f.draws.length,0);
 f.nodes.get('chartOrientation').value='horizontal';f.nodes.get('chartOrientation').dispatch('change');await settle();
 assert.equal(reads,1,'hidden trends must not issue another SQL query');
 f.nodes.get('workViewMode').value='trend';f.nodes.get('workViewMode').dispatch('change');await settle();
 assert.equal(reads,2,'showing trends reloads their current filters');
});

test('coverage matrix queries every selected organization beyond the eight-series SQL limit',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10}};
 const queries=[],organizations=Array.from({length:10},(_,i)=>'Org '+i);
 const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:complete,after:{...complete,complete:false,missing:1}}}),
  nativeOrganizationTrends:async(_rev,_kind,from,_to,orgs)=>{
   queries.push(Array.from(orgs));
   return {series:orgs.slice(0,8).map(organization=>({organization,points:[{start:from,missing:organization==='Org 0'?1:0}]}))};
  }
 });
 f.UI.snapshot.org_names=organizations;f.UI.state.selectedOrganizations=new Set(organizations);
 await f.bootstrap();const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 details.open=true;details.dispatch('toggle');await settle();
 const cells=descendants(details).filter(n=>n.tagName==='TD');
 assert.equal(cells.length,10);assert.equal(cells.filter(n=>n.textContent==='Есть').length,9);
 assert.deepEqual(queries,[organizations.slice(0,8),organizations.slice(8)]);
});

test('overview and print round percentages to one decimal without rounding SQL values',async()=>{
 const summary=amount=>({complete:true,periods:1,missing:0,total:{amount,count:amount,average:1}});
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:summary(3),after:summary(4)}})});
 await f.bootstrap();assert.match(f.nodes.get('overviewInsights').textContent,/33,3%/);
 assert.doesNotMatch(f.nodes.get('overviewInsights').textContent,/33,333/);
 f.nodes.get('printSummary').dispatch('click');assert.match(f.nodes.get('printReport').textContent,/33,3%/);
});

test('weekly and monthly coverage pages retain exact year-crossing boundaries',async()=>{
 for(const [kind,from,to,pageEnd,nextStart,lastEnd]of [
  ['week','2024-11-04','2025-02-03','2025-01-20','2025-01-27','2025-02-09'],
  ['month','2024-02-01','2025-02-01','2025-01-01','2025-02-01','2025-02-28']
 ]){
  const queries=[],full={complete:true,periods:1,missing:0,total:{amount:1,count:1,average:1}};
  const f=fixture({
   nativeAnalysisCompare:async()=>({summary:{before:full,after:{...full,complete:false,missing:1}}}),
   nativeOrganizationTrends:async(_rev,type,start,end)=>{queries.push([type,start,end]);return {series:[{organization:'A',points:[{start,missing:1}]}]};}
  });
  f.nodes.get('periodType').value=kind;f.nodes.get('unifiedSource').value='imported';await f.bootstrap();
  f.nodes.get('overviewMode').value='range';f.nodes.get('rangeStart').value=from;f.nodes.get('rangeEnd').value=to;
  f.nodes.get('overviewMode').dispatch('change');await settle();
  const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
  details.open=true;details.dispatch('toggle');await settle();
  assert.deepEqual(queries[0],[kind,from,pageEnd]);
  descendants(details).find(n=>n.tagName==='BUTTON'&&n.textContent==='Позднее →').dispatch('click');await settle();
  assert.deepEqual(queries[1],[kind,nextStart,to]);
  assert.match(details.textContent,new RegExp(lastEnd.split('-').reverse().join('\\.')));
 }
});

test('stale coverage responses cannot populate a replaced period',async()=>{
 const pending=deferred(),full={complete:true,periods:1,missing:0,total:{amount:1,count:1,average:1}};
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:full,after:{...full,complete:false,missing:1}}}),nativeOrganizationTrends:()=>pending.promise});
 await f.bootstrap();const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 details.open=true;details.dispatch('toggle');await settle();
 f.nodes.get('overviewPeriod').value='2024-01-01';f.nodes.get('overviewPeriod').dispatch('change');await settle();
 pending.resolve({series:[{organization:'A',points:[{start:'2024-01-02',missing:1}]}]});await settle();
 assert.equal(descendants(details).filter(n=>n.tagName==='TABLE').length,0);
 assert.doesNotMatch(f.nodes.get('overviewAlerts').textContent,/02\.01\.2024/);
});

test('department charts and CSV retain organizations after the first eight series',async()=>{
 const organizations=Array.from({length:10},(_,i)=>'Org '+i),queries=[];
 const f=fixture({
  nativeOrganizationTrends:async(_rev,_kind,_from,_to,orgs)=>{
   queries.push(Array.from(orgs));return {granularity:'period',series:orgs.slice(0,8).map(organization=>({organization,points:[{start:'2024-01-02',missing:0,total:{count:2.5,amount:-10.75,average:-4.3}}]}))};
  },
  nativeAnalysisOrganizations:async()=>organizations.map(org=>({org,complete:true,total:{count:2.5,amount:-10.75,average:-4.3}}))
 });
 f.UI.snapshot.org_names=organizations;f.UI.state.selectedOrganizations=new Set(organizations);f.UI.state.chartMetrics=new Set(['amount']);
 await f.bootstrap();f.nodes.get('chartBreakdown').value='organizations';
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(f.draws.at(-1)[4].data[0].get('Org 9')?.amount,-10.75);
 f.buttons.find(button=>button.dataset.chart==='table').dispatch('click');await settle();
 const table=f.tables.findLast(t=>t.root==='dataTable');
 assert.equal(table.options.csvRows.length,10);assert.equal(table.options.csvRows[9][2],-10.75);
 assert.ok(queries.every(orgs=>orgs.length<=8));
});
