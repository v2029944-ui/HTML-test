/* Canonical presentation with native, paged data capabilities only. */
(function(){
'use strict';
const O=window.unifiedOriginal;
if(!O||!window.desktopAPI)throw Error('Откройте приложение через Tauri.');
const {load:_load,save:_save,...native}=window.desktopAPI;
window.desktopAPI=Object.freeze(native);
const api=window.desktopAPI,$=O.$,state=O.state;
Object.defineProperty(state,'reports',{value:Object.freeze([]),writable:false,configurable:false});
let revision=null,snapshot=null,scope=null,generation=0,refreshing=null,cancelled=false;
const snapshots=new Set(),views=new Map(),scopes=new Set();
const el=O.el;
const status=el('div',null,'unified-load-status');status.id='unifiedIndexStatus';status.setAttribute('role','status');status.setAttribute('aria-live','polite');
const label=el('span','Открытие SQLite…');
const cancel=el('button','Остановить','button secondary');cancel.type='button';cancel.hidden=true;
const retry=el('button','Обновить данные','button secondary');retry.type='button';retry.hidden=true;
status.append(label,cancel,retry);document.querySelector('.dashboard-filter-panel').append(status);
cancel.addEventListener('click',()=>{cancelled=true;cancel.disabled=true;label.textContent='Остановка после текущей порции…';});
retry.addEventListener('click',()=>{void refresh().catch(error=>O.notice(error.message,true));});
function setPeriodInput(id,kind,date){
 const input=$(id);if(!input)return;
 const previous=input.value,oldType=input.dataset.periodType;
 input.type={day:'date',week:'week',month:'month',year:'number'}[kind]||'date';
 input.dataset.periodType=kind;
 input.min=kind==='year'?'1':'';input.max=kind==='year'?'9999':'';
 if(date!==undefined&&date!==null)input.value=window.nativePeriodSelection.fromIsoDate(date,kind);
 else if(oldType&&oldType!==kind){const iso=window.nativePeriodSelection.normalize(oldType,previous);input.value=iso?window.nativePeriodSelection.fromIsoDate(iso,kind):'';}
 // Programmatic value changes neither emit input events nor notify attribute observers.
 window.referenceCalendar?.syncField?.(input);
}
function activate(name){
 if(!['dashboard','changes','upload','groups','databases','storage'].includes(name))name='dashboard';
 const menu=$('managementNav'),returnFocus=menu.open&&menu.contains(document.activeElement);
 document.querySelectorAll('.tabs .tab').forEach(tab=>{const active=tab.dataset.view===name;tab.classList.toggle('active',active);if(active)tab.setAttribute('aria-current','page');else tab.removeAttribute('aria-current');});
 document.querySelectorAll('.view').forEach(view=>view.classList.toggle('active',view.id===name));
 $('managementToggle').classList.toggle('active',name==='upload'||name==='groups'||name==='databases'||name==='storage');
 menu.open=false;if(returnFocus)$('managementToggle').focus();
 for(const fn of views.get(name)||[])Promise.resolve().then(()=>fn()).catch(error=>O.notice(error.message||String(error),true));
 O.redrawCharts();return name;
}
function request(value,overrides={}){
 if(!value||value.revision!==revision)throw Error('Данные изменились. Повторите расчёт.');
 return {revision,source:value.source,displayType:value.displayType,organizations:value.organizations.slice(),selectedNames:value.allNames?[]:value.selectedNames.slice(),query:{method:'range',from:value.from,to:value.to,percent:30,min_count:5,min_amount:1000,...overrides}};
}
let helpId=0;
function tableHelp(section,table,headers,help){
 const note=el('div',null,'table-header-help muted small'),message=el('span'),close=el('button','Закрыть','text-button');
 note.id='table-header-help-'+(++helpId);note.hidden=true;
 message.setAttribute('role','status');close.type='button';note.append(message,' ',close);
 table.parentElement.before(note);
 let active=null;
 function hide(){
  if(!active)return;
  const opener=active;active=null;opener.setAttribute('aria-expanded','false');
  note.hidden=true;opener.focus();
 }
 close.addEventListener('click',hide);
 for(const [i,th] of Array.from(table.tHead.rows[0].cells).entries()){
  if(!help[i])continue;
  const button=el('button','?','table-help-button');
  button.type='button';button.title=help[i];button.setAttribute('aria-label','Пояснение: '+headers[i]);
  button.setAttribute('aria-controls',note.id);button.setAttribute('aria-expanded','false');
  button.addEventListener('click',()=>{
   if(active===button){hide();return;}
   active?.setAttribute('aria-expanded','false');active=button;
   message.textContent=headers[i]+'. '+help[i];note.hidden=false;button.setAttribute('aria-expanded','true');
  });
  th.append(button);
 }
 section.addEventListener('keydown',event=>{
  if(event.key!=='Escape'||event.defaultPrevented||!active)return;
  const nested=event.target.closest('details[open]');
  if(nested&&section.contains(nested))return;
  event.preventDefault();event.stopPropagation();hide();
 });
}
function table(root,metric,caption,headers,rows,options={}){
 const showHelp=options.headerHelp?.some(Boolean);
 O.makeTable(root,metric,caption,headers,rows,showHelp?{...options,headerHelp:[]}:options);
 const section=root.lastElementChild,table=section.querySelector('table');
 if(caption&&headers.length){
  table.caption.classList.add('sql-caption-source');
  const period=el('td',caption);
  period.colSpan=headers.length;
  const row=el('tr',null,'sql-table-period');
  row.setAttribute('aria-hidden','true');
  row.append(period);
  table.tHead.append(row);
 }
 if(showHelp)tableHelp(section,table,headers,options.headerHelp);
}
const originalCsv=O.tableCsvButton;
// Original makeTable calls this binding. Native exports capture the whole SQL query.
tableCsvButton=function(root,title,metric,caption,headers,rows,context,unit){
 if(context&&typeof context.export==='function'){
  const button=el('button','Скачать CSV','button secondary');button.type='button';
  button.addEventListener('click',async()=>{button.disabled=true;try{await context.export();}catch(error){O.notice(error.message||String(error),true);}finally{button.disabled=false;}});root.append(button);return;
 }
 originalCsv(root,title+' — текущая страница',metric,caption,headers,rows,Array.isArray(context)?context:null,unit);
};
async function notify(value){for(const listener of snapshots)await listener(value);}
async function refresh(){
 if(refreshing)return refreshing;
 const mine=++generation;cancelled=false;revision=null;snapshot=null;scope=null;
 cancel.hidden=false;cancel.disabled=false;retry.hidden=true;label.textContent='Открытие SQLite…';
 refreshing=(async()=>{
  await notify(null);
  try{
   const next=await api.nativeSettingsBootstrap();
   if(!Number.isSafeInteger(next.revision)||next.revision<0||!Array.isArray(next.org_names))throw Error('Некорректная ревизия SQLite.');
   const rev=next.revision;let processed=0;
   for(const kind of ['day','week','month']){
    let after=null;
    do{
     if(cancelled||mine!==generation)throw Error('Подготовка данных остановлена. Нажмите «Обновить данные», чтобы продолжить.');
     const part=await api.nativeAnalysisIndexPage(rev,kind,after,20);
     if(!part||!Number.isSafeInteger(part.processed)||part.processed<0||part.processed>20||!part.done&&(!part.next||part.processed!==20))throw Error('Некорректная порция подготовки данных.');
     processed+=part.processed;label.textContent='Подготовлено отчётов: '+processed;
     after=part.done?null:part.next;
    }while(after);
   }
   await api.nativeAnalysisIndexSettings(rev);
   if(cancelled||mine!==generation)throw Error('Подготовка данных остановлена.');
   state.settings.orgNames=next.org_names.slice();
   const selected=[...state.selectedOrganizations].filter(name=>next.org_names.includes(name));
   state.selectedOrganizations=new Set(selected.length?selected:next.org_names);
   revision=rev;snapshot=next;
   label.textContent=(window.sharedWorkspace?.state?.mode==='shared'?'Общая база SQLite':'Локальная база SQLite')+'. Отчётов: '+next.reports;
   cancel.hidden=true;retry.hidden=false;
   await notify(next);
   return next;
  }catch(error){
   revision=null;snapshot=null;scope=null;await notify(null);label.textContent=(window.sharedWorkspace?.state?.mode==='shared'?'Общая база недоступна. ':'Данные недоступны. ')+(error.message||String(error));cancel.hidden=true;retry.hidden=false;throw error;
  }finally{refreshing=null;}
 })();
 return refreshing;
}
// A new checkout can have the same SQLite revision as the previous workspace.
async function refreshWorkspace(){
 if(refreshing){
  ++generation;revision=null;snapshot=null;scope=null;
  await refreshing.catch(()=>{ /* The superseded read can reject its obsolete token. */ });
 }
 return refresh();
}
function init(){
 O.initTheme();O.initChartTooltip();
 document.querySelectorAll('.tabs .tab').forEach(tab=>tab.addEventListener('click',()=>activate(tab.dataset.view)));
 $('reportsShortcut').addEventListener('click',()=>activate('upload'));
 $('sampleBtn').addEventListener('click',()=>{activate('upload');$('excelFile').click();});
 document.addEventListener('click',event=>{const menu=$('managementNav');if(menu.open&&!menu.contains(event.target))menu.open=false;});
 document.addEventListener('keydown',event=>{if(event.key==='Escape'&&$('managementNav').open){$('managementNav').open=false;$('managementToggle').focus();}});
 activate('dashboard');
}
const U={api,state,$,el,notice:O.notice,fmt:O.fmt,table,tableHelp,drawChart:O.drawChart,drawDepartments:O.drawDepartments,setPeriodInput,range:(...args)=>window.nativePeriodSelection.range(...args),request,refresh,refreshWorkspace,activate,init,
 get revision(){return revision;},get snapshot(){return snapshot;},get scope(){return scope;},get generation(){return generation;},
 onSnapshot(fn){snapshots.add(fn);return()=>snapshots.delete(fn);},onView(name,fn){if(!views.has(name))views.set(name,new Set());views.get(name).add(fn);},
 onScope(fn){scopes.add(fn);},publishScope(value){scope=value;for(const fn of scopes)fn(value);},isCurrent(value){return revision!==null&&value===revision;}};
window.unifiedUI=Object.freeze(U);
window.nativeWorkspaceRefresh=()=>refresh().then(()=>true,()=>false);
})();
