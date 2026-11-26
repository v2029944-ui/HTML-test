const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../unified-analytics.js'),'utf8');
const coverageScript=fs.readFileSync(path.join(__dirname,'../unified-coverage.js'),'utf8');
const original=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8');
const {buildUnified}=require('../unified-build.cjs');
test('generated Tauri charts use readable patterns and bars for a single department period',()=>{
 const generated=buildUnified(original),built=generated.script;
 assert.match(built,/bar=state\.chart==='bar'\|\|periods\.length===1/);
 assert.match(built,/pattern=svgEl\('pattern',\{id,width:8,height:8,patternUnits:'userSpaceOnUse'\}\)/);
 assert.match(built,/svgEl\('circle',\{cx:4,cy:4,r:2\.2,fill:color\}\)/);
 assert.ok(generated.html.includes('<option value="organizations">Подразделения</option>'));
 assert.ok(generated.html.includes('<option value="all">Все работы</option>'));
 assert.ok(generated.html.includes('<option value="selected">Отмеченные</option>'));
 assert.ok(generated.html.includes('<option value="separate">Отдельно (значения)</option>'));
 assert.ok(generated.html.includes('<option value="combined">Вместе (% макс.)</option>'));
});
class Element{
 constructor(tag='div',text=''){this.tagName=tag.toUpperCase();this.children=[];this._text=text??'';this.value='';this.dataset={};this.listeners={};this.hidden=false;this.open=false;this.checked=false;this.classList={toggle(){}};}
 append(...children){this.children.push(...children);}replaceChildren(...children){this.children=children;this._text='';}
 get textContent(){return this._text+this.children.map(c=>typeof c==='string'?c:c.textContent).join('');}set textContent(v){this._text=String(v);this.children=[];}
 addEventListener(e,fn){(this.listeners[e]??=[]).push(fn);}dispatch(e){for(const fn of this.listeners[e]||[])fn({target:this});}setAttribute(){}
}
const settle=()=>new Promise(r=>setImmediate(r));
function deferred(){let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};}
function matrixPage(revision,kind,from,to,organizations,missing=()=>true){
 const periods=[];for(let start=from;start<=to;){const d=new Date(start+'T00:00:00Z'),end=new Date(d);if(kind==='week')end.setUTCDate(end.getUTCDate()+6);if(kind==='month'){end.setUTCMonth(end.getUTCMonth()+1);end.setUTCDate(0);}periods.push({start,end:end.toISOString().slice(0,10),unassigned_invalid:0});if(kind==='month')d.setUTCMonth(d.getUTCMonth()+1);else d.setUTCDate(d.getUTCDate()+(kind==='week'?7:1));start=d.toISOString().slice(0,10);}
 return {revision,report_type:kind,periods,rows:organizations.map(organization=>({organization,cells:periods.map(({start})=>({start,available:!missing(organization),matching_reports:missing(organization)?0:1,invalid_reports:0,duplicate_reports:0,overlap_reports:0,shifted_reports:0,warned_reports:0,count:missing(organization)?null:2,amount:missing(organization)?null:20,numeric_zero:false,has_activity:!missing(organization)}))}))};
}
function fixture(overrides={}){
 const nodes=new Map();for(const m of original.matchAll(/<([a-z][\w-]*)\b[^>]*\bid="([^"]+)"[^>]*>/gi)){const n=new Element(m[1]);n.id=m[2];nodes.set(n.id,n);}
 for(const id of ['unifiedSource','hideEmptyPeriods','chartOrientation','analysisBasePeriod','workViewMode']){const node=new Element('input');node.id=id;nodes.set(id,node);}
 for(const [id,v]of Object.entries({periodType:'day',unifiedSource:'daily',overviewMode:'single',chartBreakdown:'works',chartOrgScope:'all',chartSeriesLayout:'combined',chartOrientation:'vertical'}))nodes.get(id).value=v;
 const buttons=['line','bar','table'].map(chart=>{const n=new Element('button');n.dataset.chart=chart;return n;}),draws=[],tables=[],published=[],multi=new Map(),calls=[],notices=[];
 let snapshotHook,viewHook,prints=0;
 const totals={count:2,amount:20,average:10},summary={complete:true,periods:1,organizations:1,missing:0,values:1,total:totals,available_reports:1,available_total:totals,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const points=[{start:'2024-01-01',periods:1,missing:0,total:totals},{start:'2024-01-02',periods:1,missing:0,total:totals}];
 const defaults={nativeQualityMatrixPage:async(...args)=>matrixPage(...args),unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-01'}],min:'2024-01-01',max:'2024-01-02',total_rows:2,next:null}),unifiedNameOptions:async()=>({rows:[{key:'work:W',name:'W',kind:'work'}],next:null,total_rows:1}),nativeAnalysisCompare:async()=>({summary:{before:summary,after:summary},top_count:[],top_amount:[]}),nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,missing:0,total:totals}],nativeAnalysisRanking:async()=>({complete:true,rows:[{key:'work:W',name:'W',kind:'work'}]}),nativeAnalysisTimeline:async()=>({granularity:'period',points}),nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points})};
 const methods={...defaults,...overrides};methods.unifiedNameOptionsAtPage??=(revision,term,index,limit)=>methods.unifiedNameOptions(revision,term,null,limit);
 const api=new Proxy(methods,{get(target,key){if(key==='load'||key==='save')throw Error('Forbidden full history');if(!(key in target))throw Error('Unexpected API '+key);return (...args)=>{calls.push({key,args});return target[key](...args);};}});
 const originalHelpers={displayDay:date=>date,departmentChartTitle:(n,m)=>n+' / '+m,periodEnd:(start,kind)=>{const d=new Date(start+'T00:00:00Z');if(kind==='week')d.setUTCDate(d.getUTCDate()+6);else if(kind==='month'){d.setUTCMonth(d.getUTCMonth()+1);d.setUTCDate(0);}return d.toISOString().slice(0,10);},displayPeriod:p=>p.start,renderMultiFilter:(id,items,selected,callback)=>multi.set(id,{items,selected,callback})};
 const UI={api,revision:1,snapshot:{revision:1,org_names:['A','B','C','D']},state:{selectedOrganizations:new Set(['A']),selectedMetrics:new Set(['amount','count']),chartMetrics:new Set(['amount','count']),chart:'bar'},$:id=>nodes.get(id),el:(tag,text,cls)=>{const n=new Element(tag,text);n.className=cls;return n;},fmt:n=>Number.isFinite(n)?String(n):'—',notice:(...args)=>notices.push(args),table:(root,metric,caption,headers,rows,options)=>{tables.push({root:root.id,metric,caption,rows,options});root.append(new Element('table',rows.map(r=>r.join('|')).join('\n')));},drawChart:(...args)=>draws.push(args),drawDepartments:(...args)=>draws.push(args),setPeriodInput:(id,kind,date)=>{const node=nodes.get(id);node.dataset.periodType=kind;if(date!==undefined)node.value=date;},range:(kind,from,to,source)=>from&&from<=to?{type:source==='daily'?'day':kind,displayType:kind,source,from,to}:null,publishScope:s=>published.push(s),onSnapshot:fn=>snapshotHook=fn,onView:(view,fn)=>viewHook=fn};
 const window={unifiedUI:UI,unifiedOriginal:originalHelpers,nativePeriodSelection:{fromIsoDate:date=>date,normalize:(_kind,date)=>date},print:()=>prints++};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),{window});
 vm.runInNewContext(coverageScript,{window,Date,Map,Set});
 vm.runInNewContext(script,{window,document:{querySelectorAll:selector=>selector==='[data-chart]'?buttons:[]},Date,Map,Set,setTimeout,clearTimeout});window.unifiedAnalytics.init();
 return {UI,window,nodes,buttons,draws,tables,published,multi,calls,notices,get prints(){return prints;},bootstrap:async()=>{await snapshotHook(UI.snapshot);await settle();},invalidate:()=>snapshotHook(null)};
}
test('dashboard uses bounded aggregates, publishes exact selected names, and prints the captured SQL summary',async()=>{
 const f=fixture();await f.bootstrap();assert.match(f.nodes.get('overviewKpis').textContent,/20/);assert.equal(f.published.at(-1).allNames,false);assert.deepEqual(Array.from(f.published.at(-1).selectedNames),['W']);assert.equal(f.UI.state.reports,undefined);
 assert.equal(f.calls.find(c=>c.key==='unifiedPeriodOptions').args[4],1);assert.ok(f.calls.filter(c=>c.key==='unifiedNameOptionsAtPage').every(c=>c.args[3]<=50));
 f.nodes.get('printSummary').dispatch('click');assert.equal(f.prints,1);assert.match(f.nodes.get('printReport').textContent,/Сводка выполненных работ/);
 f.nodes.get('clearAll').dispatch('click');assert.equal(f.published.at(-1).allNames,false);assert.equal(f.published.at(-1).selectedNames.length,0);
 f.nodes.get('selectAll').dispatch('click');assert.equal(f.published.at(-1).allNames,true);assert.equal(f.published.at(-1).selectedNames.length,0);await settle();
});
test('trend filtering hides absent and confirmed zero buckets and preserves incomplete or invalid reports',async()=>{
 const unknown={count:null,amount:null,average:null};
 const points=[
  {start:'2024-01-01',periods:1,missing:1,total:unknown},
  {start:'2024-01-02',periods:1,missing:0,has_activity:true,total:{count:0,amount:0,average:null}},
  {start:'2024-01-03',periods:1,missing:1,total:unknown},
  {start:'2024-01-04',periods:2,missing:1,total:unknown},
 ];
 const f=fixture({nativeAnalysisTimeline:async()=>({granularity:'period',points,buckets_with_reports:points.slice(1).map(p=>p.start)}),
  nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points})});
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(f.draws.at(-1)[4].data[0],null);assert.equal(f.draws.at(-1)[4].data[1].get('work:W').count,0);
 const overview=f.nodes.get('overviewKpis').textContent;
 f.nodes.get('hideEmptyPeriods').checked=true;f.nodes.get('hideEmptyPeriods').dispatch('change');await settle();
 assert.deepEqual(Array.from(f.draws.at(-1)[0],p=>p.start),['2024-01-03','2024-01-04']);
 assert.ok(f.draws.at(-1)[4].data.every(p=>p===null));assert.equal(f.nodes.get('overviewKpis').textContent,overview);
 assert.deepEqual(Array.from(f.draws.at(-1)[4].hiddenPeriods,p=>p.start),['2024-01-02']);
});
test('a complete zero stays hidden even if a source reports activity, while non-zero amount stays visible',async()=>{
 const points=[
  {start:'2024-01-01',periods:1,missing:0,has_activity:true,total:{count:0,amount:15,average:null}},
  {start:'2024-01-02',periods:1,missing:0,has_activity:true,total:{count:0,amount:0,average:null}},
 ];
 const f=fixture({nativeAnalysisTimeline:async()=>({granularity:'period',points}),nativeWorkTimeline:async()=>({granularity:'period',points})});
 const seen=[];f.window.referenceDetail={active:'chartPanel',setChartPeriodCount:n=>seen.push(n)};
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 f.nodes.get('hideEmptyPeriods').checked=true;f.nodes.get('hideEmptyPeriods').dispatch('change');await settle();
 assert.equal(f.draws.at(-1)[0].length,1);
 assert.equal(f.draws.at(-1)[4].data[0].get('work:W').amount,15);
 assert.equal(seen.at(-1),1);
 assert.deepEqual(Array.from(f.draws.at(-1)[4].hiddenPeriods,p=>p.start),['2024-01-02']);
});
test('work chart keeps a zero-total day with nonzero work on another page',async()=>{
 const point={start:'2024-01-01',periods:1,missing:0,has_activity:true,total:{count:0,amount:0,average:null}};
 const f=fixture({
  nativeAnalysisTimeline:async()=>({granularity:'day',points:[point],zero_buckets_with_nonzero_works:[point.start]}),
  nativeWorkTimeline:async()=>({granularity:'day',points:[point]})
 });
 await f.bootstrap();
 f.nodes.get('advancedAnalytics').open=true;
 f.nodes.get('hideEmptyPeriods').checked=true;
 f.nodes.get('advancedAnalytics').dispatch('toggle');
 await settle();
 assert.equal(f.draws.at(-1)?.[0].length,1);
});

