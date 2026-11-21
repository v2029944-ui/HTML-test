const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
class Node{
 constructor(tag='div',text=''){this.tagName=tag.toUpperCase();this.children=[];this._text=text;this.dataset={};this.attrs={};this.listeners={};this.hidden=false;this.disabled=false;this.value='';this.className='';this.style={};this.classList={toggle:()=>{}};}
 append(...xs){for(const x of xs){if(x&&typeof x==='object')x.parentElement=this;this.children.push(x);}}replaceChildren(...xs){this.children=[];this._text='';this.append(...xs);}set textContent(x){this._text=String(x);this.children=[];}get textContent(){return this._text+this.children.map(x=>typeof x==='string'?x:x.textContent).join('');}
 setAttribute(k,v){this.attrs[k]=String(v);if(k==='id')this.id=v;if(k.startsWith('data-'))this.dataset[k.slice(5).replace(/-([a-z])/g,(_,x)=>x.toUpperCase())]=String(v);}getAttribute(k){return this.attrs[k]??null;}
 addEventListener(k,fn){(this.listeners[k]??=[]).push(fn);}async fire(k){for(const fn of this.listeners[k]||[])await fn({target:this,preventDefault(){}});}click(){return this.disabled?Promise.resolve():this.fire('click');}remove(){if(this.parentElement)this.parentElement.children=this.parentElement.children.filter(x=>x!==this);}focus(){this.focused=true;}get isConnected(){return this.tagName==='BODY'||Boolean(this.parentElement?.isConnected);}scrollIntoView(){}
 querySelectorAll(q){const parts=q.trim().split(/\s+/);const match=(n,s)=>{if(!(n instanceof Node))return false;const attr=s.match(/\[([^=\]]+)(?:=["']?([^\]"']+)["']?)?\]/);if(attr){const value=n.getAttribute(attr[1]);if(value===null||attr[2]!==undefined&&value!==attr[2])return false;s=s.replace(attr[0],'');}const id=s.match(/#([\w-]+)/);if(id&&n.id!==id[1])return false;for(const c of s.matchAll(/\.([\w-]+)/g))if(!n.className.split(/\s+/).includes(c[1]))return false;const tag=s.match(/^[a-z][a-z0-9]*/i);return !tag||n.tagName===tag[0].toUpperCase();};const found=[];const visit=n=>{for(const child of n.children){if(!(child instanceof Node))continue;if(match(child,parts.at(-1))){let p=child.parentElement,i=parts.length-2;for(;i>=0;i--){while(p&&!match(p,parts[i]))p=p.parentElement;if(!p)break;p=p.parentElement;}if(i<0)found.push(child);}visit(child);}};visit(this);return found;}
 get firstElementChild(){return this.children.find(x=>x instanceof Node)||null;}querySelector(q){return this.querySelectorAll(q)[0]||null;}
}
const flush=()=>new Promise(r=>setImmediate(r));
function deferred(){let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};}
function fixture(overrides={}){
 const body=new Node('body');const add=(root,tag,cls='',text='')=>{const n=new Node(tag,text);n.className=cls;root.append(n);return n;};
 const s1=add(body,'section');s1.id='s1';const s2=add(body,'section');s2.id='s2';add(s1,'p','report-range');add(s2,'p','report-range');
 for(const s of [s1,s2]){const note=add(s,'div','quality-notice');add(note,'span');add(note,'button','','Проверить отчёты');add(s,'button','','Печать / PDF');}
 const grid=add(s1,'section','metrics-grid');for(let i=0;i<3;i++){const c=add(grid,'div','metric-card');for(const cls of ['metric-name','metric-value','metric-delta'])add(c,'div',cls);}
 const table=add(s1,'table');table.setAttribute('aria-label','Распределение по подразделениям');add(table,'thead');add(table,'tbody');const data=add(s1,'div','data-card');add(data,'button','','Скачать CSV');
 const insight=add(s1,'div','insight-card');add(insight,'h2');add(insight,'div');add(insight,'button','','Открыть полный анализ изменений');
 for(let i=0;i<4;i++){const n=add(s2,'button','kpi');add(n,'div');const v=add(n,'div','num');v.setAttribute('data-count','');add(n,'div');}
 add(s2,'button','','Как читать результаты?');const sensitivity=add(s2,'div','filter-sensitivity');add(sensitivity,'button','filter-threshold');add(sensitivity,'span');const leaders=add(s2,'div','leaders');for(let i=0;i<2;i++){const c=add(leaders,'div','card');add(c,'div','leader-name');add(c,'div','leader-value');add(c,'div');}
 const input=add(s2,'input');input.setAttribute('placeholder','Название работы');const sort=add(s2,'select');sort.setAttribute('data-sort','');sort.value='delta';const metric=add(s2,'strong');metric.setAttribute('data-current-change-metric','');const heading=add(s2,'div','changes-heading');add(heading,'h2');
 add(s2,'p','match-note');const ct=add(add(s2,'div','table-viewport'),'table','changes-table');const head=add(ct,'thead'),tr=add(head,'tr');for(let i=0;i<6;i++)add(tr,'th');add(ct,'tbody');add(s2,'div','table-empty');add(s2,'span','pager-footer');const exp=add(s2,'button','','Скачать CSV');exp.setAttribute('data-export-changes','');
 const total={count:10,amount:100,average:10},full={complete:true,periods:1,missing:0,total,available_reports:1,available_total:total,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const calls=[],exports=[],notices=[],downloads=[];const api={nativeAnalysisCompare:async(...args)=>{calls.push(args);return {summary:{before:full,after:full},divisions:args[6].map(organization=>({organization,before:total,after:total})),top_amount:[],top_count:[]};},nativeAnalysisOrganizations:async(...args)=>args[4].map(org=>({org,complete:true,missing:0,total})),nativeAnalysisRange:async()=>full,referenceChanges:async()=>({rows:[{name:'Work',org:'A',before:1,after:2,delta:1,percent:100}],total:1,searched:1,significant:1,counts:{up:1,down:0,new:0,gone:0},leader:null,comparable:['A']}),chooseCsvPath:async()=>'/tmp/changes.csv',referenceChangesCsv:async(...args)=>exports.push(args),referenceSummaryCsv:async(...args)=>exports.push(args),...overrides};
 const sheet=add(body,'dialog');sheet.id='sheet';sheet.open=false;sheet.close=async()=>{sheet.open=false;await sheet.fire('close');};
 const document={body,createElementNS:(_namespace,tag)=>new Node(tag),createElement:tag=>new Node(tag),querySelector:q=>body.querySelector(q),querySelectorAll:q=>body.querySelectorAll(q),getElementById:id=>body.querySelector('#'+id)};
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),desktopAPI:api,referenceDesktop:{dialog:()=>{sheet.open=true;const content=new Node('div');content.append(new Node('h2'));sheet.replaceChildren(content);return content;},navigate:id=>window.referenceResults.setScreen(id),notice:text=>notices.push(text)},referenceFilters:{setMetric:m=>{scope.metric=m;window.referenceResults.refresh(scope);}},unifiedCoverage:{render:async()=>{}},print(){}};
 const scope={revision:1,type:'day',from:'2024-01-02',to:'2024-01-02',baseFrom:'2024-01-01',baseTo:'2024-01-01',organizations:['A'],metric:'amount',source:'daily',displayType:'day'};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),{window});
 const file=path.join(__dirname,'../reference-results.js');if(fs.existsSync(file))vm.runInNewContext(fs.readFileSync(file,'utf8'),{window,document,Intl,Number,Date,Set,Map,Promise,setTimeout,clearTimeout,console,Blob,URL:{createObjectURL:blob=>{downloads.push(blob);return 'blob:test';},revokeObjectURL(){}}});
 return {window,document,api,scope,s1,s2,calls,exports,notices,downloads,full,total};
}
test('summary presents observed partial totals separately from the comparable sample',async()=>{
 const f=fixture();assert.ok(f.window.referenceResults,'SQL reference results controller must be present');
 const incomplete={...f.full,complete:false,missing:1,single_missing:{org:'B',start:'2024-01-02'},total:{count:null,amount:null,average:null},available_total:{count:7,amount:70,average:10}};
 f.api.nativeAnalysisCompare=async(...args)=>{f.calls.push(args);return {summary:{before:f.full,after:args[6].length===2?incomplete:f.full},divisions:args[6].map(organization=>({organization,before:f.total,after:organization==='B'?{count:null,amount:null,average:null}:f.total})),top_amount:[],top_count:[]};};
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,missing:0,total:f.total},{org:'B',complete:false,missing:1,total:{count:null,amount:null,average:null}}];f.api.nativeAnalysisRange=async()=>incomplete;f.scope.organizations=['A','B'];
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 assert.match(f.s1.querySelector('.metric-value').textContent,/70/);assert.match(f.s1.querySelector('.quality-notice').textContent,/1 из 2/);assert.ok(f.calls.some(x=>x[6].length===1&&x[6][0]==='A'));assert.doesNotMatch(f.s1.textContent,/₽|ед\./);
 assert.match(f.s1.querySelector('.quality-notice').textContent,/B · 02\.01\.2024: нет пригодного отчёта/);
 assert.match(f.s1.querySelector('.report-range').textContent,/2 янв\. 2024/);
 for(const screen of [f.s1,f.s2]) assert.match(screen.querySelector('.report-range').children[1].textContent,/^Сравнение с /);
 await f.s1.querySelector('.metric-sample').click();const cells=f.document.querySelectorAll('.sample-values tbody tr');
 assert.deepEqual(cells.map(row=>row.children.map(c=>c.textContent)),[['Сумма','100,00','100,00'],['Количество / объём','10,000','10,000']]);
 incomplete.missing=2;await f.window.referenceResults.refresh(f.scope);assert.doesNotMatch(f.s1.querySelector('.quality-notice').textContent,/02\.01\.2024/);
});
test('null refresh rejects stale summary and changes replies',async()=>{
 const delayed=deferred(),f=fixture({nativeAnalysisCompare:()=>delayed.promise});assert.ok(f.window.referenceResults);f.window.referenceResults.init();const pending=f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.refresh(null);delayed.resolve({summary:{before:f.full,after:f.full},divisions:[],top_amount:[],top_count:[]});await pending;assert.doesNotMatch(f.s1.querySelector('.metric-value').textContent,/100/);
});
test('changes categories search pagination and full export carry the captured SQL request',async()=>{
 const requests=[],f=fixture({referenceChanges:async request=>{requests.push(request);return {rows:[{name:'=Literal Work',org:'A',before:0,after:0,delta:0,percent:null}],total:25,searched:30,significant:25,counts:{up:2,down:3,new:4,gone:5},leader:null,comparable:['A']};}});assert.ok(f.window.referenceResults);f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');await flush();
 assert.match(f.s2.querySelector('.match-note').textContent,/Значимые позиции · 25Показать все 30/);assert.match(f.s2.querySelector('.filter-sensitivity span').textContent,/объём — 5, сумма — 1000/);assert.equal(f.s2.querySelectorAll('.changes-table tbody tr').length,1);assert.equal(f.s2.querySelectorAll('.changes-table .mobile-position').length,1,'mobile layout needs its visible value card');assert.match(f.s2.querySelector('.changes-table tbody').textContent,/0,00|0/);await f.s2.querySelector('.table-viewport').scrollWindow.go(2);assert.equal(requests.at(-1).page,2);await f.s2.querySelectorAll('.kpi')[2].click();await flush();assert.equal(requests.at(-1).kind,'new');assert.equal(requests.at(-1).page,0);const input=f.s2.querySelector('input');input.value='Literal';await input.fire('input');await new Promise(r=>setTimeout(r,220));assert.equal(requests.at(-1).search,'Literal');await f.s2.querySelector('[data-export-changes]').click();assert.equal(f.exports.length,1);assert.equal(f.exports[0][0].search,'Literal');assert.equal(f.exports[0][1],'/tmp/changes.csv');
});

