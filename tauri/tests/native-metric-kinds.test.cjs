'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),vm=require('node:vm');
const {prepare}=require('../prepare.cjs');
const {userError}=require('../native-errors.js');
const script=fs.readFileSync(path.join(__dirname,'../native-metric-kinds.js'),'utf8');
const labor={id:'labor',name:'Трудозатраты',dimensionLabel:'Вид работы',metrics:[
 {id:'hours',name:'Часы',unit:'ч',operation:'sum'},
 {id:'units',name:'Сделано',unit:'шт',operation:'sum'},
 {id:'units_per_hour',name:'Выработка',unit:'шт/ч',operation:'ratio',numerator:'units',denominator:'hours'}
]};
const plan={id:'plan_fact',name:'План и факт',dimensionLabel:'Показатель',metrics:[
 {id:'plan',name:'План',unit:'шт',operation:'sum'},
 {id:'actual',name:'Факт',unit:'шт',operation:'sum'},
 {id:'completion',name:'Выполнение',unit:'%',operation:'ratio',numerator:'actual',denominator:'plan',factor:100}
]};
class Node{
 constructor(tag='div'){
  this.tag=tag;this.children=[];this.events={};this.textContent='';this.className='';
  this.value='';this.disabled=false;
 }
 replaceChildren(...children){this.children=[...children];}
 append(...children){this.children.push(...children);}
 addEventListener(name,handler){this.events[name]=handler;}
}
function mount(opts={}){
 const ids=['metricKindStatus','metricKindSelect','metricKindFrom','metricKindTo',
  'metricKindCalculate','metricKindChoose','metricKindChooseXlsx','metricXlsxProfile',
  'metricKindApply','metricKindPreview','metricKindTotals','metricKindCount','metricKindFile'];
 const elements=Object.fromEntries(ids.map(id=>[id,new Node()]));
 let confirmations=0;
 const calls=[];
 const preview={kind:'labor',name:'Трудозатраты',reportId:'lab1',
  start:'2026-11-18',end:'2026-11-18',rowCount:2,
  reviewToken:'a'.repeat(64),values:{hours:8,units:12,units_per_hour:1.5}};
 const api={
  nativeMetricKinds:async()=>[labor,plan],
  nativeMetricXlsxProfiles:async()=>[{id:'labor_simple',kind:'labor',name:'Простой Excel',
    sheet:'Data',headerRow:1,firstDataRow:2,organizationColumn:'A',
    dimensionColumn:'B',values:{hours:'C',units:'D'},headers:{A:'Подразделение',B:'Работа',C:'Часы',D:'Объём'}}],
  chooseMetricXlsx:async()=>{calls.push(['choose_xlsx']);return opts.cancelXlsx?null:'C:/Data/hours.xlsx';},
  nativeMetricXlsxPreview:async(...args)=>{calls.push(['xlsx_preview',...args]);return preview;},
  nativeMetricXlsxApply:async(...args)=>{calls.push(['xlsx_apply',...args]);
    if(opts.failXlsx)throw Error('source changed');
    return {reportId:'lab1',rowCount:2};},
  nativeMetricSummary:async(...args)=>{calls.push(['summary',...args]);
    return {kind:args[0],reportCount:1,rowCount:2,
      values:args[0]==='labor'?{hours:8,units:12,units_per_hour:1.5}:
      {plan:10,actual:7,completion:70}};},
  chooseMetricJson:async()=>{calls.push(['choose']);return opts.cancel?null:'C:/Data/new.json';},
  nativeMetricPreview:async source=>{calls.push(['preview',source]);return preview;},
  nativeMetricApply:async(source,token)=>{calls.push(['apply',source,token]);
    return {reportId:'lab1',rowCount:2};}
 };
 const window={desktopAPI:api,unifiedUI:opts.ui||null,confirm:()=>{confirmations++;return !opts.decline;},analizUserError:userError};
 vm.runInNewContext(script,{window,document:{
  getElementById:id=>elements[id],createElement:tag=>new Node(tag)
 }});
 return {elements,calls,window,get confirmations(){return confirmations;}};
}
test('metric catalog and values are rendered from declarations, not hardcoded count/amount',async()=>{
 const {elements:e,calls,window}=mount();
 await window.metricKindWorkspaceReady;
 assert.equal(e.metricKindSelect.children.length,2);
 assert.equal(e.metricKindSelect.children[1].value,'plan_fact');
 await e.metricKindCalculate.events.click();
 assert.equal(calls[0][0],'summary');
 assert.equal(calls[0][1],'labor');
 assert.equal(calls[0][2],'0001-01-01');
 assert.equal(e.metricKindTotals.children.length,3);
 assert.equal(e.metricKindTotals.children[0].children[1].textContent,'Часы · ч');
 e.metricKindSelect.value='plan_fact';e.metricKindSelect.events.change();
 await e.metricKindCalculate.events.click();
 assert.equal(calls.at(-1)[1],'plan_fact');
 assert.equal(e.metricKindTotals.children[2].children[1].textContent,'Выполнение · %');
});
test('explicit review and confirmation precede insert-only import',async()=>{
 const {elements:e,calls,window}=mount();
 await window.metricKindWorkspaceReady;
 assert.equal(e.metricKindApply.disabled,true);
 await e.metricKindChoose.events.click();
 assert.equal(e.metricKindApply.disabled,false);
 assert.equal(calls.filter(c=>c[0]==='apply').length,0);
 await e.metricKindApply.events.click();
 assert.equal(calls.filter(c=>c[0]==='apply').length,1);
 assert.equal(calls.at(-1)[1],'C:/Data/new.json');
 assert.equal(calls.at(-1)[2],'a'.repeat(64));
 assert.equal(e.metricKindApply.disabled,true);
 assert.match(e.metricKindStatus.textContent,/Отчёт добавлен/);
});
test('cancelled file or denied confirmation makes no writes',async()=>{
 const cancelled=mount({cancel:true});await cancelled.window.metricKindWorkspaceReady;
 await cancelled.elements.metricKindChoose.events.click();
 assert.equal(cancelled.calls.some(c=>c[0]==='preview'),false);
 assert.equal(cancelled.elements.metricKindApply.disabled,true);
 const declined=mount({decline:true});await declined.window.metricKindWorkspaceReady;
 await declined.elements.metricKindChoose.events.click();
 await declined.elements.metricKindApply.events.click();
 assert.equal(declined.calls.some(c=>c[0]==='apply'),false);
});
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).