test('hiding every reported period explains the empty chart and retains table export',async()=>{
 const points=[{start:'2024-01-01',periods:1,missing:0,has_activity:false,total:{count:0,amount:0,average:null}}];
 const timeline=async()=>({granularity:'period',points});
 const f=fixture({nativeAnalysisTimeline:timeline,nativeWorkTimeline:timeline});
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 f.nodes.get('hideEmptyPeriods').checked=true;f.nodes.get('hideEmptyPeriods').dispatch('change');await settle();
 assert.match(f.nodes.get('chartsContainer').textContent,/Фильтр скрыл все/);
 f.buttons.find(b=>b.dataset.chart==='table').dispatch('click');await settle();
 assert.match(f.nodes.get('dataTable').textContent,/Фильтр скрыл все/);
 assert.ok(f.tables.at(-1));
});
test('late SQL charts are discarded after scope invalidation and errors clear the old overview',async()=>{
 const wait=deferred();const f=fixture({nativeWorkTimeline:()=>wait.promise});await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();await f.invalidate();wait.resolve({granularity:'period',points:[]});await settle();assert.equal(f.draws.length,0);assert.equal(f.nodes.get('printSummary').disabled,true);
 const bad=fixture({nativeAnalysisCompare:async()=>{throw Error('database failed');}});await bad.bootstrap();assert.equal(bad.nodes.get('overviewKpis').textContent,'');assert.match(bad.nodes.get('overviewAlerts').textContent,/database failed/);assert.equal(bad.nodes.get('printSummary').disabled,true);
});
function descendants(node){return [node,...node.children.flatMap(child=>typeof child==='object'?descendants(child):[])];}
test('detailed work results mark only groups with a folder icon',async()=>{
 const entries=[{key:'group:repair',name:'Ремонт',kind:'group'},{key:'work:paint',name:'Покраска',kind:'work'}];
 const f=fixture({unifiedNameOptionsAtPage:async()=>({rows:entries,total_rows:entries.length})});
 await f.bootstrap();
 const labels=descendants(f.nodes.get('categories')).filter(node=>node.tagName==='SPAN');
 assert.equal(labels.find(node=>node.textContent==='Ремонт (группа)').className,'work-group-option');
 assert.notEqual(labels.find(node=>node.textContent==='Покраска').className,'work-group-option');
});
test('SQL name pagination seeks directly and resets on search/error',async()=>{
 let fail=false;const f=fixture({unifiedNameOptionsAtPage:async(_r,term,index,limit)=>{if(fail)throw Error('name read failed');return {rows:Array.from({length:50},(_,i)=>({key:'work:'+i,name:term||'Work '+i,kind:'work'})),total_rows:120,next:null};}});
 const attached=[],reset=[];f.window.sqlRowScroll={attach:(root,options)=>{attached.push({root,options});return root;},reset:root=>reset.push(root)};
 await f.bootstrap();let entry=attached.findLast(x=>x.root===f.nodes.get('categories'));assert.ok(entry);assert.equal(entry.options.size,50);assert.equal(entry.options.direct,true);assert.equal(entry.options.total,120);
 assert.ok(descendants(entry.root).filter(n=>n.tagName==='BUTTON').every(n=>n.hidden));
 const before=f.calls.length,resets=reset.length;await entry.options.go(3);assert.equal(f.calls.length,before);await entry.options.go(2);assert.equal(f.calls.at(-1).args[2],2);entry=attached.findLast(x=>x.root===entry.root);
 await entry.options.go(1);entry=attached.findLast(x=>x.root===entry.root);assert.equal(entry.options.index,1);assert.equal(f.calls.at(-1).args[2],1);
 await entry.options.go(0);assert.equal(f.calls.at(-1).args[2],0);assert.equal(reset.length,resets,'paging preserves pending position/stride');assert.doesNotMatch(entry.root.textContent,/Страница/);
 fail=true;entry=attached.findLast(x=>x.root===entry.root);await entry.options.go(1);assert.match(entry.root.textContent,/name read failed/);assert.ok(reset.includes(entry.root));
 const stale=entry.options.go;f.nodes.get('categorySearch').value='New';f.nodes.get('categorySearch').dispatch('input');const reads=f.calls.length;await stale(1);assert.equal(f.calls.length,reads);
 await new Promise(resolve=>setTimeout(resolve,180));
});
test('bounded selected chart rows can seek by offset without fetching intermediate SQL windows',async()=>{
 const entries=Array.from({length:24},(_,i)=>({key:'work:W'+i,name:'W'+i,kind:'work'}));
 const f=fixture({unifiedNameOptions:async()=>({rows:entries,total_rows:24,next:null})}),attached=[];
 f.window.sqlRowScroll={attach:(root,options)=>{attached.push({root,options});return root;},reset(){}};
 await f.bootstrap();await f.window.unifiedAnalytics.applyReferenceScope({...f.published.at(-1),metric:'amount',overviewMode:'single',allNames:false,nameRows:entries});
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 const entry=attached.findLast(x=>x.root.id==='chartsContainer');assert.ok(entry);assert.equal(entry.options.direct,true);assert.equal(entry.options.size,8);assert.equal(entry.options.total,24);
 const before=f.calls.length;await entry.options.go(2);assert.deepEqual(f.calls.slice(before).filter(c=>c.key==='nativeWorkTimeline').map(c=>c.args[6]),entries.slice(16).map(r=>r.key));assert.equal(attached.at(-1).options.index,2);
});
test('incomplete coverage remains visible and opens a missing-org/day matrix',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const partial={...complete,complete:false,missing:1,total:{count:null,amount:null,average:null},import_warning_reports:1};
 const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:complete,after:partial},top_count:[],top_amount:[]}),
  nativeQualityMatrixPage:async(...args)=>matrixPage(...args)
 });await f.bootstrap();
 assert.match(f.nodes.get('overviewAlerts').textContent,/Неполные данные: 1 пропуск/);
 assert.doesNotMatch(f.nodes.get('overviewAlerts').textContent,/замечание известно/);
 const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 assert.ok(details);details.open=true;details.dispatch('toggle');await settle();
 assert.match(details.textContent,/Подразделение/);assert.match(details.textContent,/02\.01\.2024/);
 assert.match(details.textContent,/Нет/);assert.match(f.nodes.get('overviewAlerts').textContent,/Проверить данные/);
 assert.match(f.nodes.get('overviewKpis').textContent,/20Без изменений/);
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

