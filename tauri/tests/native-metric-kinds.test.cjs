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
 const window={desktopAPI:api,confirm:()=>{confirmations++;return !opts.decline;},analizUserError:userError};
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
test('packaging includes registry viewer without altering standalone HTML',()=>{
 const out=fs.mkdtempSync(path.join(os.tmpdir(),'metric-ui-'));
 try{
  prepare({base:path.resolve(__dirname,'../..'),output:out});
  const html=fs.readFileSync(path.join(out,'native-workspace.html'),'utf8');
  assert.match(html,/tauri-native-metric-kinds.js/);
  assert.ok(fs.existsSync(path.join(out,'tauri-native-metric-kinds.js')));
  assert.ok(!fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8').includes('metricKindSelect'));
 }finally{fs.rmSync(out,{recursive:true,force:true});}
});

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
