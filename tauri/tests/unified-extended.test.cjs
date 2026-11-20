const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../unified-extended.js'),'utf8');
const original=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8');
class Element {
 constructor(tag='div',text=''){this.tagName=tag.toUpperCase();this.children=[];this._text=text??'';this.value='';this.dataset={};this.listeners={};this.hidden=false;this.open=false;this.disabled=false;this.classList={toggle(){}};this.attributes={};}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=children;this._text='';}
 set textContent(text){this._text=String(text);this.children=[];}
 get textContent(){return this._text+this.children.map(c=>typeof c==='string'?c:c.textContent).join('');}
 setAttribute(k,v){this.attributes[k]=v;}
 addEventListener(event,callback){(this.listeners[event]??=[]).push(callback);}
 dispatch(event){for(const callback of this.listeners[event]||[])callback({target:this});}
 get options(){return this.children.filter(c=>c.tagName==='OPTION');}
 get selectedOptions(){return this.options.filter(option=>option.value===this.value);}
 focus(){}scrollIntoView(){}
}
function deferred(){let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};}
function fixture(overrides={}){
 const nodes=new Map();for(const match of original.matchAll(/<([a-z][\w-]*)\b[^>]*\bid="([^"]+)"[^>]*>/gi)){const el=new Element(match[1]);el.id=match[2];nodes.set(el.id,el);}
 for(const id of ['comparisonPeriodOverride','structurePeriodOverride','changesPeriodOverride','comparisonPeriodReset','structurePeriodReset','changesPeriodReset','changesSensitivityValue','changesSensitivityExample','changesActiveFilter']){const el=new Element('div');el.id=id;nodes.set(id,el);}
 const defaults={changesType:'day',changesMethod:'previous',changesFrom:'2024-01-01',changesTo:'2024-01-02',changesPercent:'30',changesCountMin:'5',changesAmountMin:'1000',changesMode:'all',changesDirection:'all',changesSort:'name',comparisonMode:'periods',periodComparisonScope:'single',comparisonScope:'single',structureMetric:'amount',structureLimit:'10',structureScope:'selected',usualWindow:'4',usualPercent:'30',regularityGap:'4',regularityStatus:'all'};
 for(const [id,value]of Object.entries(defaults))nodes.get(id).value=value;
 const hooks={scope:[],snapshot:[],views:{}},tables=[],calls=[];
 const api=new Proxy({...overrides},{get(target,key){if(key in target)return target[key];if(key==='load'||key==='save')throw Error('Forbidden full history API');return (...args)=>{calls.push({key,args});return Promise.reject(Error('Unexpected API '+key));};}});
 const UI={api,revision:1,snapshot:{org_names:['A','B','C','D']},state:{selectedMetrics:new Set(['count','amount'])},$:id=>nodes.get(id),el:(tag,text,cls)=>{const node=new Element(tag,text);node.className=cls;return node;},fmt:n=>Number.isFinite(n)?String(n):'—',table:(root,metric,caption,headers,rows,options)=>{tables.push({root:root.id,metric,caption,headers,rows,options});root.append(new Element('table',rows.map(row=>row.map(c=>c?.textContent??String(c??'—')).join('|')).join('\n')));},setPeriodInput:(id,kind,date)=>{nodes.get(id).value=date;},range:(kind,from,to,source)=>/^\d{4}-\d{2}-\d{2}$/.test(from)&&from<=to?{type:source==='daily'?'day':kind,displayType:kind,source,from,to}:null,request:(scope,overrides={})=>({revision:scope.revision,source:scope.source,displayType:scope.displayType,organizations:scope.organizations,selectedNames:scope.selectedNames,query:{method:'previous',from:scope.from,to:scope.to,percent:30,min_count:5,min_amount:1000,...overrides}}),onScope:cb=>hooks.scope.push(cb),onSnapshot:cb=>hooks.snapshot.push(cb),onView:(name,cb)=>(hooks.views[name]??=[]).push(cb),drawChart(){},notice(){}};
 const window={unifiedUI:UI};vm.runInNewContext(script,{window,setTimeout,clearTimeout,console,Date,Map,Set});window.unifiedExtended.init();
 const scope={revision:1,source:'daily',displayType:'day',type:'day',from:'2024-01-01',to:'2024-01-02',organizations:['A'],selectedNames:['Work'],selectedKeys:['work:Work'],nameRows:[{name:'Work',key:'work:Work',kind:'work'}],allNames:false};
 return {UI,nodes,tables,calls,window,publish:patch=>{if(patch===null){UI.scope=null;hooks.scope.forEach(cb=>cb(null));return;}Object.assign(scope,patch);UI.scope={...scope};hooks.scope.forEach(cb=>cb(UI.scope));},view:name=>hooks.views[name]?.forEach(cb=>cb()),snapshot:()=>hooks.snapshot.forEach(cb=>cb(null))};
}
function page(name='Work'){
 const measure={base:0,current:2,delta:2,percent:null,negative:false,small_base:true,significant:false,notable:true};
 return {rows:[{key:'work:'+name,name,kind:'work',org:'A',baseStart:'2024-01-01',currentStart:'2024-01-02',status:'new',count:measure,amount:measure}],next:null,total_rows:1,summary:{rows:1,grown:0,fallen:0,new:1,gone:0,top_count:null,top_amount:null,totals:{missing_pairs:0,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_pairs:0}},missing_examples:[]};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('original controls initialize once and changes discard a late SQL response',async()=>{
 const first=deferred(),second=deferred();let reads=0;const fx=fixture({unifiedChangesPage:()=>++reads===1?first.promise:second.promise});fx.window.unifiedExtended.init();fx.publish();fx.view('changes');
 fx.nodes.get('changesMode').value='strong';fx.nodes.get('changesMode').dispatch('change');assert.equal(reads,2);
 second.resolve(page('Current'));await settle();assert.equal(fx.tables.length,1);assert.match(fx.nodes.get('changesResults').textContent,/Current/);
 first.resolve(page('Stale'));await settle();assert.equal(fx.tables.length,1);assert.doesNotMatch(fx.nodes.get('changesResults').textContent,/Stale/);
 assert.equal(fx.nodes.get('changesResults').attributes['aria-busy'],'false');
});
test('changes export captures full server filters and rejects after a scope change',async()=>{
 const exports=[];let chosen=0;const requests=[];const fx=fixture({unifiedChangesPage:(request,options)=>{requests.push({request,options});return Promise.resolve(page());},chooseCsvPath:async()=>{chosen++;return '/tmp/changes.csv';},unifiedChangesCsv:async(...args)=>exports.push(args)});
 fx.publish();fx.nodes.get('changesSearch').value='Работа';fx.nodes.get('changesDirection').value='new';fx.view('changes');await settle();
 const control=fx.tables[0].options.csvContext;await control.export();assert.equal(chosen,1);assert.equal(exports.length,1);assert.equal(exports[0][1].search,'Работа');assert.equal(exports[0][1].direction,'new');assert.equal(exports[0][1].mode,'all');assert.equal(exports[0][2],'/tmp/changes.csv');
 fx.UI.revision=2;fx.publish({revision:2});await assert.rejects(control.export(),/изменились|изменились/i);assert.equal(exports.length,1);await settle();
});
test('SQL failure clears prior rendered rows and snapshot invalidates in-flight work',async()=>{
 let pending=deferred(),reads=0;const fx=fixture({unifiedChangesPage:()=>++reads===1?Promise.resolve(page('Old')):pending.promise});fx.publish();fx.view('changes');await settle();assert.match(fx.nodes.get('changesResults').textContent,/Old/);
 fx.nodes.get('changesMode').dispatch('change');pending.reject(Error('database failed'));await settle();assert.doesNotMatch(fx.nodes.get('changesResults').textContent,/Old/);assert.match(fx.nodes.get('changesResults').textContent,/database failed/);
 pending=deferred();fx.nodes.get('changesMode').dispatch('change');fx.snapshot();pending.resolve(page('Hidden stale'));await settle();assert.doesNotMatch(fx.nodes.get('changesResults').textContent,/Hidden stale/);
});
test('regularity search/status go to SQL and empty selection does not become all work',async()=>{
 const calls=[];const fx=fixture({unifiedPatternPage:async(...args)=>{calls.push(args);return {rows:[],next:null,total_rows:0,used:1,expected:2,previous_used:0,current_complete:false};}});fx.publish();fx.nodes.get('regularityPanel').open=true;fx.nodes.get('regularitySearch').value='Дальняя';fx.nodes.get('regularityStatus').value='rare';fx.nodes.get('regularityPanel').dispatch('toggle');await settle();assert.equal(calls.length,1);assert.equal(calls[0][3].search,'Дальняя');assert.equal(calls[0][3].status,'rare');
 fx.publish({selectedNames:[],selectedKeys:[],allNames:false});fx.nodes.get('regularityPanel').dispatch('toggle');await settle();assert.equal(calls.length,1);assert.match(fx.nodes.get('regularityResults').textContent,/Отметьте/);
});
test('comparison uses bounded SQL rows and preserves unavailable values',async()=>{
 const calls=[];const total={complete:true,periods:1,missing:0,total:{count:5,amount:10,average:2}};
 const fx=fixture({nativeAnalysisCompare:async(...args)=>{calls.push(['summary',args]);return {summary:{before:total,after:{...total,complete:false,total:{count:null,amount:null,average:null}}}};},unifiedComparisonPage:async(...args)=>{calls.push(['page',args]);return [{key:'work:Known',name:'Known',kind:'work',count:null,amount:null,average:{base:null,current:null,delta:null,percent:null}}];}});
 fx.publish();fx.nodes.get('advancedAnalytics').open=true;fx.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(calls.length,2);assert.equal(calls[1][1][4],50);assert.equal(fx.tables.length,2);
 assert.deepEqual(Array.from(fx.tables[0].headers),['Работа / группа','Значение раньше']);
 assert.ok(!fx.tables[0].rows.some(row=>row[0]?.textContent==='Known'));
 assert.match(fx.nodes.get('comparisonCoverage').textContent,/Сравнение недоступно/);
});
test('a null dashboard scope invalidates pending changes and clears prior rows',async()=>{
 const wait=deferred();const f=fixture({unifiedChangesPage:()=>wait.promise});f.publish();f.view('changes');f.publish(null);wait.resolve(page('Outdated'));await settle();assert.doesNotMatch(f.nodes.get('changesResults').textContent,/Outdated/);assert.equal(f.nodes.get('changesResults').attributes['aria-busy'],'false');
});
test('department CSV columns match displayed headers and retain numeric deltas and percentages',async()=>{
 const f=fixture({nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,missing:0,total:{count:10,amount:20,average:2}},{org:'B',complete:true,missing:0,total:{count:15,amount:30,average:2}}],nativeOrganizationTrends:async()=>({granularity:'period',series:['A','B'].map(organization=>({organization,points:[{start:'2024-01-02',missing:0,total:{count:10,amount:20,average:2}}]}))})});
 f.publish({organizations:['A','B']});f.nodes.get('comparisonMode').value='organizations';f.nodes.get('comparisonBaseOrg').value='A';f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 const table=f.tables.find(table=>table.metric==='count');assert.ok(table);for(const row of table.options.csvRows)assert.equal(row.length,table.headers.length);assert.deepEqual(Array.from(table.options.csvRows[0].slice(-2)),[5,50]);
});
test('daily week/month/year usual-level drilldowns retain the expanded final day',async()=>{
 for(const [kind,from,to,baseline]of [['week','2024-01-08','2024-01-14','2023-12-11'],['month','2024-02-01','2024-02-29','2023-10-01'],['year','2024-01-01','2024-12-31','2020-01-01']]){
  const calls=[],measure={base:1,current:2,delta:1,percent:100,status:'high'};
  const f=fixture({unifiedPatternPage:async()=>({rows:[{key:'work:Work',name:'Work',kind:'work',count:measure,amount:measure,average:measure}],next:null,total_rows:1,used:4,expected:4,previous_used:4,current_complete:true,baseline_from:baseline,baseline_to:from,current_from:from,current_to:from}),nativeWorkTimeline:async(...args)=>{calls.push(args);return {granularity:kind,points:[]};}});
  f.UI.range=(type,start,end,source)=>({type:'day',displayType:type,source,from,to});f.publish({displayType:kind,source:'daily',type:'day',from,to});f.nodes.get('usualPeriod').value=from;f.nodes.get('usualPanel').open=true;f.nodes.get('usualPanel').dispatch('toggle');await settle();
  const table=f.tables.find(table=>table.root==='usualResults');assert.ok(table,kind);table.rows[0][0].dispatch('click');await settle();assert.equal(calls.length,1,kind);assert.equal(calls[0][2],baseline,kind);assert.equal(calls[0][3],to,kind);assert.equal(calls[0][8],kind);
 }
});