test('missing-month quality matrix shows the full native calendar period',async()=>{
 const full={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const f=fixture({
  unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-31'}],min:'2024-01-01',max:'2024-01-01',total_rows:1,next:null}),
  nativeAnalysisCompare:async()=>({summary:{before:full,after:{...full,complete:false,missing:1,total:{amount:null,count:null,average:null}}}}),
  nativeQualityMatrixPage:async(...args)=>matrixPage(...args)
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
  nativeQualityMatrixPage:async(revision,kind,from,to,orgs)=>{
   queries.push([from,to]);return matrixPage(revision,kind,from,to,orgs);
  }
 });await f.bootstrap();
 f.nodes.get('overviewMode').value='range';
 f.nodes.get('rangeStart').value='2024-01-01';
 f.nodes.get('rangeEnd').value='2024-02-15';
 f.nodes.get('overviewMode').dispatch('change');await settle();
 const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 assert.ok(details);details.open=true;details.dispatch('toggle');await settle();
 assert.equal(queries.length,1);assert.deepEqual(queries[0],['2024-01-01','2024-01-31']);
 assert.match(details.textContent,/01\.01\.2024 — 31\.01\.2024/);
 const next=descendants(details).find(n=>n.tagName==='BUTTON'&&n.textContent==='Позднее →');
 assert.ok(next);next.dispatch('click');await settle();
 assert.equal(queries.length,2);assert.deepEqual(queries[1],['2024-02-01','2024-02-15']);
 assert.match(details.textContent,/01\.02\.2024 — 15\.02\.2024/);
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
  nativeQualityMatrixPage:async(revision,kind,from,to,orgs)=>{
   queries.push(Array.from(orgs));
   return matrixPage(revision,kind,from,to,orgs,organization=>organization==='Org 0');
  }
 });
 f.UI.snapshot.org_names=organizations;f.UI.state.selectedOrganizations=new Set(organizations);
 await f.bootstrap();const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 details.open=true;details.dispatch('toggle');await settle();
 const cells=descendants(details).filter(n=>n.tagName==='TD');
 assert.equal(cells.length,10);assert.equal(cells.filter(n=>n.textContent==='Есть данные').length,9);
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
   nativeQualityMatrixPage:async(revision,type,start,end,orgs)=>{queries.push([type,start,end]);return matrixPage(revision,type,start,end,orgs);}
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
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:full,after:{...full,complete:false,missing:1}}}),nativeQualityMatrixPage:()=>pending.promise});
 await f.bootstrap();const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');
 details.open=true;details.dispatch('toggle');await settle();
 f.nodes.get('overviewPeriod').value='2024-01-01';f.nodes.get('overviewPeriod').dispatch('change');await settle();
 pending.resolve(matrixPage(1,'day','2024-01-02','2024-01-02',['A']));await settle();
 assert.equal(descendants(details).filter(n=>n.tagName==='TABLE').length,0);
 assert.doesNotMatch(f.nodes.get('overviewAlerts').textContent,/02\.01\.2024/);
});

