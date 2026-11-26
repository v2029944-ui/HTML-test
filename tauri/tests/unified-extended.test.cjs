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
 for(const id of ['comparisonPeriodOverride','structurePeriodOverride','changesPeriodOverride','comparisonPeriodReset','structurePeriodReset','changesPeriodReset','changesSensitivityValue','changesSensitivityExample','changesActiveFilter','workViewMode','workViewDescription']){const el=new Element('div');el.id=id;nodes.set(id,el);}
 const defaults={changesType:'day',changesMethod:'previous',changesFrom:'2024-01-01',changesTo:'2024-01-02',changesPercent:'30',changesCountMin:'5',changesAmountMin:'1000',changesMode:'all',changesDirection:'all',changesSort:'name',comparisonMode:'periods',periodComparisonScope:'single',comparisonScope:'single',structureMetric:'amount',structureLimit:'10',structureScope:'selected',usualWindow:'4',usualPercent:'30',regularityGap:'4',regularityStatus:'all'};
 for(const [id,value]of Object.entries(defaults))nodes.get(id).value=value;
 const hooks={scope:[],snapshot:[],views:{}},tables=[],calls=[];
 const methods={...overrides};
 for(const [direct,legacy,convert]of [['unifiedChangesPageAtPage','unifiedChangesPage',a=>[a[0],a[1],null]],['unifiedPatternPageAtPage','unifiedPatternPage',a=>[...a.slice(0,4),null]],['nativeCompositionPageAtPage','nativeCompositionPage',a=>[...a.slice(0,4),null,a[5]]],['unifiedNameOptionsAtPage','unifiedNameOptions',a=>[a[0],a[1],null,a[3]]]])if(!methods[direct]&&methods[legacy])methods[direct]=(...args)=>methods[legacy](...convert(args));
 if(!methods.unifiedComparisonWindow&&methods.unifiedComparisonPage)methods.unifiedComparisonWindow=async(...args)=>{const rows=await methods.unifiedComparisonPage(...args.slice(0,3),null,args[4]);return {rows,total_rows:rows.length};};
 const api=new Proxy(methods,{get(target,key){if(key in target)return target[key];if(key==='load'||key==='save')throw Error('Forbidden full history API');return (...args)=>{calls.push({key,args});return Promise.reject(Error('Unexpected API '+key));};}});
 const UI={api,revision:1,snapshot:{org_names:['A','B','C','D']},state:{selectedMetrics:new Set(['count','amount'])},$:id=>nodes.get(id),el:(tag,text,cls)=>{const node=new Element(tag,text);node.className=cls;return node;},fmt:n=>Number.isFinite(n)?String(n):'—',table:(root,metric,caption,headers,rows,options)=>{tables.push({root:root.id,metric,caption,headers,rows,options});root.append(new Element('table',rows.map(row=>row.map(c=>c?.textContent??String(c??'—')).join('|')).join('\n')));},setPeriodInput:(id,kind,date)=>{nodes.get(id).value=date;},range:(kind,from,to,source)=>/^\d{4}-\d{2}-\d{2}$/.test(from)&&from<=to?{type:source==='daily'?'day':kind,displayType:kind,source,from,to}:null,request:(scope,overrides={})=>({revision:scope.revision,source:scope.source,displayType:scope.displayType,organizations:scope.organizations,selectedNames:scope.selectedNames,query:{method:'previous',from:scope.from,to:scope.to,percent:30,min_count:5,min_amount:1000,...overrides}}),onScope:cb=>hooks.scope.push(cb),onSnapshot:cb=>hooks.snapshot.push(cb),onView:(name,cb)=>(hooks.views[name]??=[]).push(cb),drawChart(){},notice(){}};
 const window={unifiedUI:UI};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),{window});vm.runInNewContext(script,{window,setTimeout,clearTimeout,console,Date,Map,Set});window.unifiedExtended.init();
 const scope={revision:1,source:'daily',displayType:'day',type:'day',from:'2024-01-01',to:'2024-01-02',organizations:['A'],selectedNames:['Work'],selectedKeys:['work:Work'],nameRows:[{name:'Work',key:'work:Work',kind:'work'}],allNames:false};
 return {UI,nodes,tables,calls,window,publish:patch=>{if(patch===null){UI.scope=null;hooks.scope.forEach(cb=>cb(null));return;}Object.assign(scope,patch);UI.scope={...scope};hooks.scope.forEach(cb=>cb(UI.scope));},view:name=>hooks.views[name]?.forEach(cb=>cb()),snapshot:()=>hooks.snapshot.forEach(cb=>cb(null))};
}
function page(name='Work'){
 const measure={base:0,current:2,delta:2,percent:null,negative:false,small_base:true,significant:false,notable:true};
 return {rows:[{key:'work:'+name,name,kind:'work',org:'A',baseStart:'2024-01-01',currentStart:'2024-01-02',status:'new',count:measure,amount:measure}],next:null,total_rows:1,summary:{rows:1,grown:0,fallen:0,new:1,gone:0,top_count:null,top_amount:null,totals:{missing_pairs:0,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_pairs:0}},missing_examples:[]};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('extended SQL pagination seeks directly, removes duplicate ranges and rejects stale callbacks',async()=>{
 const requests=[];let fail=false;const fx=fixture({unifiedChangesPageAtPage:async(...args)=>{requests.push(args);if(fail)throw Error('scroll SQL failed');return {...page(),next:args[2]?null:'changes-cursor',total_rows:120};}});
 const attached=[],reset=[];fx.window.sqlRowScroll={attach:(root,options)=>{attached.push({root,options});return root;},reset:root=>reset.push(root),retire:(...nodes)=>nodes.forEach(node=>{if(node)node.hidden=true;})};
 fx.publish();fx.view('changes');await settle();let entry=attached.findLast(x=>x.root.id==='changesResults');assert.ok(entry);assert.equal(entry.options.size,50);assert.equal(entry.options.direct,true);assert.equal(entry.options.total,120);
 assert.equal(fx.nodes.get('changesPrev').hidden,true);assert.equal(fx.nodes.get('changesNext').hidden,true);assert.equal(fx.nodes.get('changesPage').textContent,'');
 const resets=reset.filter(root=>root.id==='changesResults').length;await entry.options.go(4);assert.equal(requests.length,1);await entry.options.go(2);assert.equal(requests[1][2],2);entry=attached.at(-1);await entry.options.go(1);assert.equal(requests[2][2],1);entry=attached.at(-1);assert.equal(entry.options.index,1);
 await entry.options.go(0);assert.equal(requests[3][2],0);assert.equal(reset.filter(root=>root.id==='changesResults').length,resets,'successful paging must preserve shared pending edge');assert.doesNotMatch(fx.nodes.get('changesPage').textContent,/Страница/);entry=attached.at(-1);fail=true;await entry.options.go(1);assert.match(entry.root.textContent,/scroll SQL failed/);assert.ok(reset.includes(entry.root));
 const count=requests.length;await entry.options.go(0);assert.equal(requests.length,count);fx.publish(null);assert.ok(reset.includes(entry.root));
});
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
 assert.match(fx.nodes.get('comparisonCoverage').textContent,/Неполные данные анализируемого периода/);
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
 f.nodes.get('changesMode').value='strong';
 assert.equal(cards[1].tagName,'BUTTON');assert.match(cards[1].attributes['aria-label'],/Значительно снизилось/);
 cards[1].dispatch('click');await settle();
 assert.equal(f.nodes.get('changesDirection').value,'significant-down');assert.equal(selected.at(-1),'significant-down');assert.equal(f.nodes.get('changesMode').value,'all');
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
 assert.equal(f.nodes.get('changesDirection').value,'significant-down');
 assert.match(f.nodes.get('changesActiveFilter').textContent,/^фильтр: значительное снижение/);
 f.nodes.get('changesDirection').value='all';f.nodes.get('changesDirection').dispatch('change');await settle();
 assert.doesNotMatch(f.nodes.get('changesActiveFilter').textContent,/снижение/);
});

