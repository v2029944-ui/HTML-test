// Native-only SQLite workspace: no loading or saving full report JSON.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
const userError=window.analizUserError||(()=>'Операция не выполнена. Перезапустите программу и повторите действие.');
const periodPicker=window.nativePeriodSelection;
const selectedNames=()=>window.nativeNameFilter?.selectedNames()??[];
const nameArgs=(names,...grouping)=>[names.length===1?names[0]:null,...grouping,names.length>1?names:null];
const describeNames=names=>names.length?names.join(', '):'все';
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=String(text);return el;};
function status(id,text,error=false){$(id).textContent=String(text);$(id).className='status '+(error?'error':'muted');}
const reportSize=100,groupSize=50;
let reportRange=null;
let revision=null,reportAfter=null,reportBack=[],reportNext=null,groupAfter=null,groupBack=[],groupNext=null;
let reportsBusy=false,groupsBusy=false,refreshBusy=false;
let queuedRefresh=null,refreshScheduled=false;
let analysisBusy=false,analysisReady=false,analysisCancelled=false,orgNames=[];
let workBusy=false,workScope=null,workAfter=null,workBack=[],workNext=null;
let exportBusy=false,backupBusy=false,restoreBusy=false,restoreSelection=null;
const workSize=50;
let workTrendSelection=null,workTrendRequest=0,workTrendBusy=false;
function invalidateWorkTrend(message='Выберите работу или группу из таблицы выше.'){
 workTrendRequest++;workTrendBusy=false;
 workTrendSelection=null;
 window.nativeWorkTrends?.clear(message);
}
let organizationTrendsRequest=0;
function invalidateOrganizationTrends(message='Динамика подразделений появится после расчёта.'){
 organizationTrendsRequest++;
 window.nativeOrganizationTrends?.clear(message);
}
let comparisonBusy=false,comparisonRequest=0,comparisonBaseline=null;
function invalidateComparison(message='Укажите базовые даты и сравните периоды.'){
 comparisonRequest++;comparisonBaseline=null;
 updateComparisonContext();
 window.nativeComparison?.clear(message);
}
function updateComparisonContext(){
 const scope=workScope;
 $('comparisonCurrentContext').textContent=scope
  ?(comparisonBaseline?'База: '+comparisonBaseline.from+' — '+comparisonBaseline.to+'; ':'')+'Текущий: '+scope.from+' — '+scope.to+' · '+scope.type+' · подразделения: '+scope.organizations.join(', ')+(scope.selectedNames.length?' · работы/группы: '+describeNames(scope.selectedNames):'')
  :'Текущий диапазон появится после расчёта аналитики.';
}
let rankingRequest=0,rankingBusy=false;
function invalidateRanking(message='Рейтинг появится после расчёта.'){
 rankingRequest++;rankingBusy=false;
 window.nativeRanking?.clear(message);
}
let timelineRequest=0;
function invalidateTimeline(message){
 timelineRequest++;
 window.nativeTimeline?.clear(message);
}
// Acknowledgement is display-only, scoped to the SQLite revision and exact
// period/organizations/issue counts. It NEVER changes SQL completeness.
let coverageKey=null,coverageResult=null,coverageScope=null;
function clearAnalysisCoverage(){
 coverageKey=null;coverageResult=null;coverageScope=null;
 $('analysisCoverageNotice').hidden=true;
 $('analysisCoverageText').textContent='';
}
function coverageAccepted(){
 if(!coverageKey)return false;
 try{return window.localStorage?.getItem(coverageKey)==='accepted';}
 catch(_){return false;}
}
function displayAnalysisCoverage(result,scope){
 coverageResult=result;coverageScope=scope;
 const available=Number.isSafeInteger(result.available_reports)&&result.available_reports>0&&
  Number.isFinite(result.available_total?.count)&&Number.isFinite(result.available_total?.amount);
 const missing=!result.complete;
 const invalid=(Number(result.invalid_reports)||0)+(Number(result.duplicate_issues)||0)+
  (Number(result.overlap_issues)||0)+(Number(result.import_warning_reports)||0);
 const flagged=missing||invalid>0;
 coverageKey=flagged?'sql-coverage-ack-v1:'+JSON.stringify([
  revision,scope.type,scope.from,scope.to,scope.organizations.slice().sort(),scope.selectedNames,
  result.periods,result.missing,result.invalid_reports,result.duplicate_issues,
  result.overlap_issues,result.import_warning_reports,result.available_reports
 ]):null;
 const accepted=coverageAccepted();
 const number=value=>typeof value==='number'&&Number.isFinite(value)
  ?value.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
 const partial=missing&&available;
 const total=partial?result.available_total:result.total||{};
 const metrics=[
  ['Полнота',result.complete?'Полная':accepted?'Неполная (известно)':'Есть пропуски'],
  ['Периодов',number(result.periods)],['Пропущено',number(result.missing)],
  [partial?'Количество / объём (имеющиеся данные)':'Количество / объём',number(total.count)],
  [partial?'Сумма (имеющиеся данные)':'Сумма',number(total.amount)],
  [partial?'Среднее (имеющиеся данные)':'Среднее',number(total.average)],
  ['Некорректных отчётов',number(result.invalid_reports)],
  ['Дубликатов',number(result.duplicate_issues)],
  ['Перекрытий',number(result.overlap_issues)]
 ];
 const box=$('analysisValues');box.replaceChildren();
 for(const [label,value] of metrics){
  const item=make('div');item.className='metric';item.append(make('strong',value),make('span',label));box.append(item);
 }
 $('analysisCoverageNotice').hidden=!flagged||accepted;
 $('analysisCoverageText').textContent=
  'Данные неполные или содержат замечания. Пропусков: '+number(result.missing)+
  ', некорректных: '+number(result.invalid_reports)+
  ', дубликатов: '+number(result.duplicate_issues)+
  ', перекрытий: '+number(result.overlap_issues)+
  '. '+(partial?'Сумма, количество и среднее относятся только к имеющимся отчётам ('+
   number(result.available_reports)+'). ':available?'':'Подтверждённых исходных отчётов нет. ')+
  'Подтверждение скрывает это предупреждение для неизменённой ревизии и выборки, но не исправляет отчёты и не подтверждает отсутствующие периоды.';
 const message=result.complete
  ?'Расчёт выполнен по полной последовательности периодов.'
  :partial
   ?'Расчёт выполнен по имеющимся данным. Полный итог за выбранный диапазон неизвестен.'
   :'Расчёт выполнен, но не все периоды подтверждены. Итоги скрыты до устранения пропусков.';
 status('analysisStatus',message+(accepted?' Замечание подтверждено для этой выборки.':''),flagged&&!accepted);
}
function acknowledgeCoverage(){
 if(!coverageKey||!coverageResult||!coverageScope)return;
 try{
  if(!window.localStorage)throw Error('No local storage');
  window.localStorage.setItem(coverageKey,'accepted');
 }catch(_){
  status('analysisStatus','Подтверждение не удалось сохранить. Проверьте настройки хранилища приложения.',true);
  return;
 }
 displayAnalysisCoverage(coverageResult,coverageScope);
}
function controlState(){
$('reportPageInfo').textContent='Страница '+(reportBack.length+1);
$('groupPageInfo').textContent='Страница '+(groupBack.length+1);
$('workPageInfo').textContent='Страница '+(workBack.length+1);
$('reportBack').disabled=reportsBusy||refreshBusy||!reportBack.length;
$('reportNext').disabled=reportsBusy||refreshBusy||!reportNext;
$('nativeReportType').disabled=reportsBusy||refreshBusy;
$('reportClearSelection').disabled=reportsBusy||refreshBusy;
$('groupBack').disabled=groupsBusy||refreshBusy||!groupBack.length;
$('groupNext').disabled=groupsBusy||refreshBusy||!groupNext;
$('workspaceRefresh').disabled=reportsBusy||groupsBusy||refreshBusy||analysisBusy||workBusy||exportBusy||backupBusy||restoreBusy;
$('backupSqlite').disabled=backupBusy||exportBusy||analysisBusy||workBusy||refreshBusy||restoreBusy;
$('restoreChoose').disabled=restoreBusy||backupBusy||analysisBusy||exportBusy||refreshBusy;
$('restoreCopy').disabled=restoreBusy||!restoreSelection||backupBusy||analysisBusy||exportBusy||refreshBusy;
$('restoreSchedule').disabled=restoreBusy||!restoreSelection||backupBusy||analysisBusy||exportBusy||refreshBusy;
$('restoreCancel').disabled=restoreBusy||backupBusy||analysisBusy||exportBusy||refreshBusy;
$('analysisPrepare').disabled=analysisBusy||refreshBusy||exportBusy||revision===null;
$('analysisCancel').disabled=!analysisBusy;
$('analysisRun').disabled=analysisBusy||refreshBusy||workBusy||exportBusy||!analysisReady;
$('analysisPrint').disabled=revision===null||analysisBusy||refreshBusy||workBusy||comparisonBusy||rankingBusy||workTrendBusy||exportBusy||!!queuedRefresh||window.nativeAdvancedAnalysis?.isBusy()||!workScope;
$('changesPrint').disabled=$('analysisPrint').disabled;
$('analysisExport').disabled=analysisBusy||refreshBusy||workBusy||exportBusy||!workScope;
$('rankingMetric').disabled=analysisBusy||refreshBusy||!workScope;
$('rankingCategory').disabled=analysisBusy||refreshBusy||!workScope;
$('comparisonRun').disabled=analysisBusy||refreshBusy||comparisonBusy||!workScope||revision===null||
 !$('comparisonBeforeFrom').value||!$('comparisonBeforeTo').value;
$('workTrendMetric').disabled=analysisBusy||refreshBusy||!workTrendSelection;
$('workBack').disabled=workBusy||analysisBusy||refreshBusy||!workBack.length;
$('workNext').disabled=workBusy||analysisBusy||refreshBusy||!workNext;
$('analysisType').disabled=analysisBusy||refreshBusy||workBusy||exportBusy;
$('analysisSource').disabled=analysisBusy||refreshBusy||workBusy||exportBusy;
const organizationLocked=revision===null||analysisBusy||refreshBusy||workBusy||exportBusy||comparisonBusy||backupBusy||restoreBusy;
if(window.nativeOrganizationFilter)window.nativeOrganizationFilter.setLocked(organizationLocked);
else $('analysisOrganization').disabled=organizationLocked;
$('analysisFrom').disabled=analysisBusy||refreshBusy||workBusy||exportBusy;
$('analysisTo').disabled=analysisBusy||refreshBusy||workBusy||exportBusy;
window.nativeNameFilter?.setLocked(analysisBusy||refreshBusy||workBusy||exportBusy||comparisonBusy);
// Mutations may commit while an unrelated bounded read is still running.
// Defer their refresh instead of losing the request and keeping old revisions.
if(queuedRefresh&&!refreshScheduled&&refreshIdle()){
 refreshScheduled=true;
 void Promise.resolve().then(async()=>{
  refreshScheduled=false;
  if(!queuedRefresh||!refreshIdle())return;
  const request=queuedRefresh;queuedRefresh=null;
  try{request.resolve(await refresh());}catch(error){request.resolve(false);}
 });
}

}
async function reports(cursor,reset=false){
if(reportsBusy||revision===null)return false;
reportsBusy=true;controlState();
try{
 const expected=revision;
 const filter={report_type:$('nativeReportType').value||null,...(reportRange?{start:reportRange.from}:{})};
 const pageRows=await api.nativeReportPage(expected,filter,cursor,reportSize);
 // SQL orders bounded headers by start date. Apply the selected start-date
 // ceiling here: the generic read filter's end constrains period_end and
 // would incorrectly hide a week/month beginning on the selected day.
 if(!Array.isArray(pageRows)||pageRows.length>reportSize)throw Error('Некорректная страница отчётов');
 const rows=reportRange?pageRows.filter(item=>item.period_start>=reportRange.from&&item.period_start<=reportRange.to):pageRows;
 if(!Array.isArray(rows)||rows.length>reportSize)throw Error('Некорректная страница отчётов');
 if(reset)reportBack=[];
 reportAfter=cursor;
 reportNext=rows.length===reportSize?{start:rows.at(-1).period_start,id:rows.at(-1).id}:null;
 const tbody=$('reportRows');tbody.replaceChildren();
 for(const item of rows){
  const tr=make('tr');tr.setAttribute('data-report-id',item.id);
  const periodNames={day:'День',week:'Неделя',month:'Месяц'};
  const date=value=>/^\d{4}-\d{2}-\d{2}$/.test(value)?value.split('-').reverse().join('.'):value;
  for(const value of [date(item.period_start),date(item.period_end),periodNames[item.report_type]||item.report_type,item.source_name||'—'])
   tr.append(make('td',value??''));
  const action=make('td');
  window.nativeReportQuality?.addReportAction(action,item,expected);
  window.nativeManagement?.addReportAction(action,item,expected);
  tr.append(action);tbody.append(tr);
 }
 status('reportStatus','На странице: '+rows.length+' отчётов.');
 $('reportSelection').textContent=reportRange?'Даты начала отчётов: '+reportRange.from+' — '+reportRange.to:'Все загруженные периоды';
 $('reportClearSelection').hidden=!reportRange;
 void window.nativeReportCalendar?.setContext(expected,pageRows[0]?.period_start);
 return true;
}catch(err){window.nativeReportCalendar?.setContext(null);$('reportRows').replaceChildren();reportNext=null;reportBack=[];status('reportStatus','Ошибка SQLite: '+userError(err)+'. Нажмите «Обновить данные».',true);return false;}
finally{reportsBusy=false;controlState();}
}
async function groups(cursor,reset=false){
if(groupsBusy||revision===null)return false;
groupsBusy=true;controlState();
try{
 const expected=revision;
 const rows=await api.nativeGroupPage(expected,cursor,groupSize);
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
   const name=editor.value;if(!name.trim()||name===group.name||expected!==revision||window.nativeManagement?.isBusy())return;
   if(!window.confirm('Переименовать группу «'+group.name+'» в «'+name+'»?'))return;
   save.disabled=true;
   try{await api.nativeGroupRename(expected,group.handle,name);await requestRefresh();status('workspaceStatus','Название группы изменено и сохранено в SQLite.');}
   catch(err){status('groupStatus','Группа не изменена: '+userError(err),true);save.disabled=false;}
  });
  const action=make('td');action.append(save);
  window.nativeManagement?.addGroupActions(action,group,expected);
  tr.append(action);tbody.append(tr);
 }
 status('groupStatus','На странице: '+rows.length+' групп. Данные отчётов не меняются.');
 return true;
}catch(err){groupNext=null;groupBack=[];status('groupStatus','Ошибка групп: '+userError(err),true);return false;}
finally{groupsBusy=false;controlState();}
}
function refreshIdle(){
 return !(refreshBusy||reportsBusy||groupsBusy||analysisBusy||workBusy||exportBusy||backupBusy||restoreBusy);
}
function requestRefresh(){
 if(queuedRefresh)return queuedRefresh.promise;
 let resolve;
 const promise=new Promise(done=>{resolve=done;});
 queuedRefresh={promise,resolve};
 status('workspaceStatus','База изменена. Обновление выполнится после завершения текущих операций.');
 controlState();
 return promise;
}
async function refresh(){
if(refreshBusy||reportsBusy||groupsBusy||analysisBusy||workBusy||exportBusy||backupBusy||restoreBusy)return false;
refreshBusy=true;controlState();
try{
 const snapshot=await api.nativeSettingsBootstrap();
 if(!Number.isSafeInteger(snapshot.revision)||snapshot.revision<0)throw Error('Ревизия SQLite слишком велика для JavaScript');
 revision=snapshot.revision;
 window.nativeReportQuality?.setRevision(revision);
 void window.nativeManagement?.setContext(snapshot);
 window.nativeNameFilter?.setRevision(revision);
 analysisReady=false;
 invalidateTimeline('Данные обновлены. Повторите расчёт графика.');
 invalidateRanking('Данные обновлены. Повторите расчёт рейтинга.');
 invalidateOrganizationTrends('Данные обновлены. Повторите расчёт динамики подразделений.');
 $('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearAnalysisCoverage();clearWork();
 status('divisionShareStatus','Доли появятся после полного подтверждённого расчёта.');
 orgNames=snapshot.org_names.slice();
 if(window.nativeOrganizationFilter)window.nativeOrganizationFilter.setOrganizations(orgNames);
 else{
  const orgSelect=$('analysisOrganization'),previousOrganization=orgSelect.value;
  orgSelect.replaceChildren();
  const all=make('option','Все из настроек');all.value='*';orgSelect.append(all);
  for(const name of orgNames){const opt=make('option',name);opt.value=name;orgSelect.append(opt);}
  orgSelect.value=orgNames.includes(previousOrganization)?previousOrganization:'*';
 }
 updateFilterContext();
 window.nativeDailyCalendar?.setContext(revision,analysisOrganizations(),selectedNames());
 $('analysisResultScope').textContent='Данные обновлены. Подготовьте индексы и повторите расчёт для выбранных фильтров.';
 for(const [id,value] of [['metricReports',snapshot.reports],['metricIndexed',snapshot.indexed_reports],['metricGroups',snapshot.groups],['metricCatalog',snapshot.catalog]])$(id).textContent=value;
 status('workspaceStatus','Данные обновлены'+' · подразделения: '+snapshot.org_names.join(', ')+'.');
 reportAfter=null;reportNext=null;reportBack=[];groupAfter=null;groupNext=null;groupBack=[];
 const reportsLoaded=await reports(null,true);
 const groupsLoaded=await groups(null,true);
 return reportsLoaded&&groupsLoaded;
}catch(err){
 revision=null;window.nativeReportCalendar?.setContext(null);window.nativeReportQuality?.setRevision(null);void window.nativeManagement?.setContext(null);window.nativeNameFilter?.setRevision(null);$('reportRows').replaceChildren();$('groupRows').replaceChildren();
 analysisReady=false;$('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearAnalysisCoverage();clearWork('Результаты недоступны. Обновите соединение с базой.');
 invalidateTimeline('Результаты недоступны до обновления базы.');invalidateRanking('Результаты недоступны до обновления базы.');invalidateOrganizationTrends('Результаты недоступны до обновления базы.');
 reportNext=null;groupNext=null;reportBack=[];groupBack=[];
 window.nativeDailyCalendar?.setContext(null,[],null);
 status('workspaceStatus','Невозможно открыть SQLite: '+userError(err),true);
 $('analysisResultScope').textContent='Результаты недоступны до восстановления соединения с базой.';
 return false;
}finally{refreshBusy=false;controlState();}
}

async function backupSqlite(){
 if(backupBusy||analysisBusy||exportBusy||refreshBusy||workBusy||restoreBusy)return;
 backupBusy=true;controlState();
 try{
  const now=new Date().toISOString().slice(0,10);
  const destination=await api.chooseSqliteBackupPath('analiz-backup-'+now+'.sqlite3');
  if(destination===null||destination===undefined||destination===''){
   status('backupStatus','Резервное копирование отменено.');return;
  }
  if(typeof destination!=='string')throw Error('Некорректный путь резервной копии');
  status('backupStatus','Создаётся согласованная копия SQLite…');
  const captured=await api.nativeBackupSqlite(destination);
  status('backupStatus','Копия SQLite сохранена. Ревизия снимка: '+captured+'. Оригинальная база не изменена.');
 }catch(err){
  status('backupStatus','Не удалось сохранить копию SQLite: '+userError(err)+'. Выберите новое имя и проверьте свободное место.',true);
 }finally{backupBusy=false;controlState();}
}
const restoreError=err=>{
 const raw=typeof err==='string'?err:typeof err?.message==='string'?err.message:'';
 if(typeof window.analizUserError==='function')return window.analizUserError(err);
 return /[А-Яа-яЁё]/.test(raw) && !/SELECT|SQLITE_ERROR|traceback|stack trace/i.test(raw)
  ?raw:'Не удалось проверить резервную копию. Проверьте файл и повторите попытку.';
};
async function inspectBackup(){
 if(restoreBusy||backupBusy||analysisBusy||exportBusy||refreshBusy)return;
 restoreSelection=null;restoreBusy=true;controlState();
 $('restoreSelected').textContent='Резервная копия не выбрана.';
 try{
  const source=await api.chooseRestoreBackupPath();
  if(!source){status('restoreStatus','Выбор резервной копии отменён.');return;}
  if(typeof source!=='string')throw Error('Выберите один файл с расширением .sqlite3.');
  status('restoreStatus','Проверяем целостность SQLite, отчёты и настройки…');
  const preview=await api.nativeRestorePreview(source);
  if(!preview||typeof preview.verification!=='string'||
     !/^[a-f0-9]{64}$/i.test(preview.verification)||
     !Number.isSafeInteger(preview.reportCount)||preview.reportCount<0)
    throw Error('Не удалось подтвердить целостность резервной копии.');
  restoreSelection={source,verification:preview.verification};
  $('restoreSelected').textContent='Проверенный файл: '+source.split(/[\\/]/).pop();
  status('restoreStatus','Копия проверена. Отчётов: '+preview.reportCount+
    '. Можно создать отдельный восстановленный файл. Рабочая база не изменена.');
 }catch(err){restoreSelection=null;status('restoreStatus',restoreError(err),true);}
 finally{restoreBusy=false;controlState();}
}
async function writeRestoreCopy(){
 if(restoreBusy||!restoreSelection||backupBusy||analysisBusy||exportBusy||refreshBusy)return;
 if(!window.confirm('Создать отдельную восстановленную копию? Рабочая база SQLite не изменится.'))return;
 const selected=restoreSelection;
 restoreBusy=true;controlState();
 try{
  const destination=await api.chooseRestoreDestination('analiz-restored-check.sqlite3');
  if(!destination){status('restoreStatus','Создание восстановленной копии отменено.');return;}
  if(typeof destination!=='string')throw Error('Укажите новый файл с расширением .sqlite3.');
  status('restoreStatus','Создаём проверенную отдельную копию без замены рабочей базы…');
  const result=await api.nativeRestoreCopy(selected.source,selected.verification,destination);
  if(result?.verification!==selected.verification)throw Error('Результат проверки копии не совпал.');
  status('restoreStatus','Восстановленная копия создана и проверена. Отчётов: '+
    result.reportCount+'. Рабочая база не изменена. Не подменяйте её вручную при открытой программе.');
 }catch(err){status('restoreStatus',restoreError(err),true);}
 finally{restoreBusy=false;controlState();}
}
async function scheduleRestoreAtStartup(){
 if(restoreBusy||!restoreSelection||backupBusy||analysisBusy||exportBusy||refreshBusy)return;
 if(!window.confirm('При следующем запуске будет заменена текущая рабочая база SQLite на выбранную резервную копию. Программа заранее сохранит защитные копии текущей базы. Продолжить?'))return;
 restoreBusy=true;controlState();
 try{
  const selected=restoreSelection;
  status('restoreStatus','Подготавливаем отдельную проверенную копию для следующего запуска…');
  const request=await api.nativeRestoreSchedule(selected.source,selected.verification);
  if(!request?.restartRequired)throw Error('Не удалось запланировать восстановление.');
  restoreSelection=null;
  status('restoreStatus','Восстановление запланировано: '+request.reportCount+
    ' отчётов. Полностью закройте приложение и откройте снова. До перезапуска рабочая база не изменена.');
 }catch(err){status('restoreStatus',restoreError(err),true);}
 finally{restoreBusy=false;controlState();}
}
async function cancelScheduledRestore(){
 if(restoreBusy||backupBusy||analysisBusy||exportBusy||refreshBusy)return;
 if(!window.confirm('Отменить запланированное восстановление? Рабочая база не изменится.'))return;
 restoreBusy=true;controlState();
 try{
  const removed=await api.nativeRestoreCancel();
  restoreSelection=null;
  status('restoreStatus',removed?
    'Отложенное восстановление отменено. Рабочая база сохранена.':
    'Запланированных восстановлений нет. Рабочая база не изменена.');
 }catch(err){status('restoreStatus',restoreError(err),true);}
 finally{restoreBusy=false;controlState();}
}
$('restoreCancel').addEventListener('click',cancelScheduledRestore);
$('restoreSchedule').addEventListener('click',scheduleRestoreAtStartup);
$('restoreChoose').addEventListener('click',inspectBackup);
$('restoreCopy').addEventListener('click',writeRestoreCopy);
$('workspaceRefresh').addEventListener('click',()=>{void refresh();});
$('backupSqlite').addEventListener('click',backupSqlite);
$('reportClearSelection').addEventListener('click',async()=>{
 if(reportsBusy||refreshBusy)return false;reportRange=null;reportAfter=null;reportBack=[];return reports(null,true);
});
window.nativeReportList=Object.freeze({selectRange:async(from,to)=>{
 const valid=value=>typeof value==='string'&&/^\d{4}-\d{2}-\d{2}$/.test(value)&&Number(value.slice(0,4))>0&&Number.isFinite(Date.parse(value+'T12:00:00Z'))&&new Date(value+'T12:00:00Z').toISOString().slice(0,10)===value;
 if(!valid(from)||!valid(to)||from>to||reportsBusy||refreshBusy)return false;
 reportRange={from,to};$('nativeReportType').value='';reportAfter=null;reportBack=[];
 $('uploadedReports').open=true;
 const ok=await reports(null,true);
 if(ok)$('uploadedReports').scrollIntoView?.({block:'start',behavior:'auto'});
 return ok;
}});
$('nativeReportType').addEventListener('change',()=>{reportAfter=null;reportBack=[];void reports(null,true);});
$('reportNext').addEventListener('click',async()=>{if(!reportNext)return;const before=reportAfter;if(await reports(reportNext))reportBack.push(before);controlState();});
$('reportBack').addEventListener('click',async()=>{if(!reportBack.length)return;const before=reportBack.at(-1);if(await reports(before))reportBack.pop();controlState();});
$('groupNext').addEventListener('click',async()=>{if(!groupNext)return;const before=groupAfter;if(await groups(groupNext))groupBack.push(before);controlState();});
$('groupBack').addEventListener('click',async()=>{if(!groupBack.length)return;const before=groupBack.at(-1);if(await groups(before))groupBack.pop();controlState();});

function analysisOrganizations(){
 if(window.nativeOrganizationFilter)return window.nativeOrganizationFilter.selected();
 const chosen=$('analysisOrganization').value;
 return chosen==='*'?orgNames.slice():orgNames.includes(chosen)?[chosen]:[];
}
function describeOrganizations(names){
 if(!names.length)return 'ничего не выбрано';
 if(names.length===orgNames.length)return 'все из настроек ('+names.length+')';
 return names.join(', ')+' ('+names.length+' из '+orgNames.length+')';
}
function updateFilterContext(){
 $('analysisFilterScope').textContent='Подразделения: '+describeOrganizations(analysisOrganizations())+
  '. Работы и группы: '+describeNames(selectedNames())+
  '. Один отбор: итоги, графики, сравнение, детализация и CSV.';
}
function updateResultContext(type,from,to,organizations,names,source){
 $('analysisResultScope').textContent='Выборка: '+type+', '+from+' — '+to+
   '; источник: '+(source==='daily'?'только дневные отчёты':'отчёты выбранного типа')+'; подразделения: '+describeOrganizations(organizations)+
   '. Работы и группы: '+describeNames(names)+
   '. CSV охватывает весь выбранный набор, а не только показанные 50 строк.';
}
async function prepareAnalysis(){
 if(analysisBusy||refreshBusy||revision===null)return;
 analysisBusy=true;analysisReady=false;analysisCancelled=false;
invalidateTimeline('Индексация данных. Рассчитайте график после подготовки.');
invalidateRanking('Индексация данных. Повторите расчёт рейтинга после подготовки.');
invalidateOrganizationTrends('Динамика подразделений станет доступна после индексации.');
$('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearWork();
 status('divisionShareStatus','Доли появятся после полного подтверждённого расчёта.');
controlState();
 const kind=$('analysisSource').value==='daily'?'day':$('analysisType').value;
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
   status('analysisStatus','Индексы не готовы: '+userError(err),true);
 }finally{analysisBusy=false;controlState();}
}

function showDivisionComparison(rows,rangeTotal,rangeComplete){
 const tbody=$('divisionRows');tbody.replaceChildren();
 if(!Array.isArray(rows)||rows.length>100)throw Error('Некорректный размер SQL-сравнения');
 const finite=x=>typeof x==='number'&&Number.isFinite(x);
 const format=value=>value===null||value===undefined?'—':Number(value).toLocaleString('ru-RU',{maximumFractionDigits:3});
 const sorted=rows.slice().sort((a,b)=>{
   const av=a.complete&&finite(a.total?.amount)?a.total.amount:null;
   const bv=b.complete&&finite(b.total?.amount)?b.total.amount:null;
   if(av===null)return bv===null?String(a.org).localeCompare(String(b.org),'ru'):1;
   if(bv===null)return -1;
   return bv-av||String(a.org).localeCompare(String(b.org),'ru');
 });
 const maximum=Math.max(0,...sorted.map(item=>
     item.complete&&finite(item.total?.amount)?Math.abs(item.total.amount):0));
 // Share is meaningful only when the complete selected SQL range agrees
 // with the sum of complete non-negative organization totals.
 const completeAmounts=sorted.every(item=>item.complete===true&&
   finite(item.total?.amount)&&item.total.amount>=0);
 const divisionsSum=sorted.reduce((total,item)=>total+
   (finite(item.total?.amount)?item.total.amount:0),0);
 const shareEnabled=rangeComplete===true&&completeAmounts&&
   finite(rangeTotal)&&rangeTotal>0&&
   Math.abs(divisionsSum-rangeTotal)<=Math.max(.000001,Math.abs(rangeTotal)*1e-8);
 status('divisionShareStatus',shareEnabled
   ?'Доли от подтверждённой суммы выбранных подразделений; состав выборки указан выше.'
   :'Доли скрыты: ряд неполный, сумма не положительна или итоги подразделений не совпадают с итогом выбранного диапазона.');
 for(const item of sorted){
   const tr=make('tr');
   const complete=item.complete===true;
   const sum=complete&&finite(item.total?.amount)?item.total.amount:null;
   tr.append(make('td',item.org??''),make('td',complete?'Полная':'Неполная'),
       make('td',format(item.missing)),
       make('td',complete?format(item.total?.count):'—'),
       make('td',sum===null?'—':format(sum)));
   const graph=make('td');
   if(sum===null){
     graph.append(make('span','Нет полного ряда'));
   }else{
     const track=make('div');track.className='division-track';
     track.setAttribute('aria-hidden','true');
     const fill=make('div');fill.className='division-fill'+(sum<0?' negative':'');
     fill.style.width=(maximum?Math.min(100,Math.abs(sum)/maximum*100):0)+'%';
     track.append(fill);graph.append(track);
   }
   tr.append(graph);
   const share=shareEnabled&&sum!==null
     ?(sum/rangeTotal*100).toLocaleString('ru-RU',{maximumFractionDigits:1})+' %'
     :'—';
   const shareCell=make('td',share);
   shareCell.className='division-share';
   if(shareEnabled&&sum!==null){
     const shareTrack=make('div');shareTrack.className='division-share-track';
     shareTrack.setAttribute('aria-hidden','true');
     const fill=make('div');fill.className='division-share-fill';
     fill.style.width=Math.min(100,Math.max(0,sum/rangeTotal*100))+'%';
     shareTrack.append(fill);shareCell.append(shareTrack);
   }
   tr.append(shareCell);tbody.append(tr);
 }
 status('divisionStatus','Подразделений в сравнении: '+rows.length+'. Порядок: по подтверждённой сумме.');
}

function clearWork(message='Расшифровка появится после расчёта.'){
 invalidateComparison('Текущая выборка изменена. Рассчитайте диапазон повторно.');
 invalidateWorkTrend('Выборка изменена. Выберите работу или группу заново.');
 status('exportStatus','Экспорт доступен после расчёта. CSV сохраняется в новый файл.');
 window.nativeAdvancedAnalysis?.clear();
 workScope=null;workAfter=null;workBack=[];workNext=null;
 updateComparisonContext();
 $('workRows').replaceChildren();
 status('workStatus',message);
 controlState();
}
async function selectWorkTrend(item){
 if(analysisBusy||refreshBusy||workBusy||!workScope||revision===null)return;
 if(typeof item.key!=='string'||typeof item.name!=='string'||
    !['work','group'].includes(item.kind))return;
 const scope=workScope;
 const chosen={scope,key:item.key,kind:item.kind};
 workTrendSelection=chosen;
 const ticket=++workTrendRequest;workTrendBusy=true;
 window.nativeWorkTrends?.clear('Расчёт выбранной позиции в SQLite…');
 controlState();
 try{
   const data=await api.nativeWorkTimeline(
     revision,scope.type,scope.from,scope.to,scope.organizations,
     scope.organization,item.key,...nameArgs(scope.selectedNames,scope.source==='daily'?scope.displayType:null)
   );
   if(ticket!==workTrendRequest||workTrendSelection!==chosen||workScope!==scope)return;
   if(!data||data.key!==item.key||data.kind!==item.kind)
     throw Error('Некорректная идентичность выбранной позиции');
   window.nativeWorkTrends?.show(data);
   const panel=$('sqlWorkTrends');
   if(typeof panel.scrollIntoView==='function')
     panel.scrollIntoView({block:'nearest',behavior:'smooth'});
 }catch(error){
   if(ticket===workTrendRequest&&workTrendSelection===chosen&&workScope===scope){
     workTrendSelection=null;
     window.nativeWorkTrends?.error('Динамика выбранной позиции недоступна: '+userError(error));
   }
 }finally{if(ticket===workTrendRequest)workTrendBusy=false;controlState();}
}

async function workPage(cursor,reset=false){
 if(workBusy||analysisBusy||refreshBusy||!workScope||revision===null)return false;
 workBusy=true;controlState();
 try{
  const scope=workScope;
  const rows=await api.nativeAnalysisRangePage(
    revision,scope.type,scope.from,scope.to,
    scope.organizations,scope.organization,cursor,workSize,...nameArgs(scope.selectedNames)
  );
  if(!Array.isArray(rows)||rows.length>workSize)throw Error('Некорректная страница групп и работ');
  if(reset)workBack=[];
  workAfter=cursor;
  workNext=rows.length===workSize?rows[rows.length-1].key:null;
  const tbody=$('workRows');tbody.replaceChildren();
  const number=x=>x===null||x===undefined?'—':Number(x).toLocaleString('ru-RU',{maximumFractionDigits:3});
  for(const item of rows){
   const tr=make('tr');
   for(const value of [
     item.name??'',item.kind==='group'?'Группа':item.kind==='work'?'Работа':item.kind??'',
     number(item.count),number(item.amount),number(item.average),item.active?'Да':'Нет'
   ])tr.append(make('td',value));
   const cell=make('td');
   const graph=make('button','График');graph.type='button';
   graph.disabled=typeof item.key!=='string'||!['work','group'].includes(item.kind);
   graph.setAttribute('aria-label','Показать динамику: '+String(item.name??''));
   graph.addEventListener('click',()=>{void selectWorkTrend(item);});
   cell.append(graph);tr.append(cell);
   tbody.append(tr);
  }
  status('workStatus','На странице: '+rows.length+' позиций. Сортировка по ключу SQLite.');
  return true;
 }catch(err){
  workNext=null;workBack=[];workScope=null;window.nativeAdvancedAnalysis?.clear();
  invalidateWorkTrend('Расшифровка недоступна. Повторите расчёт и выбор позиции.');
  $('workRows').replaceChildren();
  status('workStatus','Ошибка SQL-чтения: '+userError(err)+'. Повторите расчёт.',true);
  return false;
 }finally{workBusy=false;controlState();}
}
async function calculateAnalysis(){
 if(!analysisReady||analysisBusy||revision===null)return;
 const selection=periodPicker.range($('analysisType').value,$('analysisFrom').value,$('analysisTo').value,$('analysisSource').value||'imported');
 const {type:kind,from,to}=selection||{};
 const organizations=analysisOrganizations(),names=selectedNames(),rev=revision;
 if(!from||!to||from>to||organizations.some(x=>!x)||!organizations.length){
   status('analysisStatus','Укажите корректный диапазон дат и хотя бы одно подразделение.',true);return;
 }
 analysisBusy=true;invalidateOrganizationTrends('Считываем ряды подразделений из SQLite…');const expectedTrends=organizationTrendsRequest;invalidateRanking('Подготавливаем рейтинг SQLite…');invalidateTimeline('Строим график из агрегатов SQLite…');const expectedTimeline=timelineRequest;clearWork();controlState();
 try{
   const result=await api.nativeAnalysisRange(rev,kind,from,to,organizations,...nameArgs(names));
   if(rev!==revision||expectedTimeline!==timelineRequest)return;
   const divisions=await api.nativeAnalysisOrganizations(rev,kind,from,to,organizations,...nameArgs(names));
   if(rev!==revision||expectedTimeline!==timelineRequest)return;
   displayAnalysisCoverage(result,{type:kind,from,to,organizations,selectedNames:names});
   showDivisionComparison(divisions,result.total?.amount,result.complete);
   // A failed chart must not discard already verified KPIs or division results.
   try{
     const timeline=await api.nativeAnalysisTimeline(rev,kind,from,to,organizations,...nameArgs(names,selection.source==='daily'?selection.displayType:null));
     if(expectedTimeline===timelineRequest)window.nativeTimeline?.show(timeline);
   }catch(timelineError){
     if(expectedTimeline===timelineRequest)
       window.nativeTimeline?.error('График не построен: '+userError(timelineError));
   }
   if(rev!==revision||expectedTimeline!==timelineRequest)return;
   try{
     const trends=await api.nativeOrganizationTrends(rev,kind,from,to,organizations,...nameArgs(names,selection.source==='daily'?selection.displayType:null));
     if(expectedTrends===organizationTrendsRequest)
       window.nativeOrganizationTrends?.show(trends,orgNames);
   }catch(trendsError){
     if(expectedTrends===organizationTrendsRequest)
       window.nativeOrganizationTrends?.error('График подразделений не построен: '+userError(trendsError));
   }
   if(rev!==revision||expectedTimeline!==timelineRequest)return;
   workScope={type:kind,displayType:selection.displayType,source:selection.source,from,to,organizations,selectedNames:names,organization:organizations.length===1?organizations[0]:null};
   window.nativeAdvancedAnalysis?.setContext(revision,workScope);
   updateComparisonContext();
   await reloadRanking();
   updateResultContext(kind,from,to,organizations,names,selection.source);
   // Full and partial status is set by displayAnalysisCoverage().
 }catch(err){
   if(rev!==revision||expectedTimeline!==timelineRequest)return;
   $('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearAnalysisCoverage();
   invalidateTimeline('График недоступен: расчёт не завершён.');
   invalidateRanking('Рейтинг недоступен: расчёт не завершён.');
   invalidateOrganizationTrends('Динамика подразделений недоступна: расчёт не завершён.');
   status('divisionStatus','Сравнение не выполнено из-за ошибки.',true);
   status('divisionShareStatus','Доли недоступны: расчёт завершился ошибкой.',true);
   status('analysisStatus','Ошибка расчёта: '+userError(err),true);
   $('analysisResultScope').textContent='Расчёт не выполнен. Предыдущие результаты удалены, чтобы не показывать устаревшую выборку.';
 }finally{analysisBusy=false;controlState();}
 if(workScope)await workPage(null,true);
}

async function reloadRanking(){
 const scope=workScope;
 if(!scope||revision===null){
   invalidateRanking('Для рейтинга сначала рассчитайте выбранный диапазон.');
   return;
 }
 const ticket=++rankingRequest;rankingBusy=true;controlState();
 const metric=$('rankingMetric').value,category=$('rankingCategory').value;
 window.nativeRanking?.clear('Рассчитываем 12 крупнейших позиций в SQLite…');
 try{
   const ranking=await api.nativeAnalysisRanking(
     revision,scope.type,scope.from,scope.to,scope.organizations,
     scope.organization,metric,category,...nameArgs(scope.selectedNames)
   );
   if(ticket===rankingRequest&&workScope===scope)
     window.nativeRanking?.show(ranking);
 }catch(error){
   if(ticket===rankingRequest&&workScope===scope)
     window.nativeRanking?.error('Рейтинг недоступен: '+userError(error));
 }finally{if(ticket===rankingRequest)rankingBusy=false;controlState();}
}

async function compareAnalysis(){
 const scope=workScope;
 if(comparisonBusy||analysisBusy||refreshBusy||!scope||revision===null)return;
 const baseline=periodPicker.range(scope.displayType||scope.type,$('comparisonBeforeFrom').value,$('comparisonBeforeTo').value,scope.source||'imported');
 const {from:beforeFrom,to:beforeTo}=baseline||{};
 if(!beforeFrom||!beforeTo||beforeFrom>beforeTo){
   window.nativeComparison?.error('Укажите корректные даты базового диапазона.');return;
 }
 comparisonBusy=true;
 const ticket=++comparisonRequest;
 window.nativeComparison?.clear('Сравниваем два диапазона через SQLite…');
 controlState();
 try{
   const result=await api.nativeAnalysisCompare(
     revision,scope.type,beforeFrom,beforeTo,scope.from,scope.to,scope.organizations,...nameArgs(scope.selectedNames)
   );
   if(ticket===comparisonRequest&&workScope===scope){
     comparisonBaseline={from:beforeFrom,to:beforeTo};updateComparisonContext();
     window.nativeComparison?.show(result);
   }
 }catch(error){
   if(ticket===comparisonRequest&&workScope===scope)
     window.nativeComparison?.error('Не удалось сравнить диапазоны: '+userError(error));
 }finally{
   comparisonBusy=false;
   controlState();
 }
}

function printAnalysis(){
 if(revision===null||analysisBusy||refreshBusy||workBusy||comparisonBusy||rankingBusy||workTrendBusy||exportBusy||queuedRefresh||window.nativeAdvancedAnalysis?.isBusy()||!workScope)return;
 const details=Array.from(document.querySelectorAll?.('#sqlAnalysis details, #sqlChanges details')||[]);
 const previous=details.map(node=>node.open);
 try{
  details.forEach(node=>{node.open=true;});
  window.print();
 }finally{details.forEach((node,i)=>{node.open=previous[i];});}
}
async function exportAnalysisCsv(){
 if(exportBusy||analysisBusy||workBusy||refreshBusy||!workScope||revision===null)return;
 exportBusy=true;controlState();
 try{
  const scope=workScope;
  const proposed='analiz-'+scope.type+'-'+scope.from+'-'+scope.to+'.csv';
  const destination=await api.chooseCsvPath(proposed);
  if(destination===null||destination===undefined||destination===''){
   status('exportStatus','Сохранение CSV отменено.');return;
  }
  if(typeof destination!=='string')throw Error('Некорректный путь CSV');
  await api.nativeExportRangeCsv(
   revision,scope.type,scope.from,scope.to,
   scope.organizations,scope.organization,destination,...nameArgs(scope.selectedNames)
  );
  status('exportStatus','CSV сохранён. Исходные отчёты не изменены.');
 }catch(err){
  status('exportStatus','Экспорт не выполнен: '+userError(err)+'. Выберите новый CSV-файл.',true);
 }finally{exportBusy=false;controlState();}
}
window.nativeAdvancedAnalysis?.onStateChange(controlState);
const year=String(new Date().getFullYear());
$('analysisFrom').value=year+'-01-01';
$('analysisTo').value=year+'-12-31';
periodPicker.apply($('analysisType').value);
$('analysisSource').addEventListener('change',()=>{
 if($('analysisType').value==='year'&&$('analysisSource').value!=='daily')$('analysisType').value='month';
 $('analysisYearOption').disabled=$('analysisSource').value!=='daily';
 periodPicker.apply($('analysisType').value);analysisReady=false;analysisFiltersChanged();
 status('analysisStatus','Источник изменён. Подготовьте индексы выбранного источника.');
});
$('analysisType').addEventListener('change',()=>{
 periodPicker.apply($('analysisType').value);
 analysisReady=false;clearAnalysisCoverage();invalidateRanking('Тип периода изменён. Повторите расчёт рейтинга.');invalidateOrganizationTrends('Тип периода изменён. Повторите расчёт динамики подразделений.');invalidateTimeline('Тип периода изменён. Подготовьте индексы и повторите расчёт.');$('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearWork();
 status('divisionShareStatus','Доли появятся после полного подтверждённого расчёта.');
 status('analysisStatus','Выбран другой тип периода. Подготовьте соответствующие индексы.');
 $('analysisResultScope').textContent='Тип периода изменён. Подготовьте индексы и повторите расчёт.';
 updateFilterContext();
 controlState();
});
$('analysisPrepare').addEventListener('click',prepareAnalysis);
$('analysisCancel').addEventListener('click',()=>{analysisCancelled=true;status('analysisStatus','Остановка после текущей порции…');});
$('analysisRun').addEventListener('click',calculateAnalysis);
$('analysisAcknowledge').addEventListener('click',acknowledgeCoverage);
$('analysisExport').addEventListener('click',exportAnalysisCsv);
$('analysisPrint').addEventListener('click',printAnalysis);
$('changesPrint').addEventListener('click',printAnalysis);
$('comparisonRun').addEventListener('click',compareAnalysis);
for(const id of ['comparisonBeforeFrom','comparisonBeforeTo']){
 $(id).addEventListener('change',()=>{
  invalidateComparison('Базовый диапазон изменился. Повторите сравнение.');
  controlState();
 });
}
for(const id of ['rankingMetric','rankingCategory'])$(id).addEventListener('change',()=>{void reloadRanking();});
$('workNext').addEventListener('click',async()=>{
 if(!workNext)return;
 const previous=workAfter;
 if(await workPage(workNext))workBack.push(previous);
 controlState();
});
$('workBack').addEventListener('click',async()=>{
 if(!workBack.length)return;
 const previous=workBack[workBack.length-1];
 if(await workPage(previous))workBack.pop();
 controlState();
});
function analysisFiltersChanged(){
 window.nativeDailyCalendar?.setContext(revision,analysisOrganizations(),selectedNames());
  clearAnalysisCoverage();invalidateRanking('Фильтры изменены. Повторите расчёт рейтинга.');invalidateOrganizationTrends('Фильтры изменены. Повторите расчёт динамики подразделений.');invalidateTimeline('Фильтры изменены. Повторите расчёт графика.');$('analysisValues').replaceChildren();$('divisionRows').replaceChildren();clearWork('Параметры изменены. Повторите расчёт.');
  status('divisionShareStatus','Доли сброшены: фильтры изменены.');
  status('divisionStatus','Параметры изменены. Нажмите «Рассчитать» ещё раз.');
  $('analysisResultScope').textContent='Фильтры изменены. Повторите расчёт, чтобы увидеть актуальную выборку.';
  updateFilterContext();
 controlState();
}
for(const id of ['analysisFrom','analysisTo'])
 $(id).addEventListener('change',analysisFiltersChanged);
if(!window.nativeOrganizationFilter)
 $('analysisOrganization').addEventListener('change',analysisFiltersChanged);
window.nativeWorkspaceFilterChanged=analysisFiltersChanged;

controlState();
// Called after a verified native SQLite commit; reads only bounded metadata/pages.
window.nativeWorkspaceRefresh=requestRefresh;
window.nativeWorkspaceReady=refresh();
if(typeof api.nativeRestoreLaunchInfo==='function'){
 void api.nativeRestoreLaunchInfo().then(result=>{
  if(result?.message)status('restoreStatus',result.message,!result.restored);
 }).catch(err=>status('restoreStatus',restoreError(err),true));
}
})();