test('department charts and CSV retain organizations after the first eight series',async()=>{
 const organizations=Array.from({length:10},(_,i)=>'Org '+i),queries=[];
 const f=fixture({
  nativeOrganizationTrends:async(_rev,_kind,_from,_to,orgs)=>{
   queries.push(Array.from(orgs));
   const present='2024-01-01',absent='2024-01-02',unknown={count:null,amount:null,average:null};
   return {granularity:'period',buckets_with_reports:orgs.includes('Org 9')?[present]:[],
    series:orgs.map(organization=>({organization,points:[
     organization==='Org 9'?{start:present,missing:0,total:{count:2.5,amount:-10.75,average:-4.3}}:{start:present,missing:1,total:unknown},
     {start:absent,missing:1,total:unknown}
    ]}))};
  },
  nativeAnalysisOrganizations:async()=>organizations.map(org=>({org,complete:true,total:{count:2.5,amount:-10.75,average:-4.3}}))
 });
 f.UI.snapshot.org_names=organizations;f.UI.state.selectedOrganizations=new Set(organizations);f.UI.state.chartMetrics=new Set(['amount']);
 await f.bootstrap();f.nodes.get('chartBreakdown').value='organizations';
 f.nodes.get('hideEmptyPeriods').checked=true;
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.deepEqual(Array.from(f.draws.at(-1)[0],p=>p.start),['2024-01-01']);
 assert.equal(f.draws.at(-1)[4].data[0].get('Org 9')?.amount,-10.75);
 f.buttons.find(button=>button.dataset.chart==='table').dispatch('click');await settle();
 const table=f.tables.findLast(t=>t.root==='dataTable');
 assert.equal(table.options.csvRows.length,10);assert.deepEqual(Array.from(table.options.csvRows[9]),['Org 9','Сумма',-10.75]);
 assert.ok(queries.every(orgs=>orgs.length<=8));
});
test('horizontal combined department charts use the same scale as vertical bars while separate charts show original units',async()=>{
 const f=fixture({nativeOrganizationTrends:async()=>({granularity:'period',series:[{organization:'A',points:[{start:'2024-01-02',missing:0,total:{amount:20,count:2,average:10}}]}]})});
 await f.bootstrap();f.nodes.get('chartBreakdown').value='organizations';
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.match(f.nodes.get('chartReadGuide').textContent,/% от максимума/);
 f.nodes.get('chartOrientation').value='horizontal';f.nodes.get('chartOrientation').dispatch('change');await settle();
 assert.match(f.nodes.get('chartReadGuide').textContent,/% от максимума/);
 f.nodes.get('chartSeriesLayout').value='separate';f.nodes.get('chartSeriesLayout').dispatch('change');await settle();
 assert.match(f.nodes.get('chartReadGuide').textContent,/в исходных единицах/);
 assert.doesNotMatch(f.nodes.get('chartReadGuide').textContent,/% от максимума/);
});

