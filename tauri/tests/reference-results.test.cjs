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
 const input=add(s2,'input');input.setAttribute('placeholder','Название работы');const sort=add(s2,'select');sort.setAttribute('data-sort','');sort.value='delta';const metric=add(s2,'strong');metric.setAttribute('data-current-change-metric','');const heading=add(s2,'div','changes-heading');add(heading,'h2');add(heading,'span','excluded-orgs');
 add(s2,'p','match-note');const ct=add(add(s2,'div','table-viewport'),'table','changes-table');const head=add(ct,'thead'),tr=add(head,'tr');for(let i=0;i<6;i++)add(tr,'th');add(ct,'tbody');add(s2,'div','table-empty');add(s2,'span','pager-footer');const exp=add(s2,'button','','Скачать CSV');exp.setAttribute('data-export-changes','');
 const total={count:10,amount:100,average:10},full={complete:true,periods:1,missing:0,total,available_reports:1,available_total:total,invalid_reports:0,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};
 const calls=[],exports=[],notices=[],downloads=[];const api={nativeAnalysisCompare:async(...args)=>{calls.push(args);return {summary:{before:full,after:full},divisions:args[6].map(organization=>({organization,before:total,after:total})),top_amount:[],top_count:[]};},nativeAnalysisOrganizations:async(...args)=>args[4].map(org=>({org,complete:true,missing:0,total})),nativeAnalysisRange:async()=>full,referenceChanges:async()=>({rows:[{name:'Work',org:'A',before:1,after:2,delta:1,percent:100}],total:1,searched:1,significant:1,counts:{up:1,down:0,new:0,gone:0},leader:null,comparable:['A']}),referenceChangesPrint:async(q,emit)=>{const part=await api.referenceChanges({...q,page:0});if(part.rows.length)emit(part.rows);return part.comparable;},chooseCsvPath:async()=>'/tmp/changes.csv',referenceChangesCsv:async(...args)=>exports.push(args),referenceSummaryCsv:async(...args)=>exports.push(args),...overrides};
 const sheet=add(body,'dialog');sheet.id='sheet';sheet.open=false;sheet.close=async()=>{sheet.open=false;await sheet.fire('close');};
 const document={body,createElementNS:(_namespace,tag)=>new Node(tag),createElement:tag=>new Node(tag),querySelector:q=>body.querySelector(q),querySelectorAll:q=>body.querySelectorAll(q),getElementById:id=>body.querySelector('#'+id)};
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),desktopAPI:api,referenceDesktop:{dialog:()=>{sheet.open=true;const content=new Node('div');content.append(new Node('h2'));sheet.replaceChildren(content);return content;},setDialogApply:action=>sheet.append(action),navigate:id=>window.referenceResults.setScreen(id),notice:text=>notices.push(text)},referenceFilters:{setMetric:m=>{scope.metric=m;window.referenceResults.refresh(scope);}},unifiedCoverage:{render:async()=>{}},print(){}};
 const scope={revision:1,type:'day',from:'2024-01-02',to:'2024-01-02',baseFrom:'2024-01-01',baseTo:'2024-01-01',organizations:['A'],metric:'amount',source:'daily',displayType:'day'};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),{window});
 const file=path.join(__dirname,'../reference-results.js');if(fs.existsSync(file))vm.runInNewContext(fs.readFileSync(file,'utf8'),{window,document,Intl,Number,Date,Set,Map,Promise,setTimeout,clearTimeout,console,Blob,TextEncoder,AbortController,URL:{createObjectURL:blob=>{downloads.push(blob);return 'blob:test';},revokeObjectURL(){}}});
 return {window,document,api,scope,s1,s2,calls,exports,notices,downloads,full,total};
}
test('summary presents observed partial totals for all selected organizations',async()=>{
 const f=fixture();assert.ok(f.window.referenceResults,'SQL reference results controller must be present');
 const incomplete={...f.full,complete:false,missing:1,single_missing:{org:'B',start:'2024-01-02'},total:{count:null,amount:null,average:null},available_total:{count:7,amount:70,average:10}};
 f.api.nativeAnalysisCompare=async(...args)=>{f.calls.push(args);return {summary:{before:f.full,after:args[6].length===2?incomplete:f.full},divisions:args[6].map(organization=>({organization,before:f.total,after:organization==='B'?{count:null,amount:null,average:null}:f.total})),top_amount:[],top_count:[]};};
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,missing:0,total:f.total},{org:'B',complete:false,missing:1,total:{count:null,amount:null,average:null}}];f.api.nativeAnalysisRange=async()=>incomplete;f.scope.organizations=['A','B'];
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 assert.match(f.s1.querySelector('.metric-value').textContent,/70/);assert.match(f.s1.querySelector('.quality-notice').textContent,/1 из 2/);assert.equal(f.calls.length,1);assert.deepEqual([...f.calls[0][6]],['A','B']);assert.doesNotMatch(f.s1.textContent,/₽|ед\./);
 assert.match(f.s1.querySelector('.quality-notice').textContent,/B · 02\.01\.2024: нет пригодного отчёта/);
 assert.match(f.s1.querySelector('.report-range').textContent,/2 янв\. 2024/);
 for(const screen of [f.s1,f.s2]) assert.match(screen.querySelector('.report-range').children[1].textContent,/^Сравнение с /);
 await f.s1.querySelector('.metric-sample').click();const cells=f.document.querySelectorAll('.sample-values tbody tr');
 assert.deepEqual(cells.map(row=>row.children.map(c=>c.textContent)),[['Сумма','100,00','70,00'],['Количество / объём','10,000','7,000']]);
 incomplete.missing=2;await f.window.referenceResults.refresh(f.scope);assert.doesNotMatch(f.s1.querySelector('.quality-notice').textContent,/02\.01\.2024/);
});
test('summary cards and leaders compare available reports when every organization has gaps',async()=>{
 const f=fixture();
 const period=(amount,missing)=>({...f.full,complete:false,missing,total:{count:null,amount:null,average:null},available_total:{count:amount/10,amount,average:10}});
 const amount={base:100,current:140,delta:40,percent:40};
 f.api.nativeAnalysisCompare=async()=>({summary:{before:period(100,1),after:period(140,2),amount},
  divisions:[{organization:'A',before:{count:null},after:{count:null}}],top_amount:[{name:'Ремонт',kind:'work',amount}],top_count:[]});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 assert.match(f.s1.querySelector('.metric-delta').textContent,/\+40,00/);
 assert.match(f.s1.querySelector('.insight-card').textContent,/Ремонт/);
 const note=f.s1.querySelector('.quality-notice').textContent;
 assert.match(note,/по доступным отчётам/);assert.match(note,/Пропусков: 3/);
});
test('summary keeps quantities and reports unavailable amounts in complete periods',async()=>{
 const f=fixture();
 const total={count:17,amount:null,average:null};
 const current={...f.full,total,available_total:total};
 f.api.nativeAnalysisCompare=async()=>({summary:{before:f.full,after:current,count:{delta:7,percent:70}},divisions:[{organization:'A',before:f.total,after:total}],top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,missing:0,total}];
 f.window.referenceResults.init();
 await f.window.referenceResults.refresh({...f.scope,metric:'count'});
 assert.deepEqual(f.s1.querySelectorAll('.metric-value').map(n=>n.textContent),['—','17,000','—']);
 assert.match(f.s1.querySelector('.quality-notice').textContent,/сумма не указана.*Количество учтено/);
 await f.s1.querySelector('.metric-sample').click();
 assert.match(f.document.querySelector('#sheet').textContent,/A: полные отчёты за оба интервала/);
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
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);const rows=f.s1.querySelectorAll('tbody tr');assert.deepEqual(rows.map(row=>row.children[0].textContent),['B','A','D','C (нет данных)']);
 const warnings=f.s1.querySelectorAll('.org-warning');assert.equal(warnings.length,2);assert.match(warnings[0].getAttribute('aria-label'),/^B/);await warnings[0].click();assert.match(f.document.getElementById('sheet').textContent,/неполный базовый интервал/);assert.match(f.document.getElementById('sheet').textContent,/Сравнить только подразделения с полными отчётами/);await f.document.getElementById('sheet').close();assert.equal(warnings[0].focused,true);await warnings[0].click();await f.window.referenceResults.refresh(null);assert.equal(f.document.getElementById('sheet').open,false,'stale sample modal must close on refresh');
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

test('changes show observed partial values alongside significant rows and preserve zero',async()=>{
 const rows=[
  {name:'Полная строка',org:'A',before:30,after:60,delta:30,percent:100},
  {name:'Частичная строка',org:'B',before:20,after:0,delta:null,percent:null}
 ];
 const f=fixture({referenceChanges:async()=>({rows,total:2,searched:3,significant:1,partial_fallback:true,counts:{up:1,down:0,new:0,gone:0},leader:rows[0],comparable:['A']})});
 f.scope.organizations=['A','B'];
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 const displayed=f.s2.querySelectorAll('.changes-table tbody tr');
 assert.deepEqual(displayed.map(row=>row.children.slice(0,6).map(cell=>cell.textContent)),[
  ['Полная строка','A','30,00','60,00','+30,00','+100,0%'],
  ['Частичная строка','B','20,00','0,00','—','—']
 ]);
 const caption=f.s2.querySelector('.match-note');
 assert.match(caption.textContent,/Значимые позиции · 1 · Доступные значения · 1/);
 assert.match(caption.textContent,/Показать все 3/);
 assert.equal(f.s2.querySelector('.excluded-orgs').hidden,true);
});

test('print includes every filtered SQL row in order from page zero while leaving the visible page unchanged',async()=>{
 const queries=[],prints=[],rows=Array.from({length:51},(_,i)=>({name:'Position '+String(i+1).padStart(2,'0'),org:'A',before:i===50?0:1,after:i+2,delta:i+1,percent:i===50?null:100}));
 const f=fixture({referenceChanges:async q=>{queries.push({...q});return {rows:rows.slice(q.page*50,(q.page+1)*50),total:51,searched:51,significant:51,comparable:['A'],counts:{up:51,down:0,new:0,gone:0}};},referenceChangesPrint:async(q,emit)=>{prints.push(q);emit(rows.slice(0,50));emit(rows.slice(50));return ['A'];}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 const viewport=f.s2.querySelector('.table-viewport');
 assert.equal(viewport.scrollWindow.size,50);
 assert.equal(f.s2.querySelectorAll('.changes-table tbody tr').length,50);
 await viewport.scrollWindow.go(1);
 let printed;f.window.print=()=>{const root=f.document.getElementById('referencePrintDocument');printed={text:root?.textContent,rows:root?.querySelectorAll('.print-changes tbody tr').map(n=>n.children.map(c=>c.textContent)),mode:f.document.body.dataset.referencePrint};};
 const button=f.s2.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF');await button.click();
 assert.deepEqual(printed.rows.map(row=>row[0]),rows.map(row=>row.name));assert.equal(printed.mode,'document');
 assert.equal(printed.rows[50][2],'0,00');assert.equal(printed.rows[50][5],'—');
 assert.match(printed.text,/Значимые изменения/);assert.match(printed.text,/Подразделения с полными отчётами: 1 из 1/);
 assert.deepEqual(queries.map(q=>q.page),[0,1]);assert.deepEqual(prints.map(q=>q.page),[1]);assert.ok(queries.every(q=>q.metric==='amount'&&q.kind==='significant'&&q.revision===1));
 assert.equal(f.s2.querySelectorAll('.changes-table tbody tr').length,1);assert.equal(f.s2.querySelector('.changes-table tbody tr').children[0].textContent,'Position 51');
 assert.equal(f.document.getElementById('referencePrintDocument'),null);assert.equal(f.document.body.dataset.referencePrint,undefined);assert.equal(button.disabled,false);assert.equal(button.focused,true);
});
test('changed scope or incomplete SQL print page never prints a partial document',async()=>{
 const delayed=deferred(),f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 let signal;f.api.referenceChangesPrint=async(q,emit,stop)=>{signal=stop;const reply=await delayed.promise;emit(reply.rows);return reply.comparable;};let prints=0;f.window.print=()=>prints++;
 const button=f.s2.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF'),pending=button.click();await flush();
 await f.window.referenceResults.refresh(null);assert.equal(signal.aborted,true);delayed.resolve({rows:[{name:'old',org:'A'}],total:1,comparable:['A']});await pending;
 assert.equal(prints,0);assert.equal(f.document.getElementById('referencePrintDocument'),null);
 await f.window.referenceResults.refresh(f.scope);f.api.referenceChangesPrint=async()=>['A'];f.api.referenceChanges=async()=>({rows:[],total:1,searched:1,significant:1,comparable:['A'],counts:{up:1,down:0,new:0,gone:0}});await f.window.referenceResults.setScreen('s2');await button.click();assert.equal(prints,0);assert.match(f.notices.at(-1),/страниц|полнот|строк/i);
});
test('change table has one read-only metric and headings follow the active category',async()=>{
 const f=fixture();f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);await f.window.referenceResults.setScreen('s2');
 assert.equal(f.s2.querySelectorAll('[data-change-metric]').length,0);assert.equal(f.s2.querySelector('[data-current-change-metric]').textContent,'Сумма');
 assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Значимые изменения');
 await f.s2.querySelectorAll('.kpi')[0].click();assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Рост');
 await f.s2.querySelectorAll('.kpi')[2].click();assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Новые работы');
 await f.s2.querySelectorAll('.kpi')[3].click();assert.equal(f.s2.querySelector('.changes-heading h2').textContent,'Без выполнений');
 f.scope.metric='count';await f.window.referenceResults.refresh(f.scope);assert.equal(f.s2.querySelector('[data-current-change-metric]').textContent,'Количество / объём');
});

test('summary print uses the same observed comparison as the cards',async()=>{
 const f=fixture();f.scope.organizations=['A','B'];
 const partial={...f.full,complete:false,missing:1,available_total:{count:15,amount:150,average:10},total:{count:null,amount:null,average:null}};
 f.api.nativeAnalysisCompare=async(...args)=>({summary:{before:f.full,after:partial,amount:{delta:50,percent:50}},divisions:args[6].map(organization=>({organization,before:f.total,after:organization==='B'?{count:null,amount:null,average:null}:f.total})),top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,total:f.total},{org:'B',complete:false,total:{count:null,amount:null,average:null}}];f.api.nativeAnalysisRange=async()=>({...partial,available_total:{count:5,amount:50,average:10}});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 let proof;f.window.print=()=>{const root=f.document.getElementById('referencePrintDocument');proof={text:root.textContent,comparison:root.querySelector('.print-comparison tbody').textContent,observed:root.querySelector('.print-observed tbody').textContent};};
 const anchor=f.s1.querySelectorAll('button').find(n=>n.textContent==='Печать / PDF');await anchor.click();
 assert.match(proof.text,/Область работ: Все работы/);assert.match(proof.text,/Полные отчёты у 1 из 2/);assert.match(proof.text,/B: неполный анализируемый интервал/);
 assert.match(proof.comparison,/100,00150,00\+50,00/);assert.match(proof.observed,/150,00/);assert.equal(anchor.focused,true);
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
 const f=fixture(),zero={amount:-0,count:-0,average:null};f.api.nativeAnalysisCompare=async()=>({summary:{before:{...f.full,total:zero,available_total:zero},after:f.full,amount:{delta:100,percent:null},count:{delta:10,percent:null}},divisions:[{organization:'A',before:zero,after:f.total}],top_amount:[],top_count:[]});
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

test('failed changes reads distinguish unknown counts from zero and clear the old sample; summary CSV ignores changes controls',async()=>{
 const f=fixture(),results=f.window.referenceResults;f.scope.organizations=['A','B'];
 results.init();await results.refresh(f.scope);await results.setScreen('s2');
 const chip=f.s2.querySelector('.excluded-orgs'),search=f.s2.querySelector('input');
 assert.equal(chip.hidden,true);
 assert.match(f.s2.querySelector('.quality-notice').textContent,/изменения по доступным отчётам.*Полные отчёты у 1 из 2 подразделений/);
 assert.deepEqual(f.s2.querySelectorAll('[data-count]').map(n=>n.textContent),['▲ 1','▼ 0','0','0']);
 await f.s2.querySelectorAll('.kpi')[2].click();f.s2.querySelector('[data-sort]').value='percent';
 let calls=0;f.api.referenceChanges=async()=>{calls++;throw Error('read rejected');};
 search.value='я'.repeat(256);await search.fire('input');await new Promise(resolve=>setTimeout(resolve,180));
 assert.equal(calls,1);assert.match(f.s2.querySelector('.table-empty').textContent,/read rejected/);
 assert.deepEqual(f.s2.querySelectorAll('[data-count]').map(n=>n.textContent),['—','—','—','—']);
 assert.ok(f.s2.querySelectorAll('.kpi').every(n=>n.getAttribute('aria-label').includes('нет данных')));
 assert.equal(chip.hidden,true);assert.equal(chip.textContent,'');
 search.value+='я';await search.fire('input');await new Promise(resolve=>setTimeout(resolve,180));
 assert.equal(calls,1);assert.match(f.s2.querySelector('.table-empty').textContent,/512 байт UTF-8/);
 assert.equal(f.s2.querySelector('[data-export-changes]').disabled,true);
 await results.setScreen('s1');await f.s1.querySelector('.data-card button').click();
 assert.equal(f.exports.length,1);const q=f.exports[0][0];
 assert.deepEqual([q.search,q.kind,q.sort,q.page,q.minAmount,q.minCount,q.percent],['','all','name',0,0,0,0]);
 assert.deepEqual(Array.from(q.organizations),['A','B']);assert.equal(q.from,f.scope.from);
});
test('bounds transition invalidates reads, choosers and controls through screen switches and recovers after failure',async()=>{
 const f=fixture(),r=f.window.referenceResults;r.init();await r.refresh(f.scope);await r.setScreen('s2');
 const chooser=deferred();f.api.chooseCsvPath=()=>chooser.promise;
 const exportPending=f.s2.querySelector('[data-export-changes]').click();
 const reply=deferred();let reads=0;f.api.referenceChanges=()=>{reads++;return reply.promise;};
 const readPending=r.setScreen('s2'),oldRow=f.s2.querySelector('tbody tr'),caption=f.s2.querySelector('.report-range').textContent;
 r.beginScopeChange(f.scope.revision);
 chooser.resolve('/tmp/obsolete.csv');await exportPending;assert.equal(f.exports.length,0);
 await r.setScreen('s1');await r.setScreen('s2');
 await f.s2.querySelector('input').fire('input');await new Promise(resolve=>setTimeout(resolve,180));
 reply.resolve({rows:[],total:0,searched:0,significant:0,comparable:[],counts:{up:0,down:0,new:0,gone:0}});await readPending;
 assert.equal(reads,1);assert.equal(f.s2.querySelector('tbody tr'),oldRow);assert.equal(f.s2.querySelector('.report-range').textContent,caption);
 for(const screen of [f.s1,f.s2])assert.equal(screen.getAttribute('aria-busy'),'true');
 r.failScopeChange('Не удалось прочитать даты источника');
 assert.equal(f.s2.querySelector('tbody tr'),null);assert.match(f.s2.querySelector('.table-empty').textContent,/даты источника/);
 assert.equal(f.s1.querySelector('.metric-value').textContent,'—');assert.match(f.s1.querySelector('.quality-notice').textContent,/даты источника/);
 assert.equal(f.s2.querySelector('[data-export-changes]').disabled,true);assert.equal(f.s1.querySelector('.data-card button').disabled,true);
 await r.setScreen('s1');await r.setScreen('s2');assert.equal(reads,1);
 await r.refresh(f.scope);assert.equal(reads,2);assert.match(f.s1.querySelector('.metric-value').textContent,/100/);
 r.beginScopeChange(2);assert.equal(f.s1.querySelector('.metric-value').textContent,'—');
});

test('summary and changes print historical quality counts once, separately from each interval',async()=>{
 const f=fixture();
 f.full.invalid_reports=1;
 f.full.import_warning_reports=1;
 const printed=[];
 f.window.print=()=>printed.push(f.document.getElementById('referencePrintDocument').querySelectorAll('p').map(row=>row.textContent));
 f.window.referenceResults.init();
 await f.window.referenceResults.refresh(f.scope);
 for(const [id,screen] of [['s1',f.s1],['s2',f.s2]]){
  await f.window.referenceResults.setScreen(id);
  const banner=screen.querySelector('.quality-notice span').textContent;
  assert.match(banner,/За всю историю отчётов этого типа: ошибочных отчётов по всем подразделениям — 1/);
  await screen.querySelectorAll('button').find(node=>node.textContent.includes('Печать')).click();
  const paragraphs=printed.at(-1);
  assert.deepEqual(paragraphs.filter(line=>/^(База|Анализ): пропусков отчётов/.test(line)),
   ['База: пропусков отчётов — 0.','Анализ: пропусков отчётов — 0.']);
  assert.equal(paragraphs.filter(line=>line.includes('отчётов с замечаниями в выбранных подразделениях — 1')).length,1);
 }
});

test('a blank sensitivity threshold keeps the dialog open and explicit zero remains valid',async()=>{
 const requests=[];
 const f=fixture({referenceChanges:async q=>{
  requests.push(q);return {rows:[],total:0,searched:0,significant:0,counts:{up:0,down:0,new:0,gone:0},comparable:['A']};
 }});
 f.window.referenceResults.init();await f.window.referenceResults.refresh(f.scope);
 await f.window.referenceResults.setScreen('s2');
 await f.s2.querySelector('.filter-threshold').click();
 const sheet=f.document.getElementById('sheet');
 const [slider,quantity,amount]=sheet.querySelectorAll('input');
 const apply=sheet.querySelectorAll('button').find(b=>b.textContent==='Применить');
 const before=requests.length;
 amount.value='';await apply.click();
 assert.equal(sheet.open,true);assert.equal(requests.length,before);
 slider.value='30';quantity.value='5';amount.value='0';await apply.click();
 assert.equal(sheet.open,false);assert.equal(requests.at(-1).minAmount,0);
});

test('missing baseline explains an unavailable comparison while preserving an observed zero',async()=>{
 const f=fixture();
 const zero={count:0,amount:0,average:null};
 const observed={...f.full,total:zero,available_total:zero};
 const unavailable={...f.full,complete:false,missing:1,total:{count:null,amount:null,average:null},available_reports:0,available_total:{count:null,amount:null,average:null}};
 f.api.nativeAnalysisCompare=async()=>({summary:{before:unavailable,after:observed},divisions:[{organization:'A',before:unavailable.total,after:zero}],top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[{org:'A',complete:true,missing:0,total:zero}];
 f.api.referenceChanges=async()=>({rows:[],total:0,searched:0,significant:0,counts:{up:0,down:0,new:0,gone:0},leader:null,comparable:[]});
 f.window.referenceResults.init();
 await f.window.referenceResults.refresh(f.scope);
 await f.window.referenceResults.setScreen('s2');
 assert.equal(f.s1.querySelector('.metric-value').textContent,'0,00');
 for(const screen of [f.s1,f.s2]){
  const notice=screen.querySelector('.quality-notice').textContent;
  assert.match(notice,/За базовый интервал нет пригодных данных/);
  assert.doesNotMatch(notice,/Сравнение по 0 из/);
 }
 await f.s1.querySelector('.metric-sample').click();
 const sample=f.document.getElementById('sheet');
 assert.match(sample.textContent,/За базовый интервал нет пригодных данных/);
 assert.deepEqual(sample.querySelector('.sample-values tbody tr').children.map(n=>n.textContent),['Сумма','—','0,00']);
 await sample.close();
 let printed,totals;
 f.window.print=()=>{const root=f.document.getElementById('referencePrintDocument');printed=root.textContent;totals=root.querySelector('.print-comparison tbody tr').children.map(n=>n.textContent);};
 const print=f.s2.querySelectorAll('button').find(button=>button.textContent==='Печать / PDF');
 await print.click();
 assert.match(printed,/За базовый интервал нет пригодных данных/);
 assert.deepEqual(totals,['Сумма','—','0,00','—','—']);
});

test('summary displays shares of known amounts and average for covered values',async()=>{
 const f=fixture(),known={amount:60,count:4,average:15},partial={amount:40,count:5,average:null};
 const current={...f.full,amount_missing_rows:true,available_total:{amount:100,count:9,average:null}};
 f.api.nativeAnalysisCompare=async()=>({summary:{before:f.full,after:current},divisions:[],top_amount:[],top_count:[]});
 f.api.nativeAnalysisOrganizations=async()=>[
  {org:'A',complete:false,missing:1,available_total:known},
  {org:'B',complete:false,missing:1,available_total:partial}
 ];
 f.scope.organizations=['A','B'];
 f.window.referenceResults.init();
 await f.window.referenceResults.refresh(f.scope);
 const rows=f.s1.querySelectorAll('tbody tr');
 assert.deepEqual(rows.map(row=>row.children.slice(3).map(cell=>cell.textContent)),[['60,0%','15,00'],['40,0%','—']]);
 assert.match(f.s1.querySelector('.quality-notice').textContent,/Доли рассчитаны по заполненным суммам/);
});