test('summary CSV uses native save and rejects a stale scope while its chooser is open',async()=>{
 const f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.s1.querySelector('.data-card button').click();assert.equal(f.downloads.length,0);assert.equal(f.exports.length,1);assert.deepEqual(Array.from(f.exports[0][0].organizations),['A']);assert.equal(f.exports[0][1],'/tmp/changes.csv');
 const wait=deferred();f.api.chooseCsvPath=()=>wait.promise;const pending=f.s1.querySelector('.data-card button').click();await f.window.referenceResults.refresh(null);wait.resolve('/tmp/stale.csv');await pending;assert.equal(f.exports.length,1);
});
test('null and empty selections are informative instead of loading forever and print styles are cleared',async()=>{
 const f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(null);assert.doesNotMatch(f.s1.textContent,/Загрузка/);await f.window.referenceResults.refresh({...f.scope,organizations:[]});assert.doesNotMatch(f.s1.textContent,/Загрузка/);assert.match(f.s1.textContent,/Выберите периоды/);assert.doesNotMatch(f.s2.textContent,/Загрузка/);assert.match(f.s2.querySelector('.table-empty').textContent,/Выберите периоды/);assert.equal(f.s2.querySelector('[data-export-changes]').disabled,true);
 await f.window.referenceResults.refresh(f.scope);const print=f.s1.querySelectorAll('button').find(n=>n.textContent.includes('Печать'));await print.click();assert.equal(f.document.body.dataset.referencePrint,undefined);
});