test('fully available ranges expose lazy quality evidence without hiding source warnings',async()=>{
 const f=fixture({nativeQualityMatrixPage:async(...args)=>matrixPage(...args,()=>false)});await f.bootstrap();assert.equal(f.calls.filter(c=>c.key==='nativeQualityMatrixPage').length,0);
 const details=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='DETAILS');assert.ok(details);assert.match(details.textContent,/Качество данных/);
 details.open=true;details.dispatch('toggle');await settle();assert.equal(f.calls.filter(c=>c.key==='nativeQualityMatrixPage').length,1);assert.match(details.textContent,/Есть данные/);
});
test('the reference filter passes both exact intervals and selected work keys through the native detail scope',async()=>{
 const f=fixture();await f.bootstrap();
 const shared={revision:1,displayType:'month',source:'daily',type:'day',overviewMode:'single',from:'2024-02-01',to:'2024-02-29',
  baseFrom:'2023-02-01',baseTo:'2023-02-28',organizations:['B'],metric:'count',allNames:false,nameRows:[{key:'work:W',name:'W',kind:'work'}]};
 await f.window.unifiedAnalytics.applyReferenceScope(shared);
 const query=f.calls.filter(c=>c.key==='nativeAnalysisCompare').at(-1);
 assert.deepEqual(query.args.slice(2,6),['2023-02-01','2023-02-28','2024-02-01','2024-02-29']);
 const scope=f.published.at(-1);assert.deepEqual(Array.from(scope.organizations),['B']);
 assert.deepEqual(Array.from(scope.selectedKeys),['work:W']);assert.deepEqual(Array.from(f.UI.state.selectedMetrics),['count']);
 assert.equal(scope.baseTo,'2023-02-28');assert.equal(scope.to,'2024-02-29');
 f.window.unifiedAnalytics.applyReferenceScope({...shared,revision:0});
 assert.equal(f.published.at(-1),null);
});
test('the reference weekly chart requests daily observations without changing the shared weekly selection',async()=>{
 const f=fixture();await f.bootstrap();
 f.window.referenceDetail={active:'chartPanel'};
 f.nodes.get('advancedAnalytics').open=true;
 const shared={revision:1,displayType:'week',source:'daily',type:'day',overviewMode:'single',from:'2024-01-01',to:'2024-01-07',
  baseFrom:'2023-12-25',baseTo:'2023-12-31',organizations:['A'],metric:'count',allNames:false,nameRows:[{key:'work:W',name:'W',kind:'work'}]};
 await f.window.unifiedAnalytics.applyReferenceScope(shared);
 const query=f.calls.filter(c=>c.key==='nativeAnalysisTimeline').at(-1);
 assert.equal(query.args[6],'day');
 assert.equal(f.draws.at(-1)[0][0].type,'day');
 assert.equal(f.published.at(-1).displayType,'week');
 f.window.referenceDetail.active='regularityPanel';
 const before=f.calls.filter(c=>c.key==='nativeAnalysisTimeline').length;
 f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(f.calls.filter(c=>c.key==='nativeAnalysisTimeline').length,before);
});

test('a daily month keeps observed days and gaps identical in charts and tables',async()=>{
 const points=Array.from({length:31},(_,i)=>({
  start:'2024-01-'+String(i+1).padStart(2,'0'),periods:1,missing:i===14?1:0,
  total:i===14?{count:null,amount:null,average:null}:i===15?{count:0,amount:0,average:null}:{count:2,amount:20,average:10}
 }));
 const timeline=async()=>({granularity:'day',points});
 const f=fixture({nativeAnalysisTimeline:timeline,nativeWorkTimeline:timeline});
 f.UI.request=s=>({revision:s.revision,source:s.source,displayType:s.displayType,
  organizations:[...s.organizations],selectedNames:[...s.selectedNames],query:{from:s.from,to:s.to}});
 await f.bootstrap();
 f.window.referenceDetail={active:'chartPanel'};
 f.nodes.get('advancedAnalytics').open=true;
 await f.window.unifiedAnalytics.applyReferenceScope({
  revision:1,displayType:'month',source:'daily',type:'day',overviewMode:'single',
  from:'2024-01-01',to:'2024-01-31',baseFrom:'2023-12-01',baseTo:'2023-12-31',
  organizations:['A'],metric:'count',allNames:false,nameRows:[{key:'work:W',name:'W',kind:'work'}]
 });
 const request=f.calls.filter(call=>call.key==='nativeAnalysisTimeline').at(-1);
 assert.deepEqual(request.args.slice(1,4),['day','2024-01-01','2024-01-31']);
 assert.equal(request.args[6],'day');
 for(const style of ['line','bar']){
  f.buttons.find(button=>button.dataset.chart===style).dispatch('click');
  await settle();
  const [periods,,,,options]=f.draws.at(-1);
  assert.equal(options.chart,style);
  assert.equal(periods.length,31);
  assert.equal(options.data[13].get('work:W').count,2);
  assert.equal(options.data[14],null);
  assert.match(options.pointNotice(14),/пропусков 1/);
  assert.equal(options.data[15].get('work:W').count,0);
  assert.equal(options.data[16].get('work:W').count,2);
 }
 f.buttons.find(button=>button.dataset.chart==='table').dispatch('click');
 await settle();
 const table=f.tables.findLast(t=>t.root==='dataTable'&&t.metric==='count');
 for(const row of table.options.csvRows){
  assert.deepEqual(Array.from(row.slice(14,18)),[2,null,0,2]);
 }
 assert.deepEqual(Array.from(table.rows[1].slice(14,18)),['2','—','0','2']);
});

test('reference trend CSV retains selected names when a confirmed zero with activity is hidden',async()=>{
 const saved=[],points=[{start:'2024-01-01',periods:1,missing:0,has_activity:true,total:{count:0,amount:0,average:null}}];
 const f=fixture({
  nativeAnalysisTimeline:async()=>({granularity:'period',points}),
  nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points:points.map(p=>({...p,has_activity:false}))}),
  chooseCsvPath:async()=>'/tmp/view.csv',referenceTrendCsv:async(...args)=>saved.push(args)
 });
 f.window.referenceDetail={active:'chartPanel'};
 f.UI.request=s=>({revision:s.revision,organizations:[...s.organizations],selectedNames:s.allNames?[]:[...s.selectedNames],query:{from:s.from,to:s.to}});
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;
 f.nodes.get('selectAll').dispatch('click');f.nodes.get('hideEmptyPeriods').checked=true;
 f.buttons.find(button=>button.dataset.chart==='table').dispatch('click');await settle();
 const table=f.tables.findLast(t=>t.root==='dataTable'&&t.metric==='amount');
 // The activity flag is not a nonzero quantity/amount. The day is hidden;
 // its absence must not suppress CSV export or the full name selection.
 assert.ok(table);assert.equal(table.rows[0].length,1);
 await table.options.csvContext.export();
 assert.deepEqual([...saved[0][0].selectedNames],[]);assert.deepEqual([...saved[0][1].metrics],['amount']);
 assert.equal(saved[0][1].table,'works');assert.equal(saved[0][1].hideEmpty,true);assert.equal(saved[0][2],'/tmp/view.csv');
 assert.equal(saved[0][1].matchChart,true);
 f.window.referenceDetail.active='regularityPanel';
 await assert.rejects(table.options.csvContext.export(),/изменились/);assert.equal(saved.length,1);
});