test('composition hides base-share columns and zero-valued rows when baseline is missing',async()=>{
 const f=fixture({nativeCompositionPage:async()=>({
  currentComplete:true,baseComplete:false,sharesValid:true,total:15,
  rows:[{key:'work:A',name:'АКБ Замена,',current:15,share:100,cumulative:100,base:null,baseShare:null,shareChange:null},
        {key:'work:Zero',name:'Нулевая работа',current:0,share:0,cumulative:100,base:null}]
 })});
 f.publish();f.nodes.get('structurePanel').open=true;f.nodes.get('structurePanel').dispatch('toggle');await settle();
 const table=f.tables.find(t=>t.root==='structureResults');
 assert.ok(table);assert.deepEqual(Array.from(table.headers),['Услуга / группа','Значение сейчас','Доля сейчас','Накопленная доля']);
 assert.equal(table.rows.length,1);assert.equal(table.rows[0][0].textContent,'АКБ Замена');
 assert.match(f.nodes.get('structureResults').textContent,/Показано 1 из 10/);
});

test('clicking a change KPI filters the SQL rows by the selected category',async()=>{
 const selected=[];const f=fixture({unifiedChangesPage:async(_request,options)=>{selected.push(options.direction);return page();}});
 f.publish();f.view('changes');await settle();
 const cards=f.nodes.get('changesSummary').children;
 assert.equal(cards[1].tagName,'BUTTON');assert.match(cards[1].attributes['aria-label'],/Значительно снизилось/);
 cards[1].dispatch('click');await settle();
 assert.equal(f.nodes.get('changesDirection').value,'down');assert.equal(selected.at(-1),'down');
});

