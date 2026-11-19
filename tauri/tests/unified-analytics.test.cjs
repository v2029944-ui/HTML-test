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
 for(const id of ['unifiedSource','hideEmptyPeriods']){const node=new Element('input');node.id=id;nodes.set(id,node);}
 for(const [id,v]of Object.entries({periodType:'day',unifiedSource:'daily',overviewMode:'single',chartBreakdown:'works',chartOrgScope:'all',chartSeriesLayout:'combined'}))nodes.get(id).value=v;
 const buttons=['line','bar','table'].map(chart=>{const n=new Element('button');n.dataset.chart=chart;return n;}),draws=[],tables=[],published=[],multi=new Map(),calls=[],notices=[];
 let snapshotHook,viewHook,prints=0;
 const totals={count:2,amount:20,average:10},summary={complete:true,periods:1,organizations:1,missing:0,values:1,total:totals,available_reports:1,available_total:totals,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const points=[{start:'2024-01-01',periods:1,missing:0,total:totals},{start:'2024-01-02',periods:1,missing:0,total:totals}];
 const defaults={unifiedPeriodOptions:async()=>({rows:[{start:'2024-01-01',end:'2024-01-01'}],min:'2024-01-01',max:'2024-01-02',total_rows:2,next:null}),unifiedNameOptions:async()=>({rows:[{key:'work:W',name:'W',kind:'work'}],next:null,total_rows:1}),nativeAnalysisCompare:async()=>({summary:{before:summary,after:summary},top_count:[],top_amount:[]}),nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,missing:0,total:totals}],nativeAnalysisRanking:async()=>({complete:true,rows:[{key:'work:W',name:'W',kind:'work'}]}),nativeAnalysisTimeline:async()=>({granularity:'period',points}),nativeWorkTimeline:async()=>({key:'work:W',name:'W',kind:'work',granularity:'period',points})};
 const api=new Proxy({...defaults,...overrides},{get(target,key){if(key==='load'||key==='save')throw Error('Forbidden full history');if(!(key in target))throw Error('Unexpected API '+key);return (...args)=>{calls.push({key,args});return target[key](...args);};}});
 const originalHelpers={periodEnd:(start,kind)=>start,displayPeriod:p=>p.start,renderMultiFilter:(id,items,selected,callback)=>multi.set(id,{items,selected,callback})};
 const UI={api,revision:1,snapshot:{revision:1,org_names:['A','B','C','D']},state:{selectedOrganizations:new Set(['A']),selectedMetrics:new Set(['amount','count']),chartMetrics:new Set(['amount','count']),chart:'line'},$:id=>nodes.get(id),el:(tag,text,cls)=>{const n=new Element(tag,text);n.className=cls;return n;},fmt:n=>Number.isFinite(n)?String(n):'—',notice:(...args)=>notices.push(args),table:(root,metric,caption,headers,rows,options)=>{tables.push({root:root.id,metric,rows,options});root.append(new Element('table',rows.map(r=>r.join('|')).join('\n')));},drawChart:(...args)=>draws.push(args),drawDepartments:(...args)=>draws.push(args),setPeriodInput:(id,kind,date)=>{const node=nodes.get(id);node.dataset.periodType=kind;if(date!==undefined)node.value=date;},range:(kind,from,to,source)=>from&&from<=to?{type:source==='daily'?'day':kind,displayType:kind,source,from,to}:null,publishScope:s=>published.push(s),onSnapshot:fn=>snapshotHook=fn,onView:(view,fn)=>viewHook=fn};
 const window={unifiedUI:UI,unifiedOriginal:originalHelpers,nativePeriodSelection:{fromIsoDate:date=>date},print:()=>prints++};
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
test('coverage acknowledgement retains the canonical key and never confirms missing data',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const partial={...complete,complete:false,missing:1,total:{count:null,amount:null,average:null},import_warning_reports:1};
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:complete,after:partial},top_count:[],top_amount:[]})}),storage=new Map();f.window.localStorage={getItem:key=>storage.get(key),setItem:(key,v)=>storage.set(key,v)};
 await f.bootstrap();const acknowledge=descendants(f.nodes.get('overviewAlerts')).find(n=>n.tagName==='BUTTON'&&n.textContent.includes('замечание известно'));assert.ok(acknowledge);acknowledge.dispatch('click');
 const expected='sql-coverage-ack-v1:'+JSON.stringify([1,'day','2024-01-02','2024-01-02',['A'],[],1,1,0,0,0,1,1]);assert.equal(storage.get(expected),'accepted');
 assert.match(f.nodes.get('overviewAlerts').textContent,/неполная \(известно\)/);assert.match(f.nodes.get('overviewKpis').textContent,/—/);assert.doesNotMatch(f.nodes.get('overviewAlerts').textContent,/Понятно, замечание известно/);
 f.nodes.get('printSummary').dispatch('click');assert.match(f.nodes.get('printReport').textContent,/Частичный итог/);assert.match(f.nodes.get('printReport').textContent,/исходных отчётах есть замечания/);assert.match(f.nodes.get('printReport').textContent,/20/);
 f.UI.revision=2;f.UI.snapshot.revision=2;await f.bootstrap();assert.match(f.nodes.get('overviewAlerts').textContent,/Понятно, замечание известно/);
});
test('pending and empty sources immediately publish a null scope',async()=>{
 const wait=deferred();let reads=0;const f=fixture({unifiedPeriodOptions:()=>++reads===1?Promise.resolve({min:'2024-01-01',max:'2024-01-02',total_rows:2}):wait.promise});await f.bootstrap();assert.ok(f.published.at(-1));f.nodes.get('periodType').value='month';f.nodes.get('periodType').dispatch('change');assert.equal(f.published.at(-1),null);
 wait.resolve({min:null,max:null,total_rows:0});await settle();assert.equal(f.published.at(-1),null);assert.match(f.nodes.get('overviewAlerts').textContent,/нет отчётов/);
});
test('printed complete summaries include both captured driver tables',async()=>{
 const full={complete:true,periods:1,missing:0,total:{count:2,amount:20,average:10},available_reports:1,available_total:{count:2,amount:20,average:10},invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:1};
 const f=fixture({nativeAnalysisCompare:async()=>({summary:{before:full,after:full},top_count:[{name:'Count driver',kind:'work',count:{base:1,current:2,delta:1}}],top_amount:[{name:'Amount driver',kind:'group',amount:{base:10,current:20,delta:10}}]})});await f.bootstrap();f.nodes.get('printSummary').dispatch('click');const text=f.nodes.get('printReport').textContent;assert.match(text,/Count driver/);assert.match(text,/Amount driver/);assert.match(text,/исходных отчётах есть замечания/);
});
