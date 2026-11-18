// Native-only SQLite workspace: no loading or saving full report JSON.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=String(text);return el;};
function status(id,text,error=false){$(id).textContent=String(text);$(id).className='status '+(error?'error':'muted');}
const reportSize=100,groupSize=50;
let revision=null,reportAfter=null,reportBack=[],reportNext=null,groupAfter=null,groupBack=[],groupNext=null;
let reportsBusy=false,groupsBusy=false,refreshBusy=false;
let analysisBusy=false,analysisReady=false,analysisCancelled=false,orgNames=[];
function controlState(){
$('reportBack').disabled=reportsBusy||refreshBusy||!reportBack.length;
$('reportNext').disabled=reportsBusy||refreshBusy||!reportNext;
$('nativeReportType').disabled=reportsBusy||refreshBusy;
$('groupBack').disabled=groupsBusy||refreshBusy||!groupBack.length;
$('groupNext').disabled=groupsBusy||refreshBusy||!groupNext;
$('workspaceRefresh').disabled=reportsBusy||groupsBusy||refreshBusy||analysisBusy;
$('analysisPrepare').disabled=analysisBusy||refreshBusy||revision===null;
$('analysisCancel').disabled=!analysisBusy;
$('analysisRun').disabled=analysisBusy||refreshBusy||!analysisReady;
$('analysisType').disabled=analysisBusy||refreshBusy;
$('analysisOrganization').disabled=analysisBusy||refreshBusy;
}
async function reports(cursor,reset=false){
if(reportsBusy||revision===null)return false;
reportsBusy=true;controlState();
try{
 const rows=await api.nativeReportPage(revision,{report_type:$('nativeReportType').value||null},cursor,reportSize);
 if(!Array.isArray(rows)||rows.length>reportSize)throw Error('Некорректная страница отчётов');
 if(reset)reportBack=[];
 reportAfter=cursor;
 reportNext=rows.length===reportSize?{start:rows.at(-1).period_start,id:rows.at(-1).id}:null;
 const tbody=$('reportRows');tbody.replaceChildren();
 for(const item of rows){const tr=make('tr');for(const value of [item.period_start,item.period_end,item.report_type,item.source_name||'—',item.id])tr.append(make('td',value??''));tbody.append(tr);}
 status('reportStatus','На странице: '+rows.length+' отчётов. Сортировка по дате и идентификатору.');
 return true;
}catch(err){reportNext=null;reportBack=[];status('reportStatus','Ошибка SQLite: '+String(err?.message||err)+'. Нажмите «Обновить данные».',true);return false;}
finally{reportsBusy=false;controlState();}
}
async function groups(cursor,reset=false){
if(groupsBusy||revision===null)return false;
groupsBusy=true;controlState();
try{
 const rows=await api.nativeGroupPage(revision,cursor,groupSize);
 if(!Array.isArray(rows)||rows.length>groupSize)throw Error('Некорректная страница групп');
 if(reset)groupBack=[];
 groupAfter=cursor;groupNext=rows.length===groupSize?rows.at(-1).handle:null;
 const tbody=$('groupRows');tbody.replaceChildren();
 for(const group of rows){
  const tr=make('tr');tr.append(make('td',group.id));
  const editor=make('input');editor.type='text';editor.value=group.name;editor.maxLength=1024;editor.setAttribute('aria-label','Новое имя группы '+group.id);
  const nameCell=make('td');nameCell.append(editor);tr.append(nameCell);tr.append(make('td',group.members));
  const save=make('button','Сохранить название');save.type='button';save.disabled=true;
  editor.addEventListener('input',()=>{save.disabled=editor.value===group.name||!editor.value.trim();});
  save.addEventListener('click',async()=>{
   const name=editor.value;if(!name.trim()||name===group.name)return;
   if(!window.confirm('Переименовать группу «'+group.name+'» в «'+name+'»?'))return;
   save.disabled=true;
   try{await api.nativeGroupRename(revision,group.handle,name);await refresh();status('workspaceStatus','Название группы изменено и сохранено в SQLite.');}
   catch(err){status('groupStatus','Группа не изменена: '+String(err?.message||err),true);save.disabled=false;}
  });
  const action=make('td');action.append(save);tr.append(action);tbody.append(tr);
 }
 status('groupStatus','На странице: '+rows.length+' групп. Данные отчётов не меняются.');
 return true;
}catch(err){groupNext=null;groupBack=[];status('groupStatus','Ошибка групп: '+String(err?.message||err),true);return false;}
finally{groupsBusy=false;controlState();}
}
async function refresh(){
if(refreshBusy||reportsBusy||groupsBusy||analysisBusy)return;
refreshBusy=true;controlState();
try{
 const snapshot=await api.nativeSettingsBootstrap();
 if(!Number.isSafeInteger(snapshot.revision)||snapshot.revision<0)throw Error('Ревизия SQLite слишком велика для JavaScript');
 revision=snapshot.revision;
 analysisReady=false;
 orgNames=snapshot.org_names.slice();
 const orgSelect=$('analysisOrganization');orgSelect.replaceChildren();
 const all=make('option','Все из настроек');all.value='*';orgSelect.append(all);
 for(const name of orgNames){const opt=make('option',name);opt.value=name;orgSelect.append(opt);}
 for(const [id,value] of [['metricReports',snapshot.reports],['metricIndexed',snapshot.indexed_reports],['metricGroups',snapshot.groups],['metricCatalog',snapshot.catalog]])$(id).textContent=value;
 status('workspaceStatus','Ревизия SQLite: '+revision+' · подразделения: '+snapshot.org_names.join(', ')+'.');
 reportAfter=null;reportNext=null;reportBack=[];groupAfter=null;groupNext=null;groupBack=[];
}catch(err){
 revision=null;$('reportRows').replaceChildren();$('groupRows').replaceChildren();
 reportNext=null;groupNext=null;reportBack=[];groupBack=[];
 status('workspaceStatus','Невозможно открыть SQL-режим: '+String(err?.message||err),true);return;
}finally{refreshBusy=false;controlState();}
await reports(null,true);await groups(null,true);
}
$('workspaceRefresh').addEventListener('click',()=>{void refresh();});
$('nativeReportType').addEventListener('change',()=>{reportAfter=null;reportBack=[];void reports(null,true);});
$('reportNext').addEventListener('click',async()=>{if(!reportNext)return;const before=reportAfter;if(await reports(reportNext))reportBack.push(before);controlState();});
$('reportBack').addEventListener('click',async()=>{if(!reportBack.length)return;const before=reportBack.at(-1);if(await reports(before))reportBack.pop();controlState();});
$('groupNext').addEventListener('click',async()=>{if(!groupNext)return;const before=groupAfter;if(await groups(groupNext))groupBack.push(before);controlState();});
$('groupBack').addEventListener('click',async()=>{if(!groupBack.length)return;const before=groupBack.at(-1);if(await groups(before))groupBack.pop();controlState();});