test('import warnings are not mislabeled as missing comparisons',async()=>{
 const p=page('Import note');
 p.summary.totals.import_warning_pairs=4;
 p.summary.totals.missing_pairs=0;
 const f=fixture({unifiedChangesPage:async()=>p});f.publish();f.view('changes');await settle();
 const message=f.nodes.get('changesWarnings').textContent;
 assert.match(message,/Сопоставимых строк по текущим фильтрам: 1/);
 assert.match(message,/Пар с замечаниями импорта: 4/);
 assert.doesNotMatch(message,/Нет данных для сравнения: 0|не включены в сопоставление/);
});
test('missing report pairs are reported separately with formatted dates',async()=>{
 const p=page('Missing');
 p.summary.totals.missing_pairs=2;
 p.missing_examples=[{org:'A',missing_base:true,base_start:'2024-01-01',missing_current:false}];
 const f=fixture({unifiedChangesPage:async()=>p});f.publish();f.view('changes');await settle();
 const message=f.nodes.get('changesWarnings').textContent;
 assert.match(message,/Нет полной пары отчётов: 2/);
 assert.match(message,/не включены в сопоставление/);
 assert.match(message,/01\.01\.2024/);
});

test('work view toggle keeps composition SQL request limited and uses existing controls',async()=>{
 const queries=[];
 const f=fixture({nativeCompositionPage:async(request,base,mode,metric,cursor,limit)=>{
  queries.push({request,base,mode,metric,cursor,limit});
  return {currentComplete:true,baseComplete:true,sharesValid:true,total:0,rows:[]};
 }});
 f.publish();
 assert.equal(f.nodes.get('chartPanel').hidden,false);
 assert.equal(f.nodes.get('structurePanel').hidden,true);
 f.nodes.get('workViewMode').value='composition';
 f.nodes.get('workViewMode').dispatch('change');
 assert.equal(f.nodes.get('chartPanel').hidden,true);
 assert.equal(f.nodes.get('structurePanel').hidden,false);
 f.nodes.get('structurePanel').dispatch('toggle');await settle();
 assert.equal(queries.length,1);
 assert.equal(queries[0].mode,'structure');
 assert.equal(queries[0].limit,10);
 f.nodes.get('workViewMode').value='trend';
 f.nodes.get('workViewMode').dispatch('change');
 assert.equal(f.nodes.get('chartPanel').hidden,false);
 assert.equal(f.nodes.get('structurePanel').hidden,true);
 assert.equal(queries.length,1,'trends must not fetch composition rows');
});

test('department comparison dynamics include selected organizations beyond eight',async()=>{
 const organizations=Array.from({length:10},(_,i)=>'Org '+i),plots=[],queries=[];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>organizations.map(org=>({org,complete:true,missing:0,total:{count:2.5,amount:-10.75,average:-4.3}})),
  nativeOrganizationTrends:async(_rev,_kind,_from,_to,orgs)=>{
   queries.push(Array.from(orgs));return {granularity:'period',series:orgs.map(organization=>({organization,points:['2024-01-01','2024-01-02'].map(start=>({start,missing:0,total:{count:2.5,amount:-10.75,average:-4.3}}))}))};
  }
 });
 f.UI.drawChart=(...args)=>plots.push(args);f.UI.snapshot.org_names=organizations;
 f.publish({organizations,overviewMode:'range'});f.nodes.get('comparisonMode').value='organizations';f.nodes.get('comparisonBaseOrg').value='Org 0';
 f.nodes.get('advancedAnalytics').open=true;f.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 const chart=plots.find(args=>args[4].chart==='line');
 assert.equal(chart[4].data[0].get('Org 9')?.amount,-10.75);
 assert.deepEqual(queries,[organizations.slice(0,8),organizations.slice(8)]);
});

test('organization comparison overrides reset after a kind change and follow the shared interval',()=>{
 const f=fixture();f.publish();f.nodes.get('comparisonMode').value='organizations';
 f.nodes.get('comparisonPeriod').value='2024-01-03';f.nodes.get('comparisonPeriod').dispatch('change');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,false);
 f.publish({displayType:'week',from:'2024-01-08',to:'2024-01-14'});
 f.publish({from:'2024-01-15',to:'2024-01-21'});
 assert.equal(f.nodes.get('comparisonPeriod').value,'2024-01-21');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,true);
 f.nodes.get('comparisonPeriod').value='2024-01-14';f.nodes.get('comparisonPeriod').dispatch('change');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,false);
 f.nodes.get('comparisonPeriodReset').dispatch('click');
 assert.equal(f.nodes.get('comparisonPeriod').value,'2024-01-21');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,true);
 f.nodes.get('comparisonScope').value='range';f.nodes.get('comparisonScope').dispatch('change');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,false);
 f.nodes.get('comparisonRangeStart').value='2024-01-01';f.nodes.get('comparisonRangeStart').dispatch('change');
 f.nodes.get('comparisonPeriodReset').dispatch('click');
 assert.equal(f.nodes.get('comparisonScope').value,'single');
 assert.equal(f.nodes.get('comparisonRangeStart').value,'2024-01-15');
 assert.equal(f.nodes.get('comparisonPeriodOverride').hidden,true);
});