test('shared baseline seeds comparison and structure, local overrides stay flagged until reset',async()=>{
 const f=fixture();
 f.publish({baseFrom:'2023-12-28',baseTo:'2023-12-29',overviewMode:'range'});
 assert.equal(f.nodes.get('basePeriod').value,'2023-12-28');
 assert.equal(f.nodes.get('baseRangeEnd').value,'2023-12-29');
 assert.equal(f.nodes.get('structureBase').value,'2023-12-28');
 assert.equal(f.nodes.get('periodComparisonScope').value,'range');
 f.nodes.get('structureBase').value='2023-12-25';f.nodes.get('structureBase').dispatch('change');
 assert.equal(f.nodes.get('structurePeriodOverride').hidden,false);
 f.publish({baseFrom:'2023-12-26',baseTo:'2023-12-27'});
 assert.equal(f.nodes.get('structureBase').value,'2023-12-25','local override must survive changed shared scope');
 f.nodes.get('structurePeriodReset').dispatch('click');
 assert.equal(f.nodes.get('structureBase').value,'2023-12-26');
 assert.equal(f.nodes.get('structurePeriodOverride').hidden,true);
});

test('imported week captions and one-period chart bounds show Sunday as the end',async()=>{
 const plots=[];const f=fixture({nativeCompositionPage:async()=>({
  currentComplete:true,baseComplete:true,sharesValid:true,total:10,
  rows:[{key:'work:W',name:'Work',current:10,base:5,baseShare:100,share:100,cumulative:100,shareChange:0}]
 })});
 f.UI.drawChart=(...args)=>plots.push(args);
 f.publish({displayType:'week',type:'week',source:'imported',from:'2024-01-01',to:'2024-01-01',baseFrom:'2023-12-25',baseTo:'2023-12-25'});
 f.nodes.get('structurePanel').open=true;f.nodes.get('structurePanel').dispatch('toggle');await settle();
 const table=f.tables.find(t=>t.root==='structureResults');assert.ok(table);
 assert.match(table.caption,/01\.01\.2024 — 07\.01\.2024/);
 assert.equal(plots[0][0][0].end,'2024-01-07');
});