test('mapped XLSX is previewed with selected profile and explicit dates',async()=>{
 const {elements:e,calls,window}=mount();await window.metricKindWorkspaceReady;
 assert.equal(e.metricXlsxProfile.children.length,1);
 assert.equal(e.metricKindChooseXlsx.disabled,false);
 await e.metricKindChooseXlsx.events.click();
 assert.equal(calls.some(v=>v[0]==='choose_xlsx'),false,'date required before file picker');
 assert.equal(e.metricKindApply.disabled,true);
 e.metricKindFrom.value='2026-11-18';e.metricKindTo.value='2026-11-18';
 await e.metricKindChooseXlsx.events.click();
 const review=calls.find(v=>v[0]==='xlsx_preview');
 assert.deepEqual(review,['xlsx_preview','C:/Data/hours.xlsx','labor_simple','2026-11-18','2026-11-18']);
 assert.equal(e.metricKindApply.disabled,false);
 await e.metricKindApply.events.click();
 const apply=calls.find(v=>v[0]==='xlsx_apply');
 assert.deepEqual(apply,['xlsx_apply','C:/Data/hours.xlsx','labor_simple','2026-11-18','2026-11-18','a'.repeat(64)]);
 assert.equal(e.metricKindApply.disabled,true);
});
test('changing XLSX mapping or date invalidates review and cannot apply stale token',async()=>{
 const {elements:e,calls,window}=mount();await window.metricKindWorkspaceReady;
 e.metricKindFrom.value='2026-11-18';e.metricKindTo.value='2026-11-18';
 await e.metricKindChooseXlsx.events.click();
 assert.equal(e.metricKindApply.disabled,false);
 e.metricXlsxProfile.events.change();
 assert.equal(e.metricKindApply.disabled,true);
 await e.metricKindApply.events.click();
 assert.equal(calls.some(c=>c[0]==='xlsx_apply'),false);
 await e.metricKindChooseXlsx.events.click();
 e.metricKindFrom.value='2026-11-19';e.metricKindFrom.events.change();
 assert.equal(e.metricKindApply.disabled,true);
});
test('cancelled or rejected XLSX import never reports success',async()=>{
 const cancelled=mount({cancelXlsx:true});await cancelled.window.metricKindWorkspaceReady;
 cancelled.elements.metricKindFrom.value='2026-11-18';
 cancelled.elements.metricKindTo.value='2026-11-18';
 await cancelled.elements.metricKindChooseXlsx.events.click();
 assert.equal(cancelled.elements.metricKindApply.disabled,true);
 assert.equal(cancelled.calls.some(c=>c[0]==='xlsx_preview'),false);
 const failed=mount({failXlsx:true});await failed.window.metricKindWorkspaceReady;
 failed.elements.metricKindFrom.value='2026-11-18';failed.elements.metricKindTo.value='2026-11-18';
 await failed.elements.metricKindChooseXlsx.events.click();
 await failed.elements.metricKindApply.events.click();
 assert.equal(failed.elements.metricKindApply.disabled,true);
 assert.match(failed.elements.metricKindStatus.textContent,/Импорт отклонён/);
 assert.match(failed.elements.metricKindStatus.textContent,/Файл изменился после проверки/);
 assert.ok(!failed.elements.metricKindStatus.textContent.includes('source changed'));
});