test('two incomplete comparison intervals explain both gaps without empty tables, exports or pager',async()=>{
 for(const length of [0,1,50]){
  const incomplete={complete:false,periods:1,missing:1,total:{count:null,amount:null,average:null}};
  const fx=fixture({nativeAnalysisCompare:async()=>({summary:{before:incomplete,after:incomplete}}),unifiedComparisonPage:async()=>Array.from({length},(_,i)=>({key:'work:Known'+i,name:'Known'+i,kind:'work',count:null,amount:null,average:{base:null,current:null,delta:null,percent:null}}))});
  fx.publish();fx.nodes.get('advancedAnalytics').open=true;fx.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
  assert.equal(fx.tables.length,0);assert.match(fx.nodes.get('comparisonTable').textContent,/Нет доступных значений/);
  assert.doesNotMatch(fx.nodes.get('comparisonTable').textContent,/Предыдущая|Следующая|Страница|Known/);
  assert.match(fx.nodes.get('comparisonCoverage').textContent,/базового и анализируемого периодов/);
  assert.equal(fx.nodes.get('comparisonTable').attributes['aria-busy'],'false');
 }
});
test('confirmed zero comparison retains real values and matching CSV columns',async()=>{
 const complete={complete:true,periods:1,missing:0,total:{count:0,amount:0,average:null}},measure={base:0,current:0,delta:0,percent:null};
 const fx=fixture({nativeAnalysisCompare:async()=>({summary:{before:complete,after:complete}}),unifiedComparisonPage:async()=>[{key:'work:Zero',name:'Zero',kind:'work',count:measure,amount:measure,average:{base:null,current:null,delta:null,percent:null}}]});
 fx.publish();fx.nodes.get('advancedAnalytics').open=true;fx.nodes.get('advancedAnalytics').dispatch('toggle');await settle();
 assert.equal(fx.tables.length,2);for(const table of fx.tables){assert.deepEqual(Array.from(table.options.csvRows[1]),['Zero',0,0,0,null]);assert.equal(table.headers.length,table.options.csvRows[1].length);assert.equal(table.rows[1][1],'0');assert.equal(table.rows[1][2],'0');assert.equal(table.rows[1][4],'—');}
 assert.match(fx.nodes.get('comparisonTable').textContent,/Предыдущая/);
});

test('reference comparison uses exact shared intervals and preserves selected organizations in SQL and export',async()=>{
 const requests=[],applied=[],exports=[];
 const total={complete:true,periods:1,missing:0,total:{count:1,amount:15,average:15}};
 const f=fixture({
  nativeAnalysisOrganizations:async(_rev,_kind,from,to)=>[
   {org:'A',complete:true,missing:0},{org:'B',complete:from==='2024-01-01',missing:from==='2024-01-01'?0:1}
  ],
  nativeAnalysisCompare:async(...args)=>{requests.push(args);return {summary:{before:total,after:{...total,complete:false,missing:1,total:{count:null,amount:null,average:null},available_total:total.total}}};},
  unifiedComparisonPage:async(req)=>{assert.deepEqual([...req.organizations],['A','B']);return [];},
  chooseCsvPath:async()=>'/tmp/ref.csv',referenceDetailCsv:async(...args)=>exports.push(args)
 });
 f.window.referenceDetail={active:'comparison'};f.window.referenceFilters={setOrganizations:names=>applied.push(...names)};
 f.publish({from:'2024-01-10',to:'2024-01-20',baseFrom:'2024-01-01',baseTo:'2024-01-04',organizations:['A','B']});
 await f.window.unifiedExtended.openComparison();
 assert.match(f.nodes.get('comparisonHelp').textContent,/Область расчёта: выбранные работы и группы \(1\)/);assert.match(f.nodes.get('comparisonHelp').textContent,/Сводка и главный «Анализ изменений» учитывают все работы/);
 assert.deepEqual(requests[0].slice(2,6),['2024-01-01','2024-01-04','2024-01-10','2024-01-20']);
 assert.deepEqual([...requests[0][6]],['A','B']);
 const exportView=f.tables[0].options.csvContext;
 await exportView.export();
 assert.deepEqual([...exports[0][0].request.organizations],['A','B']);assert.equal(exports[0][0].section,'comparison');
 assert.deepEqual({...exports[0][0].before},{from:'2024-01-01',to:'2024-01-04'});assert.equal(exports[0][1],'/tmp/ref.csv');
 const note=f.nodes.get('comparisonCoverage').children[0];
 assert.match(note.textContent,/Полные отчёты у 1 из 2 подразделений/);
 note.children.at(-1).dispatch('click');assert.deepEqual(applied,['A']);
 f.window.referenceDetail.active='storage';await assert.rejects(exportView.export(),/изменились/);assert.equal(exports.length,1);
});

test('reference regularity retains incomplete organizations and matches the chosen monthly unit',async()=>{
 const calls=[];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>{throw Error('Do not exclude entire organization');},
  unifiedPatternPage:async(...args)=>{calls.push(args);return {rows:[],total_rows:0,used:0,expected:1,previous_used:1,current_complete:false};}
 });
 f.window.referenceDetail={active:'regularityPanel'};
 f.publish({from:'2024-02-01',to:'2024-02-29',displayType:'month',baseFrom:'2023-02-01',baseTo:'2023-02-28'});
 f.nodes.get('regularityPanel').open=true;f.nodes.get('regularityPanel').dispatch('toggle');await settle();
 assert.equal(calls.length,1);
 assert.equal(calls[0][0].displayType,'month');
 assert.deepEqual({...calls[0][3].baseline},{from:'2023-02-01',to:'2023-02-28'});
 assert.match(f.nodes.get('regularityCoverage').textContent,/Полных периодов анализа: 0 из 1/);
});

test('reference comparison and its CSV keep four organizations and calculate differences on available totals',async()=>{
 for(const [missingBefore,missingAfter,zeroBase]of [[false,true,false],[true,false,false],[true,true,false],[false,true,true]]){
  const selected=['A','B','C','D'],exports=[],calls=[];
  const baseline=zeroBase?0:90,current=210;
  const values=n=>({count:n/10,amount:n,average:n?10:null});
  const total=(n,missing)=>({complete:!missing,periods:2,missing:Number(missing),total:missing?{count:null,amount:null,average:null}:values(n),available_total:values(n)});
  const f=fixture({
   nativeAnalysisOrganizations:async(_rev,_kind,from)=>selected.map(org=>({org,complete:org!=='D'||!(from==='2024-01-01'?missingBefore:missingAfter),missing:org==='D'&&(from==='2024-01-01'?missingBefore:missingAfter)?1:0})),
   nativeAnalysisCompare:async(...args)=>{calls.push(args);assert.deepEqual([...args[6]],selected);return {summary:{before:total(baseline,missingBefore),after:total(current,missingAfter)}};},
   unifiedComparisonWindow:async(req,before)=>{assert.deepEqual([...req.organizations],selected);assert.deepEqual({...before},{from:'2024-01-01',to:'2024-01-02'});return {rows:[{key:'work:Work',name:'Work',kind:'work',observed_before:values(baseline),observed_after:values(current)}],total_rows:1};},
   chooseCsvPath:async()=>'/tmp/comparable.csv',referenceDetailCsv:async(...args)=>exports.push(args)
  });
  f.window.referenceDetail={active:'comparison'};f.window.referenceFilters={setOrganizations(){}};
  f.publish({from:'2024-01-10',to:'2024-01-11',baseFrom:'2024-01-01',baseTo:'2024-01-02',organizations:selected,selectedNames:[],selectedKeys:[],allNames:true});
  await f.window.unifiedExtended.openComparison();
  assert.match(f.nodes.get('comparisonHelp').textContent,/Область расчёта: все работы/);assert.equal(calls.length,1);assert.deepEqual(calls[0].slice(2,6),['2024-01-01','2024-01-02','2024-01-10','2024-01-11']);
  const amount=f.tables.find(table=>table.root==='comparisonTable'&&table.metric==='amount');assert.ok(amount);assert.deepEqual([...amount.options.csvRows[0].slice(1)],[baseline,current,current-baseline,zeroBase?null:(current-baseline)/baseline*100]);
  const text=f.nodes.get('comparisonCoverage').textContent;assert.match(text,/Полные отчёты у 3 из 4 подразделений/);assert.equal((text.match(/Неполные данные/g)||[]).length,1);assert.match(text,/Сравнить только с полными отчётами/);if(missingBefore)assert.match(text,/D · базовый период · пропусков: 1/);if(missingAfter)assert.match(text,/D · анализируемый период · пропусков: 1/);
  assert.equal(text.includes('Первый интервал: Полные данные'),!missingBefore);
  assert.equal(text.includes('Второй интервал: Полные данные'),!missingAfter);
  assert.equal(f.nodes.get('comparisonCoverage').children.filter(n=>/\bwarn\b|comparison-warning/.test(n.className)).length,1);
  await amount.options.csvContext.export();assert.deepEqual([...exports[0][0].request.organizations],selected);assert.deepEqual({...exports[0][0].before},{from:'2024-01-01',to:'2024-01-02'});
  if(zeroBase){assert.equal(amount.rows[0][1],'0');assert.equal(amount.rows[0][4],'—');assert.doesNotMatch(f.nodes.get('comparisonTable').textContent,/Infinity|NaN/);}
 }
});