test('reference details receive exact selected dates while overview SQL is pending and discard them on failure',async()=>{
 const overview=deferred(),organizations=deferred();
 const f=fixture({nativeAnalysisCompare:()=>overview.promise,nativeAnalysisOrganizations:()=>organizations.promise});
 const shared={revision:1,displayType:'week',source:'daily',type:'day',overviewMode:'single',from:'2026-11-16',to:'2026-11-22',
  baseFrom:'2026-11-09',baseTo:'2026-11-15',organizations:['A'],metric:'amount',allNames:false,nameRows:[{key:'work:W',name:'W',kind:'work'}]};
 const pending=f.window.unifiedAnalytics.applyReferenceScope(shared);
 const immediate=f.published.at(-1);
 assert.ok(immediate);
 assert.deepEqual([immediate.from,immediate.to,immediate.baseFrom,immediate.baseTo],[shared.from,shared.to,shared.baseFrom,shared.baseTo]);
 assert.deepEqual([...immediate.selectedKeys],['work:W']);
 assert.deepEqual([...immediate.organizations],['A']);
 const count=f.published.length;
 organizations.resolve([]);overview.reject(Error('overview unavailable'));
 await pending;
 assert.equal(f.published.length,count+1);
 assert.equal(f.published.at(-1),null);
 assert.match(f.nodes.get('overviewAlerts').textContent,/overview unavailable/);
});
test('successful reference overview publishes its validated detail scope once',async()=>{
 const f=fixture(),shared={revision:1,displayType:'day',source:'daily',type:'day',overviewMode:'single',
  from:'2024-01-02',to:'2024-01-02',baseFrom:'2024-01-01',baseTo:'2024-01-01',
  organizations:['A'],metric:'amount',allNames:true,nameRows:[]};
 const pending=f.window.unifiedAnalytics.applyReferenceScope(shared);
 const initial=f.published.at(-1);
 assert.equal(initial?.allNames,true);
 await pending;
 assert.deepEqual(f.published.filter(Boolean),[initial]);
 assert.equal(f.calls.filter(call=>call.key==='nativeAnalysisCompare').length,1);
});

test('a completed chart survives three overlapping mode switches and only the newest SQL response can replace it',async()=>{
 let hold=false;const gates=[],points=[{start:'2024-01-02',periods:1,missing:0,total:{amount:40,count:2,average:20}}];
 const f=fixture({nativeWorkTimeline:async()=>{if(hold){const gate=deferred();gates.push(gate);await gate.promise;}return {granularity:'period',points};},nativeAnalysisTimeline:async()=>({granularity:'period',points})});
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 const plot=f.nodes.get('chartsContainer'),table=f.nodes.get('dataTable'),previous=plot.children[0];assert.ok(previous);
 hold=true;
 for(const mode of ['table','line','table']){f.buttons.find(b=>b.dataset.chart===mode).dispatch('click');await settle();assert.equal(plot.children[0],previous);assert.equal(plot.hidden,false);assert.equal(plot.inert,true);}
 assert.equal(gates.length,3);
 gates[2].resolve();await settle();const newest=table.children[0];assert.ok(newest);assert.equal(table.hidden,false);
 gates[1].resolve();gates[0].reject(Error('old read failed'));await settle();assert.equal(table.children[0],newest);assert.equal(table.inert,false);
 f.buttons[0].dispatch('click');await settle();gates.at(-1).reject(Error('database unavailable'));await settle();assert.match(plot.textContent,/database unavailable/);assert.equal(plot.inert,false);
});

test('chart contents and caption keep the completed period throughout a delayed filter change',async()=>{
 const overview=deferred(),timeline=deferred();
 const total={count:2,amount:4,average:2},summary={complete:true,periods:1,total};
 const result={summary:{before:summary,after:summary}};
 const points={granularity:'period',points:[]};
 let updating=false,refreshDate=null;
 const f=fixture({nativeAnalysisCompare:()=>updating?overview.promise:Promise.resolve(result),
  nativeWorkTimeline:()=>updating?timeline.promise:Promise.resolve(points),nativeAnalysisTimeline:async()=>points});
 f.window.referenceDetail={active:'chartPanel',refreshChartCaption(){refreshDate=f.window.unifiedAnalytics.displayedChartScope?.from;}};
 const filters={revision:1,type:'day',displayType:'day',source:'daily',overviewMode:'single',
  from:'2024-01-01',to:'2024-01-01',baseFrom:'2023-12-31',baseTo:'2023-12-31',organizations:['A'],metric:'amount',allNames:true,nameRows:[]};
 const plot=f.nodes.get('chartsContainer');
 f.nodes.get('advancedAnalytics').open=true;
 await f.window.unifiedAnalytics.applyReferenceScope(filters);
 const old=plot.children[0];
 assert.equal(refreshDate,filters.from);
 updating=true;
 const next=f.window.unifiedAnalytics.applyReferenceScope({...filters,from:'2024-02-01',to:'2024-02-01'});
 assert.equal(f.published.at(-1).from,'2024-02-01');
 assert.equal(plot.inert,true);
 assert.equal(f.window.unifiedAnalytics.displayedChartScope.from,filters.from);
 overview.resolve(result);await settle();
 assert.equal(plot.children[0],old);assert.equal(plot.inert,true);assert.equal(refreshDate,filters.from);
 timeline.resolve(points);await next;
 assert.equal(refreshDate,'2024-02-01');assert.notEqual(plot.children[0],old);assert.equal(plot.inert,false);
});

test('a workspace invalidation clears retained chart dates, controls and results even with SQL in flight',async()=>{
 const pending=deferred();let late=false;
 const result={granularity:'period',points:[]};
 const f=fixture({nativeWorkTimeline:()=>late?pending.promise:Promise.resolve(result),nativeAnalysisTimeline:async()=>result});
 await f.bootstrap();const plot=f.nodes.get('chartsContainer');
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.ok(f.window.unifiedAnalytics.displayedChartScope);
 late=true;f.buttons[0].dispatch('click');assert.equal(plot.inert,true);
 await f.invalidate();pending.resolve(result);await settle();
 assert.equal(f.window.unifiedAnalytics.displayedChartScope,null);assert.equal(plot.children.length,0);assert.equal(plot.inert,false);
});