const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};
test('native asynchronous metric confirmation awaits decision and blocks duplicate writes',async()=>{
 const {elements:e,calls,window}=mount();await window.metricKindWorkspaceReady;
 await e.metricKindChoose.events.click();
 const approval=deferred();window.confirm=()=>approval.promise;
 const pending=e.metricKindApply.events.click();
 assert.equal(e.metricKindApply.disabled,true);assert.equal(calls.some(c=>c[0]==='apply'),false);
 await e.metricKindApply.events.click();assert.equal(calls.some(c=>c[0]==='apply'),false);
 approval.resolve(false);await pending;assert.equal(calls.some(c=>c[0]==='apply'),false);
 const again=deferred();window.confirm=()=>again.promise;
 const requested=e.metricKindApply.events.click();again.resolve(true);await requested;
 assert.equal(calls.filter(c=>c[0]==='apply').length,1);
});
test('metric confirmation refuses stale revision and changed parameters',async()=>{
 const ui={revision:1,refresh:async()=>{}};
 const a=mount({ui});await a.window.metricKindWorkspaceReady;await a.elements.metricKindChoose.events.click();
 const approval=deferred();a.window.confirm=()=>approval.promise;
 const pending=a.elements.metricKindApply.events.click();ui.revision=2;approval.resolve(true);await pending;
 assert.equal(a.calls.some(c=>c[0]==='apply'),false);
 const b=mount();await b.window.metricKindWorkspaceReady;
 b.elements.metricKindFrom.value='2026-11-18';b.elements.metricKindTo.value='2026-11-18';await b.elements.metricKindChooseXlsx.events.click();
 const approval2=deferred();b.window.confirm=()=>approval2.promise;
 const pending2=b.elements.metricKindApply.events.click();b.elements.metricKindFrom.value='2026-11-19';
 approval2.resolve(true);await pending2;
 assert.equal(b.calls.some(c=>c[0]==='xlsx_apply'),false);
});
test('metric commit is reported as successful after a failed dashboard refresh',async()=>{
 const ui={revision:1,refresh:async()=>{throw Error('refresh fail');}};
 const m=mount({ui});await m.window.metricKindWorkspaceReady;await m.elements.metricKindChoose.events.click();
 await m.elements.metricKindApply.events.click();
 assert.equal(m.calls.filter(c=>c[0]==='apply').length,1);
 assert.match(m.elements.metricKindStatus.textContent,/Отчёт добавлен/);
 assert.match(m.elements.metricKindStatus.textContent,/Не удалось обновить экран/);
 assert.doesNotMatch(m.elements.metricKindStatus.textContent,/Импорт отклонён/);
});