test('reference comparison with disjoint complete organizations explains both incomplete intervals and offers no numerical comparison',async()=>{
 const f=fixture({nativeAnalysisOrganizations:async(_rev,_kind,from)=>['A','B','C','D'].map(org=>({org,complete:from==='2024-01-01'?['A','B'].includes(org):['C','D'].includes(org),missing:1})),
  nativeAnalysisCompare:async()=>({summary:{before:{complete:false,periods:1,missing:2,total:{count:null,amount:null,average:null}},after:{complete:false,periods:1,missing:2,total:{count:null,amount:null,average:null}}}}),
  unifiedComparisonWindow:async()=>({rows:[],total_rows:0})});
 f.window.referenceDetail={active:'comparison'};f.publish({from:'2024-01-10',to:'2024-01-10',baseFrom:'2024-01-01',baseTo:'2024-01-01',organizations:['A','B','C','D'],selectedNames:[],selectedKeys:[],allNames:true});
 await f.window.unifiedExtended.openComparison();assert.equal(f.tables.length,0);assert.equal(f.nodes.get('kpis').children.length,0);assert.match(f.nodes.get('comparisonCoverage').textContent,/Нет сопоставимых подразделений/);assert.match(f.nodes.get('comparisonTable').textContent,/оба интервала неполны/);
});

test('adjacent pairs follow the exact common unit and interval even when old local dates disagree',async()=>{
 const requests=[];
 const f=fixture({unifiedChangesPageAtPage:async req=>{requests.push(req);return page('Work');}});
 f.window.referenceDetail={active:'changes'};
 f.publish({displayType:'month',source:'daily',from:'2024-02-01',to:'2024-02-29',overviewMode:'range'});
 f.nodes.get('changesFrom').value='2023-12-01';f.nodes.get('changesTo').value='2023-12-31';
 f.view('changes');await settle();
 assert.deepEqual([requests.at(-1).query.method,requests.at(-1).query.from,requests.at(-1).query.to],['previous','2024-02-01','2024-02-29']);
 f.publish({from:'2024-01-01',to:'2024-03-31'});await settle();
 assert.deepEqual([requests.at(-1).query.method,requests.at(-1).query.from,requests.at(-1).query.to],['range','2024-01-01','2024-03-31']);
});

test('history receives focus before SQL and closing restores the same logical source row after a redraw',async()=>{
 const wait=deferred(),fx=fixture({unifiedChangesPage:async()=>page('Work'),nativeWorkTimeline:()=>wait.promise});
 fx.publish();fx.view('changes');await settle();
 const source=fx.tables.find(t=>t.root==='changesResults').rows[0][0],heading=fx.nodes.get('changeDetailTitle');
 let focused=source;heading.focus=()=>{focused=heading;};
 const sibling=new Element('button'),replacement=new Element('button'),host=fx.nodes.get('changesResults');
 sibling.dataset.historyEntry=source.dataset.historyEntry;replacement.dataset.historyEntry=source.dataset.historyEntry;
 const before=[sibling,source],after=[sibling,replacement];host.querySelectorAll=()=>source.isConnected?before:after;
 source.isConnected=true;source.closest=()=>host;source.focus=()=>{focused=source;};replacement.focus=()=>{focused=replacement;};
 source.dispatch('click');assert.equal(focused,heading,'Escape must target the drilldown while its SQL is pending');
 source.isConnected=false;fx.nodes.get('closeChangeDetails').dispatch('click');
 assert.equal(fx.nodes.get('changeDetails').hidden,true);assert.equal(focused,replacement,'same occurrence when two metric rows have the same work');
 wait.resolve({points:[],granularity:'day'});await settle();assert.equal(focused,replacement);
});

test('contribution CSV becomes unavailable as soon as the local baseline changes and stays unavailable after comparison failure',async()=>{
 let baseline='2024-01-08',blocked;
 const saved=[],compositionReads=[];
 const period={complete:true,periods:1,missing:0,total:{count:1,amount:10,average:10}};
 const f=fixture({
  nativeAnalysisOrganizations:async()=>[{org:'A',...period}],
  nativeAnalysisCompare:()=>blocked?blocked.promise:Promise.resolve({summary:{before:period,after:period}}),
  unifiedComparisonWindow:async()=>({rows:[],total_rows:0}),
  nativeCompositionPageAtPage:async(_request,before)=>{
   compositionReads.push(before.from);
   return {rows:[{name:'Work',key:'work:Work',kind:'work',base:1,current:2,delta:1,percent:100,contribution:100}],
    total_rows:1,baseComplete:true,currentComplete:true,check:{sum:1,delta:1,reconciled:true}};
  },
  chooseCsvPath:async()=>'/tmp/contribution.csv',referenceDetailCsv:async spec=>saved.push(spec)
 });
 f.UI.state.selectedMetrics=new Set(['amount']);
 f.window.referenceDetail={active:'comparison',currentComparison:()=>({
  before:{from:baseline,to:'2024-01-09'},after:{from:'2024-01-10',to:'2024-01-11'}})};
 f.publish();const detail=f.nodes.get('contributions'),results=f.nodes.get('contributionResults');
 detail.open=true;await f.window.unifiedExtended.openComparison();await settle();
 const old=f.tables.findLast(t=>t.root===results.id).options.csvContext;
 const dialog=deferred();f.UI.api.chooseCsvPath=()=>dialog.promise;
 const saving=old.export();
 baseline='2024-01-05';blocked=deferred();
 const update=f.window.unifiedExtended.openComparison();
 assert.equal(results.inert,true);
 dialog.resolve('/tmp/old.csv');await assert.rejects(saving,/изменились/);
 await assert.rejects(old.export(),/изменились/);
 detail.dispatch('toggle');await settle();
 assert.deepEqual(compositionReads,['2024-01-08']);
 blocked.reject(Error('comparison read failed'));await update;
 assert.match(results.textContent,/comparison read failed/);assert.equal(results.children.some(n=>n.tagName==='TABLE'),false);
 detail.dispatch('toggle');await settle();await assert.rejects(old.export(),/изменились/);
 assert.deepEqual(compositionReads,['2024-01-08']);assert.equal(saved.length,0);
 blocked=null;await f.window.unifiedExtended.openComparison();await settle();
 await f.tables.findLast(t=>t.root===results.id).options.csvContext.export();
 assert.equal(saved.length,1);assert.deepEqual({...saved[0].before},{from:'2024-01-05',to:'2024-01-09'});
});