test('changing global calendar granularity resets an obsolete local change-view type',()=>{
 const f=fixture();f.publish();
 assert.equal(f.nodes.get('changesType').value,'day');
 f.nodes.get('changesType').value='month';f.nodes.get('changesType').dispatch('change');
 f.publish({source:'daily',displayType:'week',type:'day',from:'2024-01-01',to:'2024-01-07'});
 assert.equal(f.nodes.get('changesType').value,'week');
 assert.equal(f.nodes.get('changesPeriodOverride').hidden,true);
});

test('sensitivity slider updates its clear example and preserves numeric SQL thresholds',async()=>{
 const args=[];const f=fixture({unifiedChangesPage:async(request,options)=>{args.push({request,options});return page();}});
 f.publish();f.view('changes');await settle();
 assert.match(f.nodes.get('changesSensitivityExample').textContent,/было 100/);
 assert.equal(f.nodes.get('changesSensitivityValue').textContent,'30%');
 f.nodes.get('changesPercent').value='55';f.nodes.get('changesPercent').dispatch('input');
 assert.equal(f.nodes.get('changesSensitivityValue').textContent,'55%');
 assert.match(f.nodes.get('changesSensitivityExample').textContent,/55/);
 await new Promise(resolve=>setTimeout(resolve,200));await settle();
 assert.equal(args.at(-1).request.query.percent,55);
 assert.equal(args.at(-1).request.query.min_count,5);
 assert.equal(args.at(-1).request.query.min_amount,1000);
});
test('hidden advanced controls advertise an active KPI direction filter',async()=>{
 const f=fixture({unifiedChangesPage:async()=>page()});f.publish();f.view('changes');await settle();
 const category=f.nodes.get('changesSummary').children[1];
 category.dispatch('click');await settle();
 assert.equal(f.nodes.get('changesDirection').value,'down');
 assert.match(f.nodes.get('changesActiveFilter').textContent,/снижение/);
 f.nodes.get('changesDirection').value='all';f.nodes.get('changesDirection').dispatch('change');await settle();
 assert.doesNotMatch(f.nodes.get('changesActiveFilter').textContent,/снижение/);
});