function analysisOrganizations(){
 const chosen=$('analysisOrganization').value;
 return chosen==='*'?orgNames.slice():[chosen];
}
async function prepareAnalysis(){
 if(analysisBusy||refreshBusy||revision===null)return;
 analysisBusy=true;analysisReady=false;analysisCancelled=false;controlState();
 const kind=$('analysisType').value;
 let processed=0,cursor=null;
 try{
   while(true){
     if(analysisCancelled)throw Error('Подготовка остановлена. Для продолжения запустите её заново.');
     const part=await api.nativeAnalysisIndexPage(revision,kind,cursor,20);
     if(!part||!Number.isSafeInteger(part.processed)||part.processed<0||part.processed>20)
       throw Error('Некорректная порция индексации');
     processed+=part.processed;
     status('analysisStatus','Индексировано отчётов: '+processed+'. Обработка идёт в SQLite.');
     if(part.done)break;
     if(!part.next||part.processed!==20)throw Error('Отсутствует курсор для продолжения');
     cursor=part.next;
   }
   if(analysisCancelled)throw Error('Подготовка остановлена');
   await api.nativeAnalysisIndexSettings(revision);
   analysisReady=true;
   status('analysisStatus','Индексы готовы: '+processed+' отчётов. Теперь выберите даты и нажмите «Рассчитать».');
 }catch(err){
   status('analysisStatus','Индексы не готовы: '+String(err?.message||err),true);
 }finally{analysisBusy=false;controlState();}
}
async function calculateAnalysis(){
 if(!analysisReady||analysisBusy||revision===null)return;
 const kind=$('analysisType').value,from=$('analysisFrom').value,to=$('analysisTo').value;
 const organizations=analysisOrganizations();
 if(!from||!to||from>to||organizations.some(x=>!x)||!organizations.length){
   status('analysisStatus','Укажите корректный диапазон дат и хотя бы одно подразделение.',true);return;
 }
 analysisBusy=true;controlState();
 try{
   const result=await api.nativeAnalysisRange(revision,kind,from,to,organizations);
   const total=result.total||{},number=value=>value===null||value===undefined?'—':Number(value).toLocaleString('ru-RU',{maximumFractionDigits:3});
   const metrics=[
     ['Полнота',result.complete?'Полная':'Есть пропуски'],
     ['Периодов',number(result.periods)],['Пропущено',number(result.missing)],
     ['Количество',number(total.count)],['Сумма',number(total.amount)],
     ['Среднее',number(total.average)],['Некорректных отчётов',number(result.invalid_reports)],
     ['Дубликатов',number(result.duplicate_issues)],['Перекрытий',number(result.overlap_issues)]
   ];
   const box=$('analysisValues');box.replaceChildren();
   for(const [label,value] of metrics){const item=make('div');item.className='metric';item.append(make('strong',value),make('span',label));box.append(item);}
   status('analysisStatus',result.complete?'Расчёт выполнен по полной последовательности периодов.':'Расчёт выполнен, но не все периоды подтверждены. Итоги скрыты до устранения пропусков.',!result.complete);
 }catch(err){
   $('analysisValues').replaceChildren();
   status('analysisStatus','Ошибка расчёта: '+String(err?.message||err),true);
 }finally{analysisBusy=false;controlState();}
}
const year=String(new Date().getFullYear());
$('analysisFrom').value=year+'-01-01';
$('analysisTo').value=year+'-12-31';
$('analysisType').addEventListener('change',()=>{
 analysisReady=false;$('analysisValues').replaceChildren();
 status('analysisStatus','Выбран другой тип периода. Подготовьте соответствующие индексы.');
 controlState();
});
$('analysisPrepare').addEventListener('click',prepareAnalysis);
$('analysisCancel').addEventListener('click',()=>{analysisCancelled=true;status('analysisStatus','Остановка после текущей порции…');});
$('analysisRun').addEventListener('click',calculateAnalysis);

controlState();
window.nativeWorkspaceReady=refresh();
})();