test('partial contributions retain all selected departments and export observed rows without changes',async()=>{
 const complete={complete:true,periods:2,missing:0,total:{count:2,amount:20,average:10}};
 const partial={...complete,complete:false,missing:1,total:{count:null,amount:null,average:null},available_total:{count:1,amount:0,average:0}};
 const reads=[],exports=[];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>[{org:'A',...complete},{org:'B',...partial}],
  nativeAnalysisCompare:async()=>({summary:{before:complete,after:partial}}),
  unifiedComparisonWindow:async()=>({rows:[],total_rows:0}),
  nativeCompositionPageAtPage:async (request,_before,section,metric,index)=>{
   reads.push([Array.from(request.organizations),section,metric,index]);
   return {baseComplete:true,currentComplete:false,total_rows:1,rows:[
    {key:'work:Work',name:'Work',kind:'work',base:20,current:0,delta:null,percent:null,contribution:null}
   ]};
  },
  chooseCsvPath:async()=>'/tmp/partial.csv',referenceDetailCsv:async spec=>exports.push(spec)
 });
 f.window.referenceDetail={active:'comparison',currentComparison:()=>({
  before:{from:'2024-01-01',to:'2024-01-02'},after:{from:'2024-01-03',to:'2024-01-04'}
 })};
 f.publish({organizations:['A','B']});f.nodes.get('contributions').open=true;
 await f.window.unifiedExtended.openComparison();await settle();
 assert.equal(reads.length,2);assert.ok(reads.every(([organizations])=>organizations.join()=== 'A,B'));
 const notice=f.nodes.get('contributionCheck').children.filter(n=>n.className.includes('warn'));
 assert.equal(notice.length,1);assert.match(notice[0].textContent,/Показаны доступные значения/);
 const table=f.tables.find(t=>t.root==='contributionResults'&&t.metric==='amount');
 assert.deepEqual(Array.from(table.rows[0].slice(1)),['20','0','—','—','—']);
 assert.deepEqual(Array.from(table.options.csvRows[0]),['Work',20,0,null,null,null]);
 await table.options.csvContext.export();
 assert.deepEqual(Array.from(exports[0].request.organizations),['A','B']);
 assert.equal(exports[0].section,'contributions');
});

test('month scope with equal-length October baseline uses exact daily intervals for composition and row history',async()=>{
 const period={complete:true,periods:30,missing:0,total:{amount:20,count:1,average:20}};
 const item={key:'work:Work',name:'Work',kind:'work',amount:{base:1,current:2,delta:1,percent:100}};
 const reads=[],history=[],f=fixture({
  nativeAnalysisOrganizations:async()=>[{org:'A',...period}],
  nativeAnalysisCompare:async()=>({summary:{before:period,after:period}}),
  unifiedComparisonWindow:async()=>({rows:[item],total_rows:1}),
  nativeWorkTimeline:async(...args)=>{history.push(args);return {granularity:'period',points:[]};},
  nativeCompositionPage:async(req,before,mode)=>{reads.push([mode,req,before]);return {baseComplete:true,currentComplete:true,sharesValid:true,total_rows:0,rows:[],check:{reconciled:true}};}
 });
 f.UI.state.selectedMetrics=new Set(['amount']);f.window.referenceDetail={active:'structurePanel'};
 f.publish({displayType:'month',from:'2026-11-01',to:'2026-11-30',baseFrom:'2026-10-02',baseTo:'2026-10-31'});
 f.nodes.get('structurePanel').open=true;f.nodes.get('structurePanel').dispatch('toggle');await settle();
 f.window.referenceDetail.active='comparison';f.nodes.get('contributions').open=true;
 await f.window.unifiedExtended.openComparison();await settle();
 for(const [mode,req,before] of reads){
  assert.equal(req.displayType,'day',mode);
  assert.deepEqual([req.query.from,req.query.to,before.from,before.to],['2026-11-01','2026-11-30','2026-10-02','2026-10-31']);
 }
 assert.deepEqual(reads.map(r=>r[0]),['structure','contributions']);
 const link=f.tables.find(t=>t.root==='comparisonTable').rows[1][0];link.dispatch('click');await settle();
 assert.deepEqual([history[0][2],history[0][3],history[0][8]],['2026-10-02','2026-11-30','day']);
});
test('overview comparison shortcut uses the same section router as the comparison tab',()=>{
 const calls=[],f=fixture();f.window.referenceDetail={active:'overviewPanel'};
 f.window.referenceDesktop={openPanel:(...args)=>calls.push(args)};
 const shortcut=f.nodes.get('overviewShowComparison');shortcut.dispatch('click');
 assert.deepEqual(calls,[['comparison',shortcut]]);
});

test('usual level captions expand calendar keys into the full sampled dates',async()=>{
 for(const [source,kind,from,to,first,last,baseEnd,currentEnd] of [
  ['imported','week','2024-01-10','2024-01-10','2023-12-13','2024-01-03','09.01.2024','16.01.2024'],
  ['imported','month','2024-02-01','2024-02-01','2023-10-01','2024-01-01','31.01.2024','29.02.2024'],
  ['daily','month','2024-02-01','2024-02-29','2023-10-01','2024-01-01','31.01.2024','29.02.2024']
 ]){
  const f=fixture({unifiedPatternPage:async()=>({rows:[],total_rows:0,used:4,expected:4,previous_used:4,current_complete:true,baseline_from:first,baseline_to:last,current_from:from,current_to:from})});
  f.UI.range=()=>({type:source==='daily'?'day':kind,displayType:kind,source,from,to});f.publish({displayType:kind,source,type:source==='daily'?'day':kind,from,to});
  f.nodes.get('usualPeriod').value=from;f.nodes.get('usualPanel').open=true;f.nodes.get('usualPanel').dispatch('toggle');await settle();
  const caption=f.tables.find(t=>t.root==='usualResults').caption;
  assert.ok(caption.includes(baseEnd)&&caption.endsWith(currentEnd),caption);
 }
});