test('only the displayed work chart or table owns a pager after switching modes',async()=>{
 const f=fixture(),activePagers=new Map();
 f.window.sqlRowScroll={
  attach(root,options){activePagers.set(root.id,options);return root;},
  reset(root){activePagers.delete(root.id);}
 };
 await f.bootstrap();
 const rows=Array.from({length:17},(_,i)=>({key:'work:'+i,name:'Work '+i,kind:'work'}));
 f.nodes.get('advancedAnalytics').open=true;
 await f.window.unifiedAnalytics.applyReferenceScope({...f.published.at(-1),metric:'amount',nameRows:rows});
 const visible=()=>[...activePagers.keys()].filter(id=>id==='chartsContainer'||id==='dataTable');
 assert.deepEqual(visible(),['chartsContainer']);
 f.buttons.find(b=>b.dataset.chart==='table').dispatch('click');await settle();
 assert.deepEqual(visible(),['dataTable']);
 assert.equal(activePagers.get('dataTable').count,17);
 f.buttons.find(b=>b.dataset.chart==='line').dispatch('click');await settle();
 assert.deepEqual(visible(),['chartsContainer']);
 await activePagers.get('chartsContainer').go(1);
 assert.equal(activePagers.get('chartsContainer').index,1);
});

test('department trends with all performed works allow an empty individual selection',async()=>{
 const f=fixture({nativeOrganizationTrends:async()=>({granularity:'period',series:[]})});
 await f.bootstrap();
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('chartBreakdown').value='organizations';
 await f.window.unifiedAnalytics.applyReferenceScope({...f.published.at(-1),metric:'amount',allNames:false,nameRows:[]});
 const trendReads=()=>f.calls.filter(c=>c.key==='nativeOrganizationTrends');
 assert.equal(trendReads().length,1);
 assert.deepEqual([...trendReads()[0].args[7]],[]);
 assert.match(f.nodes.get('chartHelp').textContent,/все выполненные работы/);
 f.nodes.get('chartOrgScope').value='selected';f.nodes.get('chartOrgScope').dispatch('change');await settle();
 assert.equal(trendReads().length,1);
 assert.match(f.nodes.get('chartsContainer').textContent,/Нет отмеченных работ/);
 f.nodes.get('chartBreakdown').value='works';f.nodes.get('chartBreakdown').dispatch('change');await settle();
 assert.match(f.nodes.get('chartsContainer').textContent,/хотя бы одну работу/);
});

test('overview scope, period label and KPIs follow the same completed SQL response through overlap and failure',async()=>{
 const pending=[],f=fixture({nativeAnalysisCompare:()=>{const gate=deferred();pending.push(gate);return gate.promise;}});
 const filters={revision:1,type:'day',displayType:'day',source:'daily',overviewMode:'range',organizations:['A'],metric:'amount',allNames:true,nameRows:[]};
 const apply=month=>f.window.unifiedAnalytics.applyReferenceScope({...filters,
  from:'2024-'+month+'-10',to:'2024-'+month+'-11',baseFrom:'2024-'+month+'-08',baseTo:'2024-'+month+'-09'});
 const result=amount=>{const period={complete:true,periods:1,missing:0,total:{count:1,amount,average:amount}};return {summary:{before:period,after:period}};};
 const presented=()=>f.window.unifiedAnalytics.displayedOverviewScope;
 const kpis=f.nodes.get('overviewKpis'),period=f.nodes.get('overviewPeriodText');
 const jan=apply('01');pending[0].resolve(result(111));await jan;
 const feb=apply('02'),mar=apply('03');
 assert.equal(presented().from,'2024-01-10');assert.equal(presented().baseFrom,'2024-01-08');
 assert.match(period.textContent,/10\.01\.2024/);assert.match(kpis.textContent,/111/);assert.equal(kpis.inert,true);
 pending[2].resolve(result(888));await mar;
 pending[1].resolve(result(222));await feb;
 assert.equal(presented().from,'2024-03-10');assert.match(period.textContent,/10\.03\.2024/);assert.match(kpis.textContent,/888/);
 const apr=apply('04');pending[3].reject(Error('current error'));await apr;
 assert.equal(presented(),null);assert.equal(period.textContent,'');assert.equal(kpis.textContent,'');
 assert.match(f.nodes.get('overviewAlerts').textContent,/current error/);
});

test('all three imported trend table captions use the full calendar interval',async()=>{
 const totals={amount:2,count:1,average:2},point=start=>({start,total:totals,missing:0});
 for(const [kind,first,last] of [['week','2026-11-11','2026-11-17'],['month','2026-11-01','2026-11-30']]){
  const f=fixture({nativeOrganizationTrends:async()=>({granularity:'period',series:[{organization:'A',points:[point(first)]}]})});
  await f.bootstrap();f.window.referenceDetail={active:'chartPanel'};f.UI.request=s=>s;
  const shared={...f.published.at(-1),source:'imported',displayType:kind,type:kind,from:first,to:first,baseFrom:first,baseTo:first,metric:'amount',overviewMode:'single'};
  f.nodes.get('advancedAnalytics').open=true;
  await f.window.unifiedAnalytics.applyReferenceScope(shared);
  f.buttons.find(b=>b.dataset.chart==='table').dispatch('click');await settle();
  const works=f.tables.findLast(t=>t.root==='dataTable');
  f.nodes.get('chartBreakdown').value='organizations';f.nodes.get('chartBreakdown').dispatch('change');await settle();
  for(const table of [works,f.tables.findLast(t=>t.root==='dataTable'),f.tables.findLast(t=>t.root==='chartOrgTotals')]){
   assert.ok(table.caption.includes(last),table.caption);assert.ok(table.caption.includes(first),table.caption);
  }
 }
});