test('fresh SQLite snapshot invalidates pending metric preview and previously reviewed token',async()=>{
 let notify;
 const ui={revision:1,refresh:async()=>{},onSnapshot:fn=>{notify=fn;}};
 const m=mount({ui});await m.window.metricKindWorkspaceReady;
 await m.elements.metricKindChoose.events.click();assert.equal(m.elements.metricKindApply.disabled,false);
 ui.revision=2;notify(null);
 assert.equal(m.elements.metricKindApply.disabled,true);
 assert.match(m.elements.metricKindStatus.textContent,/повторно проверьте источник/);
 await m.elements.metricKindApply.events.click();
 assert.equal(m.calls.some(c=>c[0]==='apply'),false);
});
test('metric calculate does not publish a stale response after SQLite revision changes',async()=>{
 let resolve;
 const ui={revision:1,refresh:async()=>{},onSnapshot:()=>{}};
 const m=mount({ui});await m.window.metricKindWorkspaceReady;
 m.window.desktopAPI.nativeMetricSummary=()=>new Promise(r=>resolve=r);
 const pending=m.elements.metricKindCalculate.events.click();
 ui.revision=2;
 resolve({reportCount:10,rowCount:12,values:{hours:100}});
 await pending;
 assert.equal(m.elements.metricKindTotals.children.length,0);
 assert.match(m.elements.metricKindStatus.textContent,/Ошибка расчёта/);
});

test('canceling JSON or XLSX reselection preserves the original reviewed path, format and token',async()=>{
 for(const original of ['json','xlsx']){
  const opts={};const m=mount(opts),e=m.elements;await m.window.metricKindWorkspaceReady;
  e.metricKindFrom.value='2026-11-18';e.metricKindTo.value='2026-11-18';
  await (original==='json'?e.metricKindChoose:e.metricKindChooseXlsx).events.click();
  const draft=e.metricKindPreview.children[0],file=e.metricKindFile.textContent;
  opts.cancel=true;opts.cancelXlsx=true;
  await e.metricKindChoose.events.click();await e.metricKindChooseXlsx.events.click();
  assert.equal(e.metricKindPreview.children[0],draft);assert.equal(e.metricKindFile.textContent,file);
  assert.equal(e.metricKindApply.disabled,false);
  assert.equal(m.calls.filter(c=>c[0]==='preview'||c[0]==='xlsx_preview').length,1);
  await e.metricKindApply.events.click();
  const writes=m.calls.filter(c=>c[0]==='apply'||c[0]==='xlsx_apply');
  assert.equal(writes.length,1);assert.equal(writes[0][0],original==='json'?'apply':'xlsx_apply');
  assert.equal(writes[0][1],original==='json'?'C:/Data/new.json':'C:/Data/hours.xlsx');assert.equal(writes[0].at(-1),'a'.repeat(64));
 }
});

test('empty calculation and import give first actions until a real result becomes stale',async()=>{
 let notify;
 const ui={revision:1,onSnapshot:fn=>{notify=fn;}};
 const m=mount({ui}),e=m.elements;
 await m.window.metricKindWorkspaceReady;
 notify(null);notify({});
 await e.metricKindFrom.events.change();
 assert.doesNotMatch(e.metricKindCount.textContent,/Повторите/);
 assert.doesNotMatch(e.metricKindStatus.textContent,/повторно/);
 assert.equal(e.metricKindApply.hidden,true);
 await e.metricKindCalculate.events.click();
 await e.metricKindTo.events.change();
 assert.match(e.metricKindCount.textContent,/Повторите/);
 m.window.sharedWorkspace={state:{mode:'shared',workspace_token:'shared',available:true,pending:false}};
 await e.metricKindChoose.events.click();
 assert.equal(e.metricKindApply.hidden,false);
 assert.equal(e.metricKindApply.textContent,'Сохранить в общую базу');
 notify(null);notify({});
 assert.match(e.metricKindStatus.textContent,/повторно/);
 assert.equal(e.metricKindApply.hidden,true);
});