test('department comparison uses identity-keyed rows for an entire catalog page and matches both organizations',async()=>{
 const totals=[{org:'A',complete:true,missing:0,total:{amount:150}},{org:'B',complete:true,missing:0,total:{amount:260}}];
 const rows=[['group:g','Ремонт (группа)','group',100,200],['work:Ремонт','Ремонт (работа)','work',40,60],['work:Уборка','Уборка','work',10,0]];
 const calls=[];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>totals,
  unifiedNameOptionsAtPage:async()=>({rows:[{name:'Уборка',key:'work:Уборка',kind:'work'}],total_rows:1}),
  unifiedOrganizationWorkWindow:async(req,index,limit)=>{calls.push([req,index,limit]);return {rows:rows.map(([key,name,kind,a,b])=>({entry:{key,name,kind},rows:[{org:'A',total:{amount:a}},{org:'B',total:{amount:b}}]})),total_rows:3};},
  nativeOrganizationTrends:async()=>({granularity:'period',series:[]}),
  nativeWorkTimeline:async(...args)=>{calls.push(args);return {points:[]};}
 });
 f.window.referenceDetail={active:'comparison'};
 f.publish({organizations:['A','B'],selectedNames:[],selectedKeys:[],nameRows:[],allNames:true});
 f.nodes.get('comparisonMode').value='organizations';f.nodes.get('comparisonBaseOrg').value='A';
 await f.window.unifiedExtended.openComparison();
 const table=f.tables.find(t=>t.metric==='amount');assert.ok(table);
 assert.deepEqual(Array.from(table.rows.slice(1),r=>[r[0].textContent,r[1],r[2]]),rows.map(r=>[r[1],String(r[3]),String(r[4])]));
 assert.deepEqual(Array.from(table.options.csvRows.slice(1),r=>Array.from(r).slice(0,3)),rows.map(r=>[r[1],r[3],r[4]]));
 assert.equal(calls[0][1],0);assert.equal(calls[0][2],50);
 table.rows[1][0].dispatch('click');await settle();assert.equal(calls[1][6],'group:g');
});

test('opening a group uses the same qualified label in the history title and chart',async()=>{
 const metric={base:10,current:25,delta:15,percent:150};
 const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:{complete:true,total:{amount:10}},after:{complete:true,total:{amount:25}}}}),
  unifiedComparisonWindow:async()=>({rows:[{name:'Ремонт',kind:'group',key:'group:g',amount:metric}],total_rows:1}),
  nativeWorkTimeline:async()=>({granularity:'period',points:[{start:'2024-01-02',missing:0,total:{amount:25}}]})
 });
 const labels=[];f.UI.drawChart=(_p,_o,_m,_root,options)=>labels.push(options.label());
 f.publish();await f.window.unifiedExtended.openComparison();
 const button=f.tables.find(t=>t.metric==='amount').rows[1][0];
 assert.equal(button.textContent,'Ремонт (группа)');button.dispatch('click');await settle();
 assert.equal(f.nodes.get('analyticsDetailTitle').textContent,button.textContent);
 assert.ok(labels.length);assert.ok(labels.every(label=>label===button.textContent));
});

test('composition chart distinguishes a work and matching group just like its table',async()=>{
 const rows=['work','group'].map((kind,i)=>({key:kind+':r',name:'Ремонт',kind,base:5,current:i+1}));
 const f=fixture({nativeCompositionPage:async()=>({rows,total:3,currentComplete:true,baseComplete:true,sharesValid:true})});
 let labels;
 f.UI.drawChart=(_p,_o,_m,_root,options)=>{labels=options.categories.map(options.label);};
 f.publish({allNames:true});f.nodes.get('structurePanel').open=true;f.nodes.get('structurePanel').dispatch('toggle');await settle();
 const names=f.tables.find(t=>t.root==='structureResults').rows.map(row=>row[0].textContent);
 assert.deepEqual(Array.from(labels),Array.from(names));assert.equal(new Set(labels).size,2);
});

test('drilldown on the available side of a partial comparison opens the range printed above it',async()=>{
 const history=[];
 const full={complete:true,periods:1,missing:0,total:{count:5,amount:10,average:2}},missing={...full,complete:false,missing:1,total:{count:null,amount:null,average:null}};
 const fx=fixture({
  nativeAnalysisOrganizations:async(_rev,_type,from)=>[{org:'A',...(from==='2024-01-01'?full:missing)}],
  nativeAnalysisCompare:async()=>({summary:{before:full,after:missing}}),
  unifiedComparisonWindow:async()=>({total_rows:1,rows:[{key:'work:W',name:'W',kind:'work',available:{amount:10,count:5}}]}),
  nativeWorkTimeline:async(...a)=>{history.push(a);return {granularity:'period',points:[]};}
 });
 fx.window.referenceDetail={active:'comparison'};fx.publish({from:'2024-01-10',to:'2024-01-10',baseFrom:'2024-01-01',baseTo:'2024-01-01',allNames:true});
 await fx.window.unifiedExtended.openComparison();
 const table=fx.tables.find(t=>t.root==='comparisonTable');assert.equal(table.caption,'01.01.2024');
 table.rows[1][0].dispatch('click');await settle();
 assert.deepEqual(history[0].slice(2,4),['2024-01-01','2024-01-01']);
});

test('detailed comparison displays rounded changes neutrally while CSV retains their original precision',async()=>{
 const total=amount=>({complete:true,total:{count:1,amount,average:amount}});
 const delta=-0.002,measure={base:10,current:10+delta,delta,percent:-0.02};
 const f=fixture({
  nativeAnalysisCompare:async()=>({summary:{before:total(10),after:total(10+delta)}}),
  unifiedComparisonWindow:async()=>({rows:[{key:'work:r',name:'Ремонт',kind:'work',amount:measure}],total_rows:1})
 });
 f.UI.fmt=(n,m)=>Number.isFinite(n)?n.toLocaleString('ru-RU',{minimumFractionDigits:2,maximumFractionDigits:2}):'—';
 f.publish();await f.window.unifiedExtended.openComparison();
 const table=f.tables.find(t=>t.root==='comparisonTable'&&t.metric==='amount');
 assert.equal(table.rows[1][3],'0,00');assert.equal(table.rows[1][4],'≈ 0%');
 assert.equal(table.options.csvRows[1][3],delta);assert.equal(table.options.csvRows[1][4],-0.02);
});