test('summary orders observed amounts with null last and warns for either missing interval with focus return',async()=>{
 const f=fixture(),tot=amount=>({count:1,amount,average:amount});f.scope.organizations=['A','B','C','D'];
 f.api.nativeAnalysisCompare=async(...args)=>({summary:{before:f.full,after:f.full},divisions:args[6].map(organization=>({organization,before:organization==='B'?{count:null,amount:null,average:null}:tot(1),after:organization==='C'?{count:null,amount:null,average:null}:tot(1)})),top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,total:tot(50)},{org:'B',complete:true,total:tot(100)},{org:'C',complete:false,total:{count:null,amount:null,average:null}},{org:'D',complete:true,total:tot(-5)}];f.api.nativeAnalysisRange=async()=>({...f.full,available_reports:0,available_total:{count:null,amount:null,average:null}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);const rows=f.s1.querySelectorAll('tbody tr');assert.deepEqual(rows.map(row=>row.children[0].textContent),['B','A','D','C (частичные данные)']);
 const warnings=f.s1.querySelectorAll('.org-warning');assert.equal(warnings.length,2);assert.match(warnings[0].getAttribute('aria-label'),/^B/);await warnings[0].click();assert.match(f.document.getElementById('sheet').textContent,/неполный базовый интервал/);await f.document.getElementById('sheet').close();assert.equal(warnings[0].focused,true);await warnings[0].click();await f.window.referenceResults.refresh(null);assert.equal(f.document.getElementById('sheet').open,false,'stale sample modal must close on refresh');
});
test('KPI direction follows the displayed precision and preserves measurable differences',async()=>{
 const f=fixture();
 const summary={before:f.full,after:f.full};
 f.api.nativeAnalysisCompare=async()=>({summary,divisions:[{organization:'A',before:f.total,after:f.total}],top_amount:[],top_count:[]});
 const marks=()=>f.s1.querySelectorAll('.metric-delta').map(n=>[n.firstElementChild.textContent,n.firstElementChild.className]);
 f.window.referenceResults.init();
 Object.assign(summary,{amount:{delta:0,percent:0},count:{delta:0.00049,percent:1},average:{delta:-0.004,percent:-0.0002}});
 await f.window.referenceResults.refresh(f.scope);
 assert.deepEqual(marks(),[['Без изменений','neutral'],['≈ без изменений','neutral'],['≈ без изменений','neutral']]);
 Object.assign(summary,{amount:{delta:-0.005,percent:-0.01},count:{delta:0.0005,percent:0.05},average:{delta:-1e-10,percent:-1e-9}});
 await f.window.referenceResults.refresh(f.scope);
 assert.deepEqual(marks(),[['▼ −0,01 (< 0,1%)','down'],['▲ +0,001 (+0,1%)','up'],['Без изменений','neutral']]);
});
test('changes table and leader display subprecision values without a signed zero',async()=>{
 const row={name:'Work',org:'A',before:100,after:99.996,delta:-0.004,percent:-0.004};
 const f=fixture({referenceChanges:async()=>({rows:[row],total:1,searched:1,significant:1,counts:{up:0,down:0,new:0,gone:0},leader:row,comparable:['A']})});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');await flush();
 const cells=f.s2.querySelector('.changes-table tbody tr').children;
 assert.deepEqual(cells.slice(4,6).map(c=>[c.textContent,c.className]),[['0,00','num neutral'],['≈ 0,0%','num neutral']]);
 assert.equal(f.s2.querySelectorAll('.leader-value')[1].textContent,'≈ без изменений');
 assert.match(cells[6].textContent,/0,00 · ≈ 0,0%/);
});

test('print includes every filtered SQL row in order from page zero while leaving the visible page unchanged',async()=>{
 const queries=[],rows=Array.from({length:13},(_,i)=>({name:'Position '+String(i+1).padStart(2,'0'),org:'A',before:i===12?0:1,after:i+2,delta:i+1,percent:i===12?null:100}));
 const f=fixture({referenceChanges:async q=>{queries.push({...q});return {rows:rows.slice(q.page*12,(q.page+1)*12),total:13,searched:13,significant:13,comparable:['A'],counts:{up:13,down:0,new:0,gone:0}};}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 await f.s2.querySelector('.table-viewport').scrollWindow.go(1);
 let printed;f.window.print=()=>{const root=f.document.getElementById('referencePrintDocument');printed={text:root?.textContent,rows:root?.querySelectorAll('.print-changes tbody tr').map(n=>n.children.map(c=>c.textContent)),mode:f.document.body.dataset.referencePrint};};
 const button=f.s2.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF');await button.click();
 assert.deepEqual(printed.rows.map(row=>row[0]),rows.map(row=>row.name));assert.equal(printed.mode,'document');
 assert.equal(printed.rows[12][2],'0,00');assert.equal(printed.rows[12][5],'—');
 assert.match(printed.text,/Значимые изменения/);assert.match(printed.text,/Строк: 13/);assert.match(printed.text,/Сопоставимые подразделения: 1 из 1/);
 assert.deepEqual(queries.slice(-2).map(q=>q.page),[0,1]);assert.ok(queries.every(q=>q.metric==='amount'&&q.kind==='significant'&&q.revision===1));
 assert.equal(f.s2.querySelectorAll('.changes-table tbody tr').length,1);assert.equal(f.s2.querySelector('.changes-table tbody tr').children[0].textContent,'Position 13');
 assert.equal(f.document.getElementById('referencePrintDocument'),null);assert.equal(f.document.body.dataset.referencePrint,undefined);assert.equal(button.disabled,false);assert.equal(button.focused,true);
});
test('changed scope or incomplete SQL print page never prints a partial document',async()=>{
 const delayed=deferred(),f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 f.api.referenceChanges=()=>delayed.promise;let prints=0;f.window.print=()=>prints++;
 const button=f.s2.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF'),pending=button.click();await flush();
 await f.window.referenceResults.refresh(null);delayed.resolve({rows:[{name:'old',org:'A'}],total:1,comparable:['A']});await pending;
 assert.equal(prints,0);assert.equal(f.document.getElementById('referencePrintDocument'),null);
 await f.window.referenceResults.refresh(f.scope);f.api.referenceChanges=async()=>({rows:[],total:1,searched:1,significant:1,comparable:['A'],counts:{up:1,down:0,new:0,gone:0}});await f.window.referenceResults.setScreen('s2');await button.click();assert.equal(prints,0);assert.match(f.notices.at(-1),/страниц|полнот|строк/i);
});
test('change table has one read-only metric and headings follow the active category',async()=>{
 const f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 assert.equal(f.s2.querySelectorAll('[data-change-metric]').length,0);assert.equal(f.s2.querySelector('[data-current-change-metric]').textContent,'Сумма');
 assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Значимые изменения');
 await f.s2.querySelectorAll('.kpi')[0].click();assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Рост');
 f.scope.metric='count';await f.window.referenceResults.refresh(f.scope);assert.equal(f.s2.querySelector('[data-current-change-metric]').textContent,'Количество / объём');
});

test('summary print distinguishes observed totals from the same comparable sample on both sides',async()=>{
 const f=fixture();f.scope.organizations=['A','B'];
 const partial={...f.full,complete:false,missing:1,available_total:{count:15,amount:150,average:10},total:{count:null,amount:null,average:null}};
 f.api.nativeAnalysisCompare=async(...args)=>({summary:{before:f.full,after:args[6].length===2?partial:f.full},divisions:args[6].map(organization=>({organization,before:f.total,after:organization==='B'?{count:null,amount:null,average:null}:f.total})),top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,total:f.total},{org:'B',complete:false,total:{count:null,amount:null,average:null}}];f.api.nativeAnalysisRange=async()=>({...partial,available_total:{count:5,amount:50,average:10}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 let proof;f.window.print=()=>{const root=f.document.getElementById('referencePrintDocument');proof={text:root.textContent,comparison:root.querySelector('.print-comparison tbody').textContent,observed:root.querySelector('.print-observed tbody').textContent};};
 const anchor=f.s1.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF');await anchor.click();
 assert.match(proof.text,/Область работ: Все работы/);assert.match(proof.text,/Сопоставимые подразделения: 1 из 2/);assert.match(proof.text,/Исключены из сравнения: B/);
 assert.match(proof.comparison,/100,00100,00/);assert.match(proof.observed,/150,00/);assert.equal(anchor.focused,true);
});

test('same-revision summary and changes keep their own completed captions and rows during SQL, then clear explicitly on failure',async()=>{
 const f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 const oldRow=f.s2.querySelector('tbody tr'),oldSummary=f.s1.querySelector('tbody tr');
 const oldCaption=f.s1.querySelector('.report-range').textContent;
 const summary=deferred(),changes=deferred();f.api.nativeAnalysisCompare=()=>summary.promise;f.api.referenceChanges=()=>changes.promise;
 const pending=f.window.referenceResults.refresh({...f.scope,from:'2024-02-01',to:'2024-02-02'});
 assert.equal(f.s2.querySelector('tbody tr'),oldRow);assert.equal(f.s1.querySelector('tbody tr'),oldSummary);
 assert.equal(f.s1.querySelector('.report-range').textContent,oldCaption);
 assert.equal(f.s1.getAttribute('aria-busy'),'true');assert.equal(f.s2.getAttribute('aria-busy'),'true');
 changes.reject(Error('read failed'));summary.reject(Error('read failed'));await pending;
 assert.match(f.s2.querySelector('.table-empty').textContent,/read failed/);assert.match(f.s1.querySelector('.quality-notice').textContent,/read failed/);
 assert.equal(f.s1.querySelector('tbody tr'),null);assert.equal(f.s2.querySelector('tbody tr'),null);
 assert.equal(f.s1.getAttribute('aria-busy'),'false');assert.equal(f.s2.getAttribute('aria-busy'),'false');
});

test('zero-base comparison prints confirmed zero without a negative sign and leaves percent unavailable',async()=>{
 const f=fixture();f.api.nativeAnalysisCompare=async()=>({summary:{before:{...f.full,total:{amount:-0,count:-0,average:null}},after:f.full,amount:{delta:100,percent:null},count:{delta:10,percent:null}},divisions:[{organization:'A',before:{amount:-0,count:-0,average:null},after:f.total}],top_amount:[],top_count:[]});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 let rows;f.window.print=()=>{rows=f.document.getElementById('referencePrintDocument').querySelectorAll('.print-comparison tbody tr').map(row=>row.children.map(cell=>cell.textContent));};
 await f.s1.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF').click();
 assert.deepEqual(rows[0].slice(1),['0,00','100,00','+100,00','—']);assert.equal(rows[1][1],'0,000');assert.equal(rows[2][1],'—');
});

test('new search immediately blocks print and CSV until rows match the search box',async()=>{
 const names=['Ремонт','Покраска'],requested=[],printed=[];
 const f=fixture({referenceChanges:async q=>{
  requested.push(q.search);
  const rows=names.filter(name=>name.includes(q.search)).map(name=>({name,org:'A',before:1,after:2,delta:1,percent:100}));
  return {rows,total:rows.length,searched:rows.length,significant:rows.length,comparable:['A'],counts:{up:2,down:0,new:0,gone:0}};
 }});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 await f.window.referenceResults.setScreen('s2');
 const print=f.s2.querySelectorAll('button').find(b=>b.textContent==='Печать / PDF');
 const csv=f.s2.querySelector('[data-export-changes]'),input=f.s2.querySelector('input');
 f.window.print=()=>printed.push(f.document.getElementById('referencePrintDocument')
  .querySelectorAll('.print-changes tbody tr').map(row=>row.children[0].textContent));
 input.value='Ремонт';await input.fire('input');
 await print.click();await csv.click();
 assert.deepEqual(printed,[]);assert.deepEqual(f.exports,[]);assert.deepEqual(requested,['']);
 assert.equal(f.s2.getAttribute('aria-busy'),'true');assert.equal(print.inert,true);
 await new Promise(resolve=>setTimeout(resolve,180));
 await print.click();await csv.click();
 assert.deepEqual(printed,[['Ремонт']]);assert.equal(f.exports[0][0].search,'Ремонт');
 assert.equal(f.s2.getAttribute('aria-busy'),'false');
});

test('editing the search invalidates pending print pages, save chooser and earlier search replies',async()=>{
 const f=fixture(),sql=deferred(),chooser=deferred(),olderSearch=deferred(),newerSearch=deferred();
 const page=name=>({rows:[{name,org:'A',before:1,after:2,delta:1,percent:100}],total:1,searched:1,
  significant:1,comparable:['A'],counts:{up:1,down:0,new:0,gone:0}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 const print=f.s2.querySelectorAll('button').find(b=>b.textContent==='Печать / PDF');
 const csv=f.s2.querySelector('[data-export-changes]'),input=f.s2.querySelector('input'),requests=[];
 let prints=0;f.window.print=()=>prints++;f.api.chooseCsvPath=()=>chooser.promise;
 f.api.referenceChanges=q=>{requests.push(q.search);return q.search==='A'?olderSearch.promise:q.search==='B'?newerSearch.promise:sql.promise;};
 const printing=print.click(),exporting=csv.click();await flush();
 input.value='A';await input.fire('input');
 sql.resolve(page('До поиска'));chooser.resolve('/tmp/before-search.csv');await Promise.all([printing,exporting]);
 assert.equal(prints,0);assert.deepEqual(f.exports,[]);
 await new Promise(resolve=>setTimeout(resolve,180));assert.deepEqual(requests,['','A']);
 input.value='B';await input.fire('input');olderSearch.resolve(page('Устаревшая строка'));await flush();
 assert.equal(f.s2.getAttribute('aria-busy'),'true');assert.doesNotMatch(f.s2.querySelector('.changes-table').textContent,/Устаревшая/);
 await new Promise(resolve=>setTimeout(resolve,180));newerSearch.resolve(page('Строка B'));await flush();
 assert.match(f.s2.querySelector('.changes-table').textContent,/Строка B/);
 assert.equal(f.s2.getAttribute('aria-busy'),'false');
});
