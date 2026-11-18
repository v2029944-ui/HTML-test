// Native-only SQLite workspace: no loading or saving full report JSON.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=String(text);return el;};
function status(id,text,error=false){$(id).textContent=String(text);$(id).className='status '+(error?'error':'muted');}
const reportSize=100,groupSize=50;
let revision=null,reportAfter=null,reportBack=[],reportNext=null,groupAfter=null,groupBack=[],groupNext=null;
let reportsBusy=false,groupsBusy=false,refreshBusy=false;
function controlState(){
$('reportBack').disabled=reportsBusy||refreshBusy||!reportBack.length;
$('reportNext').disabled=reportsBusy||refreshBusy||!reportNext;
$('nativeReportType').disabled=reportsBusy||refreshBusy;
$('groupBack').disabled=groupsBusy||refreshBusy||!groupBack.length;
$('groupNext').disabled=groupsBusy||refreshBusy||!groupNext;
$('workspaceRefresh').disabled=reportsBusy||groupsBusy||refreshBusy;
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
if(refreshBusy||reportsBusy||groupsBusy)return;
refreshBusy=true;controlState();
try{
 const snapshot=await api.nativeSettingsBootstrap();
 if(!Number.isSafeInteger(snapshot.revision)||snapshot.revision<0)throw Error('Ревизия SQLite слишком велика для JavaScript');
 revision=snapshot.revision;
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
controlState();
window.nativeWorkspaceReady=refresh();
})();