test('adjacent change table and leader keep a rounded-zero change neutral and identify its group',async()=>{
 const data=page('Ремонт'),row=data.rows[0];
 row.kind='group';row.status='existing';row.amount={base:10,current:10.003,delta:.003,percent:.03};
 data.summary.top_amount=row;
 const f=fixture({unifiedChangesPage:async()=>data});
 f.UI.fmt=(n,metric)=>Number.isFinite(n)?n.toFixed(metric==='amount'?2:3).replace('.',','):'—';
 f.publish();f.view('changes');await settle();
 const table=f.tables.find(item=>item.root==='changesResults');
 const direction=table.rows[1].at(-1);
 assert.equal(direction.textContent,'≈ без изменений');assert.equal(direction.className,'');
 const leader=f.nodes.get('changesSummary').children.at(-1);
 assert.match(leader.textContent,/Ремонт \(группа\)/);assert.equal(leader.children[1].className,'');
});

test('contribution detail respects the selected work scope used by its parent comparison',async()=>{
 const contributions=[],period={complete:true,periods:1,missing:0,total:{count:3,amount:30,average:10}};
 const f=fixture({
  nativeAnalysisOrganizations:async()=>[{org:'A',...period}],
  nativeAnalysisCompare:async()=>({summary:{before:period,after:period}}),
  unifiedComparisonWindow:async()=>({rows:[],total_rows:0}),
  nativeCompositionPageAtPage:async request=>{
   contributions.push(request);
   return {rows:[],total_rows:0,baseComplete:true,currentComplete:true,check:{sum:0,delta:0,reconciled:true}};
  }
 });
 f.window.referenceDetail={active:'comparison',currentComparison:()=>({
  before:{from:'2024-01-01',to:'2024-01-01'},after:{from:'2024-01-02',to:'2024-01-02'}
 })};
 f.publish();f.nodes.get('contributions').open=true;
 await f.window.unifiedExtended.openComparison();await settle();
 assert.ok(contributions.length);
 assert.deepEqual(Array.from(contributions[0].selectedNames),['Work']);
 assert.match(f.nodes.get('contributionCheck').textContent,/отмеченные работы/);
 f.publish({allNames:true,selectedNames:[],selectedKeys:[]});
 await f.window.unifiedExtended.openComparison();await settle();
 assert.deepEqual(Array.from(contributions.at(-1).selectedNames),[]);
 assert.match(f.nodes.get('contributionCheck').textContent,/все работы/);
});

test('department comparison loads a line chart automatically only for a range',async()=>{
 let trendReads=0;
 const values=[{org:'A',complete:true,missing:0,total:{count:5,amount:10,average:2}}];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>values,
  unifiedOrganizationWorkWindow:async()=>({rows:[],total_rows:0}),
  nativeOrganizationTrends:async()=>{
   trendReads++;
   return {granularity:'period',series:[{organization:'A',points:[{start:'2024-01-02',missing:0,total:{count:5,amount:10,average:2}}]}]};
  }
 });
 f.publish({allNames:true,selectedNames:[],selectedKeys:[]});
 f.nodes.get('comparisonMode').value='organizations';f.nodes.get('comparisonBaseOrg').value='A';
 await f.window.unifiedExtended.openComparison();await settle();
 assert.equal(trendReads,0);
 const chart=f.nodes.get('comparisonDynamics');
 assert.equal(chart.hidden,true);
 f.publish({overviewMode:'range'});
 await f.window.unifiedExtended.openComparison();
 assert.equal(trendReads,1);
 assert.equal(chart.hidden,false);
 assert.match(f.nodes.get('comparisonDynamicsPlots').textContent,/один период/);
});

test('switching comparison type clears the previous results before SQL completes',async()=>{
 const pending=deferred();
 const total={count:2,amount:20,average:10};
 const f=fixture({
  nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,total}],
  nativeAnalysisCompare:()=>pending.promise,
  unifiedComparisonWindow:async()=>({rows:[],total_rows:0})
 });
 f.publish();
 const mode=f.nodes.get('comparisonMode'),table=f.nodes.get('comparisonTable'),charts=f.nodes.get('comparisonCharts');
 mode.value='organizations';mode.dispatch('change');await settle();
 assert.match(table.textContent,/20/);
 assert.ok(charts.children.length);
 mode.value='periods';mode.dispatch('change');await settle();
 assert.equal(table.textContent,'Загрузка…');
 assert.equal(charts.textContent,'Загрузка…');
 assert.equal(table.attributes['aria-busy'],'true');
 const full={complete:true,periods:1,missing:0,total,available_reports:1,available_total:total};
 pending.resolve({summary:{before:full,after:full}});
 await settle();
 assert.equal(table.attributes['aria-busy'],'false');
 assert.match(table.textContent,/20/);
});

test('comparison shows differences for both observed partial intervals with one coverage warning',async()=>{
 const unknown={count:null,amount:null,average:null};
 const summary=amount=>({complete:false,periods:2,missing:1,available_reports:1,total:unknown,available_total:{amount,count:amount/10,average:10}});
 const f=fixture({
  nativeAnalysisOrganizations:async(_rev,_kind,_from,_to,orgs)=>orgs.map(org=>({org,complete:false,missing:1,total:unknown})),
  nativeAnalysisCompare:async(...args)=>{assert.deepEqual([...args[6]],['A','B']);return {summary:{before:summary(20),after:summary(30)}};},
  unifiedComparisonWindow:async()=>({rows:[{key:'work:W',name:'W',kind:'work',amount:{delta:10,percent:50},observed_before:{amount:20},observed_after:{amount:30}}],total_rows:1})
 });
 f.window.referenceDetail={active:'comparison'};
 f.publish({organizations:['A','B']});
 await f.window.unifiedExtended.openComparison();
 const table=f.tables.find(t=>t.root==='comparisonTable'&&t.metric==='amount');
 assert.ok(table);assert.deepEqual(Array.from(table.options.csvRows[0].slice(1)),[20,30,10,50]);
 assert.deepEqual(Array.from(table.options.csvRows[1]),['W',20,30,10,50]);
 const coverage=f.nodes.get('comparisonCoverage');
 assert.match(coverage.textContent,/разница.*по доступным отч[её]там/i);
 assert.equal(coverage.children.length,1);
});