test('all-work dynamics requests identities for both namesakes and itemizes the full amount',async()=>{
 const series=[
  {key:'group:g',name:'Ремонт (группа)',kind:'group',amount:100},
  {key:'work:Ремонт',name:'Ремонт (работа)',kind:'work',amount:40},
  {key:'work:Уборка',name:'Уборка',kind:'work',amount:10},
 ];
 const seriesFor=amount=>({granularity:'period',points:[{start:'2024-01-02',missing:0,has_activity:true,total:{amount,count:1,average:amount}}]});
 const f=fixture({
  unifiedNameOptionsAtPage:async(_revision,_query,_index,_limit,allSeries)=>{
   const rows=allSeries?series:series.slice(2);
   return {rows,next:null,total_rows:rows.length};
  },
  nativeAnalysisTimeline:async()=>seriesFor(150),
  nativeWorkTimeline:async(_r,_t,_a,_b,_orgs,_org,key)=>seriesFor(series.find(row=>row.key===key)?.amount??0),
 });
 await f.bootstrap();
 await f.window.unifiedAnalytics.applyReferenceScope({...f.published.at(-1),allNames:true,nameRows:[],metric:'amount',overviewMode:'single'});
 f.buttons.find(button=>button.dataset.chart==='table').dispatch('click');
 await settle();
 const table=f.tables.findLast(entry=>entry.root==='dataTable'&&entry.metric==='amount');
 assert.deepEqual(Array.from(table.rows,row=>Array.from(row)),[
  ['Итого по выбранным работам','150'],['Ремонт (группа)','100'],['Ремонт (работа)','40'],['Уборка','10'],
 ]);
});
test('detailed overview distinguishes namesakes and neutralizes sub-precision changes on screen and in print',async()=>{
 const period=amount=>({complete:true,periods:1,missing:0,total:{amount,count:1,average:amount}});
 const top_amount=[{kind:'work',name:'Ремонт',amount:{delta:.002}},{kind:'group',name:'Ремонт',amount:{delta:-.002}}];
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:period(100),after:period(100.002)},top_amount,top_count:[]})});
 f.UI.fmt=(n,m)=>Number.isFinite(n)?new Intl.NumberFormat('ru-RU',{minimumFractionDigits:m==='count'?0:2,maximumFractionDigits:m==='count'?3:2}).format(n):'—';
 await f.bootstrap();f.nodes.get('printSummary').dispatch('click');
 for(const id of ['overviewInsights','printReport']){
  const text=f.nodes.get(id).textContent;assert.doesNotMatch(text,/[-+−]0,00/);
  assert.match(text,/Ремонт \(группа\)/);assert.match(text,/≈ без изменений/);
 }
 for(const card of f.nodes.get('overviewKpis').children.slice(0,3))assert.ok(!card.children.at(-1).className);
});
test('top-eight selection publishes only names accepted by the work filter',async()=>{
 const invalid={key:'group:g',name:'Ремонт',kind:'group'},valid={key:'work:Шины',name:'Шины',kind:'work'};
 const f=fixture({nativeAnalysisRanking:async(_r,_t,_from,_to,_orgs,_org,_metric,category)=>({complete:true,rows:category==='selectable'?[valid]:[invalid,valid]})});
 await f.bootstrap();f.nodes.get('selectTop').dispatch('click');await settle();
 assert.deepEqual(Array.from(f.published.at(-1).selectedKeys),[valid.key]);
});

test('clearing the work selection supersedes an in-flight top-eight ranking',async()=>{
 const gate=deferred();let reads=0;
 const rows=[{name:'Ремонт',key:'work:Ремонт',kind:'work'}];
 const fx=fixture({nativeAnalysisRanking:()=>++reads===1?Promise.resolve({complete:true,rows}):gate.promise});
 await fx.bootstrap();
 fx.nodes.get('selectTop').dispatch('click');
 fx.nodes.get('clearAll').dispatch('click');
 gate.resolve({complete:true,rows});await settle();
 assert.deepEqual(Array.from(fx.published.at(-1).selectedKeys),[]);
});

test('top-eight selection uses ranked work from partial reports',async()=>{
 const work={key:'work:Шины',name:'Шины',kind:'work'};
 const f=fixture({nativeAnalysisRanking:async()=>({complete:false,rows:[work]})});
 await f.bootstrap();
 assert.deepEqual(Array.from(f.published.at(-1).selectedKeys),[work.key]);
 f.nodes.get('selectTop').dispatch('click');await settle();
 assert.deepEqual(Array.from(f.published.at(-1).selectedKeys),[work.key]);
 assert.ok(f.notices.some(([message])=>message.includes('по доступным отчётам')));
});

test('the local all-works control stays accurate when same-name series cannot be selected individually',async()=>{
 const fx=fixture({unifiedNameOptionsAtPage:async()=>({rows:[],total_rows:0})});
 await fx.bootstrap();fx.nodes.get('selectAll').dispatch('click');await settle();
 assert.equal(fx.nodes.get('categoryCount').textContent,'Все работы и группы');
});

test('work choices made in overview update the shared scope used by other detail tabs',async()=>{
 const fx=fixture(),sent=[];
 await fx.bootstrap();
 fx.window.referenceDetail={active:'overviewPanel'};
 fx.window.referenceFilters={setWorks:(all,rows)=>sent.push([all,[...rows.keys()]])};
 fx.nodes.get('clearAll').dispatch('click');await settle();
 fx.nodes.get('selectTop').dispatch('click');await settle();
 fx.nodes.get('selectAll').dispatch('click');await settle();
 assert.deepEqual(sent,[[false,[]],[false,['work:W']],[true,[]]]);
});

test('overview keeps its extra metrics across a new interval until the shared metric itself changes',async()=>{
 const f=fixture();await f.bootstrap();f.window.referenceDetail={active:'overviewPanel'};
 const scope={...f.published.at(-1),metric:'amount',overviewMode:'range',baseFrom:'2023-12-30',baseTo:'2023-12-31'};
 await f.window.unifiedAnalytics.applyReferenceScope(scope);
 f.UI.state.selectedMetrics.add('count');
 await f.window.unifiedAnalytics.applyReferenceScope({...scope,from:'2024-01-03',to:'2024-01-04'});
 assert.deepEqual([...f.UI.state.selectedMetrics],['amount','count']);
 await f.window.unifiedAnalytics.applyReferenceScope({...scope,metric:'count'});
 assert.deepEqual([...f.UI.state.selectedMetrics],['count']);
});

test('plots and tabulates available work amounts while leaving absent dates blank',async()=>{
 const unknown={count:null,amount:null,average:null};
 const points=[
  {start:'2024-01-01',periods:1,missing:0,total:{count:5,amount:null,average:null},available_total:{count:5,amount:20,average:null}},
  {start:'2024-01-02',periods:1,missing:1,total:unknown,available_total:{count:4,amount:30,average:7.5}},
  {start:'2024-01-03',periods:1,missing:2,total:unknown,available_total:unknown}
 ];
 const result={granularity:'period',points,buckets_with_reports:points.slice(0,2).map(p=>p.start)};
 const f=fixture({nativeAnalysisTimeline:async()=>result,nativeWorkTimeline:async()=>result});
 await f.bootstrap();f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');
 f.buttons.find(b=>b.dataset.chart==='line').dispatch('click');await settle();
 const opts=f.draws.findLast(d=>d[2]==='amount')[4];
 assert.deepEqual(Array.from(opts.data,point=>point?.get('work:W').amount??null),[20,30,null]);
 assert.match(f.nodes.get('chartHelp').textContent,/частичн/i);
 f.buttons.find(b=>b.dataset.chart==='table').dispatch('click');await settle();
 assert.deepEqual(Array.from(f.tables.findLast(t=>t.root==='dataTable'&&t.metric==='amount').options.csvRows[1]),['W',20,30,null]);
});