test('department comparison graphs one selected metric, preserving zero and missing values',async()=>{
 const plots=[],requests=[];
 const empty={count:null,amount:null,average:null};
 const point=(start,amount)=>({start,missing:Number(amount===null),total:{count:amount,amount,average:null}});
 const trends=[
  {organization:'A',points:[point('2024-01',0),point('2024-02',9)]},
  {organization:'B',points:[point('2024-01',null),point('2024-02',3)]}
 ];
 const f=fixture({
  nativeAnalysisOrganizations:async()=>[
   {org:'A',total:{amount:0},complete:true,missing:0},
   {org:'B',total:empty,available_total:{amount:3},complete:false,missing:1}
  ],
  nativeOrganizationTrends:async(...args)=>{requests.push(args);return {granularity:'month',series:trends};}
 });
 f.UI.drawChart=(...args)=>plots.push(args);
 f.UI.state.selectedMetrics=new Set(['amount']);
 f.window.referenceDetail={active:'comparison'};
 f.publish({organizations:['A','B','C'],overviewMode:'range',displayType:'month',to:'2024-02-29'});
 f.nodes.get('comparisonMode').value='organizations';
 await f.window.unifiedExtended.openComparison();
 const [totals,trend]=plots;
 assert.deepEqual(Array.from(requests[0].slice(2,4)),['2024-01-01','2024-02-29']);
 assert.deepEqual(Array.from(requests[0][4]),['A','B','C']);
 assert.equal(requests[0][6],'month');
 assert.deepEqual(Array.from(requests[0][7]),['Work']);
 assert.equal(totals[2],'amount');
 assert.equal(totals[4].hideZeroRankingRows,false);
 assert.equal(totals[4].data[0].get('A').amount,0);
 assert.equal(totals[4].data[0].get('B').amount,3);
 assert.equal(totals[4].data[0].get('C').amount,null);
 assert.equal(trend[2],'amount');
 assert.equal(trend[4].chart,'line');
 assert.deepEqual(Array.from(trend[0],period=>period.end),['2024-01-31','2024-02-29']);
 assert.deepEqual(Array.from(trend[4].categories),['A','B','C']);
 assert.equal(trend[4].data[0].get('A').amount,0);
 assert.equal(trend[4].data[0].get('B').amount,null);
 assert.equal(trend[4].data[0].get('C').amount,null);
 assert.equal(trend[4].data[1].get('B').amount,3);
 assert.match(trend[4].pointNotice(0,['A','B','C']),/B, C/);
 assert.equal(trend[4].pointNotice(0,['A']),'');
});

test('deviations explain missing reports and insufficient historical windows',async()=>{
 const f=fixture({unifiedPatternPageAtPage:async()=>({
  rows:[],total_rows:0,used:1,expected:4,previous_used:1,current_complete:false,
  baseline_from:'2023-12-24',baseline_to:'2023-12-31',current_from:'2024-01-01',current_to:'2024-01-02'
 })});
 f.window.referenceDetail={active:'usualPanel'};
 f.publish({baseFrom:'2023-12-30',baseTo:'2023-12-31'});
 f.nodes.get('usualPanel').open=true;f.nodes.get('usualPanel').dispatch('toggle');await settle();
 const message=f.nodes.get('usualCoverage').textContent;
 assert.match(message,/прочерк.*неполном интервале/i);
 assert.match(message,/минимум два полных исторических окна/i);
});

test('one metric and one work can be compared across three selected departments',async()=>{
 const calls=[],charts=[];
 const f=fixture({nativeAnalysisOrganizations:async(...args)=>{
  calls.push(args);
  return ['A','B','C'].map((org,i)=>({org,complete:true,missing:0,total:{count:i+1,amount:(i+1)*10,average:10}}));
 }});
 f.UI.drawChart=(...args)=>charts.push(args);
 f.window.referenceDetail={active:'comparison'};
 f.UI.state.selectedMetrics=new Set(['count']);
 f.publish({organizations:['A','B','C']});
 f.nodes.get('comparisonMode').value='organizations';
 f.nodes.get('comparisonMode').dispatch('change');await settle();
 assert.deepEqual(Array.from(calls.at(-1)[4]),['A','B','C']);
 assert.deepEqual(Array.from(calls.at(-1)[6]),['Work']);
 assert.deepEqual(f.tables.map(table=>table.metric),['count']);
 assert.deepEqual(Array.from(f.tables[0].options.csvRows[1].slice(1)),[1,2,1,100,3,2,200]);
 assert.equal(charts.length,1);
 assert.deepEqual(Array.from(charts[0][4].categories),['A','B','C']);
 assert.match(f.nodes.get('comparisonHelp').textContent,/Показатели сравнения/);
});

test('department comparison opens work history with the locally selected average',async()=>{
 const totals={count:2,amount:-10,average:-5};
 const f=fixture({
  nativeAnalysisOrganizations:async()=>['A','B'].map(org=>({org,complete:true,missing:0,total:totals})),
  nativeWorkTimeline:async()=>({granularity:'period',points:[{start:'2024-01-01',missing:0,total:totals}]})
 });
 const history=[];
 f.UI.drawChart=(_periods,_orgs,metric,_root,options)=>{
  if(options.categories[0]==='work:Work')history.push([metric,options.data[0].get('work:Work')[metric]]);
 };
 f.window.referenceDetail={active:'comparison'};
 f.UI.state.selectedMetrics=new Set(['count']);
 f.publish({organizations:['A','B']});
 f.nodes.get('comparisonMode').value='organizations';
 for(const row of f.nodes.get('comparisonMetrics').children){
  const input=row.children[0];
  if(input.value==='amount')continue;
  input.checked=input.value==='average';input.dispatch('change');
 }
 await settle();
 const table=f.tables.findLast(table=>table.metric==='average');
 assert.ok(table);
 assert.deepEqual(Array.from(table.options.csvRows[1].slice(1)),[-5,-5,0,0]);
 table.rows[1][0].dispatch('click');await settle();
 assert.deepEqual(history,[['average',-5]]);
});

test('department comparison uses observed totals and shows one coverage notice',async()=>{
 const empty={count:null,amount:null,average:null};
 const f=fixture({nativeAnalysisOrganizations:async()=>[
  {org:'A',complete:false,missing:2,total:empty,available_total:{count:5,amount:20,average:4}},
  {org:'B',complete:false,missing:1,total:empty,available_total:{count:8,amount:30,average:3.75}},
  {org:'C',complete:false,missing:1,total:empty,available_total:{count:10,amount:35,average:null},amount_missing_rows:true},
  {org:'D',complete:false,missing:3,total:empty,available_total:empty}
 ]});
 f.window.referenceDetail={active:'comparison'};
 f.UI.state.selectedMetrics=new Set(['amount','count','average']);
 f.publish({organizations:['A','B','C','D']});
 f.nodes.get('comparisonMode').value='organizations';
 f.nodes.get('comparisonBaseOrg').value='A';
 await f.window.unifiedExtended.openComparison();
 const raw=(metric)=>Array.from(f.tables.find(t=>t.root==='comparisonTable'&&t.metric===metric).options.csvRows[0]).slice(1);
 assert.deepEqual(raw('amount'),[20,30,10,50,35,null,null,null,null,null]);
 assert.deepEqual(raw('count'),[5,8,3,60,10,5,100,null,null,null]);
 assert.deepEqual(raw('average'),[4,3.75,-0.25,-6.25,null,null,null,null,null,null]);
 const coverage=f.nodes.get('comparisonCoverage');
 assert.equal(coverage.children.length,1);
 assert.match(coverage.textContent,/Полные отчёты у 0 из 4/);
 assert.doesNotMatch(coverage.textContent,/Нет сопоставимых подразделений/);
 const summary=f.nodes.get('kpis').textContent;
 assert.match(summary,/Доля суммыСумма \/ количество \(объём\)/);
 assert.match(summary,/A20523,5%4B30835,3%3\.75C351041,2%—D————/);
});
