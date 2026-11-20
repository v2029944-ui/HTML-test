// Unified Tauri workspace: additional metric kinds with profile-driven XLSX mapping.
// Existing day/week/month analytics and legacy reports remain separate.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
if(!api?.nativeMetricKinds)return;
const userError=window.analizUserError||(()=>'Операция не выполнена. Перезапустите программу и повторите действие.');
let kinds=[],profiles=[],source=null,review=null,format=null,xlsxArgs=null,busy=false;
const ui=window.unifiedUI;
const currentRevision=()=>ui?.revision??null;
const parameters=()=>JSON.stringify([$('metricKindSelect').value,$('metricKindFrom').value,$('metricKindTo').value,$('metricXlsxProfile').value]);
const make=(tag,value)=>{
 const e=document.createElement(tag);
 if(value!==undefined)e.textContent=String(value);
 return e;
};
function status(message,error=false){
 $('metricKindStatus').textContent=message;
 $('metricKindStatus').className='status '+(error?'error':'muted');
}
function clearReview(){
 source=null;review=null;format=null;xlsxArgs=null;
 $('metricKindPreview').replaceChildren();
}
function controls(){
 for(const id of ['metricKindSelect','metricKindFrom','metricKindTo','metricXlsxProfile']){
  $(id).disabled=busy;
 }
 $('metricKindCalculate').disabled=busy||!kinds.length;
 $('metricKindChoose').disabled=busy||!kinds.length;
 $('metricKindChooseXlsx').disabled=busy||!profiles.length;
 $('metricKindApply').disabled=busy||!review;
}
function render(values,kind,container){
 const el=$(container);el.replaceChildren();
 for(const item of kind.metrics){
  const box=make('div');box.className='metric';
  const number=values?.[item.id];
  const shown=number===null||number===undefined||!Number.isFinite(Number(number))
   ?'—':Number(number).toLocaleString('ru-RU',{maximumFractionDigits:3});
  box.append(make('strong',shown),make('span',item.name+(item.unit?' · '+item.unit:'')));
  el.append(box);
 }
}
if(typeof ui?.onSnapshot==='function')ui.onSnapshot(()=>{
 clearReview();
 $('metricKindTotals').replaceChildren();
 $('metricKindCount').textContent='Данные изменились. Повторите расчёт.';
 status('База обновилась. Перед импортом повторно проверьте источник.');
 controls();
});
function findKind(id){return kinds.find(k=>k.id===id);}
function showPreview(result){
 if(!result||!findKind(result.kind)||typeof result.reviewToken!=='string'||
    result.reviewToken.length!==64)throw Error('Неверный план импорта.');
 review=result;
 const kind=findKind(result.kind);
 render(result.values,kind,'metricKindPreview');
 status('Проверено: '+kind.name+', '+result.start+' — '+result.end+
   ', '+result.rowCount+' строк. Запись начнётся только после подтверждения.');
}
async function calculate(){
 if(busy||!kinds.length)return;
 const id=$('metricKindSelect').value,kind=findKind(id),revision=currentRevision();
 if(!kind)return;
 const from=$('metricKindFrom').value||'0001-01-01';
 const to=$('metricKindTo').value||'9999-12-31';
 if(from>to){status('Начальная дата позже конечной.',true);return;}
 busy=true;controls();
 try{
  const summary=await api.nativeMetricSummary(id,from,to);
  if(currentRevision()!==revision)throw Error('Данные изменились. Повторите расчёт.');
  render(summary.values,kind,'metricKindTotals');
  $('metricKindCount').textContent='Отчётов: '+summary.reportCount+
    '. Строк: '+summary.rowCount+'. Суммы по загруженным данным, без проверки полноты периодов.';
  status('Показатели рассчитаны по отдельной схеме вида отчёта.');
 }catch(err){
  $('metricKindTotals').replaceChildren();
  status('Ошибка расчёта: '+userError(err),true);
 }finally{busy=false;controls();}
}
$('metricKindCalculate').addEventListener('click',calculate);
$('metricKindSelect').addEventListener('change',()=>{
 clearReview();controls();
 $('metricKindTotals').replaceChildren();$('metricKindCount').textContent='Нажмите «Рассчитать».';
});
$('metricXlsxProfile').addEventListener('change',()=>{
 clearReview();controls();
 status('Шаблон изменён. Выберите XLSX заново.');
});
for(const id of ['metricKindFrom','metricKindTo']){
 $(id).addEventListener('change',()=>{
  clearReview();
  $('metricKindTotals').replaceChildren();$('metricKindCount').textContent='Диапазон изменён. Повторите расчёт.';
  controls();
 });
}
$('metricKindChoose').addEventListener('click',async()=>{
 if(busy)return;
 clearReview();busy=true;controls();
 try{
  const selected=await api.chooseMetricJson();
  if(!selected){status('Выбор JSON отменён.');return;}
  $('metricKindFile').textContent='Проверяется JSON: '+selected.split(/[\\/]/).pop();
  const revision=currentRevision(),result=await api.nativeMetricPreview(selected);
  if(currentRevision()!==revision)throw Error('База изменилась во время проверки файла.');
  showPreview(result);
  source=selected;format='json';
 }catch(err){
  clearReview();status('Импорт не подготовлен: '+userError(err),true);
 }finally{busy=false;controls();}
});
$('metricKindChooseXlsx').addEventListener('click',async()=>{
 if(busy)return;
 clearReview();controls();
 const profile=$('metricXlsxProfile').value;
 const start=$('metricKindFrom').value,end=$('metricKindTo').value;
 if(!profile||!start||!end||start>end){
  status('Для импорта XLSX выберите шаблон и корректные даты начала и конца периода.',true);
  return;
 }
 busy=true;controls();
 try{
  const selected=await api.chooseMetricXlsx();
  if(!selected){status('Выбор XLSX отменён.');return;}
  $('metricKindFile').textContent='Проверяется XLSX: '+selected.split(/[\\/]/).pop();
  const revision=currentRevision(),result=await api.nativeMetricXlsxPreview(selected,profile,start,end);
  if(currentRevision()!==revision)throw Error('База изменилась во время проверки файла.');
  showPreview(result);
  source=selected;format='xlsx';xlsxArgs={profile,start,end};
 }catch(err){
  clearReview();status('Импорт XLSX не подготовлен: '+userError(err),true);
 }finally{busy=false;controls();}
});
$('metricKindApply').addEventListener('click',async()=>{
 if(busy||!review||!source)return;
 const captured={review,source,format,xlsxArgs,revision:currentRevision(),parameters:parameters()};
 busy=true;controls();
 try{
  const accepted=await window.confirm('Добавить отчёт «'+captured.review.reportId+'» в SQLite без перезаписи существующих?');
  if(accepted!==true){status('Импорт отменён. Проверенные данные не записаны.');return;}
  if(review!==captured.review||source!==captured.source||format!==captured.format||
     xlsxArgs!==captured.xlsxArgs||currentRevision()!==captured.revision||
     parameters()!==captured.parameters)throw Error('Параметры или база изменились. Проверьте файл заново.');
  const receipt=captured.format==='xlsx'
   ?await api.nativeMetricXlsxApply(captured.source,captured.xlsxArgs.profile,captured.xlsxArgs.start,captured.xlsxArgs.end,captured.review.reviewToken)
   :await api.nativeMetricApply(captured.source,captured.review.reviewToken);
  const importedKind=captured.review.kind;
  clearReview();
  $('metricKindSelect').value=importedKind;
  $('metricKindTotals').replaceChildren();
  $('metricKindCount').textContent='Отчёт добавлен. Нажмите «Рассчитать» для обновления итогов.';
  $('metricKindFile').textContent='Импортирован отчёт: '+receipt.reportId;
  const saved='Отчёт добавлен: '+receipt.reportId+'. Строк: '+receipt.rowCount+'.';
  try{await ui?.refresh?.();status(saved);}
  catch(err){status(saved+' Не удалось обновить экран: '+userError(err)+'. Обновите данные.',true);}
 }catch(err){
  clearReview();
  status('Импорт отклонён: '+userError(err)+'. Выберите файл заново.',true);
 }finally{busy=false;controls();}
});
controls();
window.metricKindWorkspaceReady=(async()=>{
 try{
  [kinds,profiles]=await Promise.all([api.nativeMetricKinds(),api.nativeMetricXlsxProfiles()]);
  if(!Array.isArray(kinds)||!kinds.length||!Array.isArray(profiles))
   throw Error('Каталог видов отчётов недоступен');
  const select=$('metricKindSelect');select.replaceChildren();
  for(const kind of kinds){
   const option=make('option',kind.name);option.value=kind.id;select.append(option);
  }
  select.value=kinds[0].id;
  const mapping=$('metricXlsxProfile');mapping.replaceChildren();
  for(const profile of profiles){
   const option=make('option',profile.name);option.value=profile.id;mapping.append(option);
  }
  if(profiles.length)mapping.value=profiles[0].id;
  status('Видов отчётов: '+kinds.length+'. Шаблонов XLSX: '+profiles.length+
   '. Добавление новых форматов — через каталог шаблонов.');
  return kinds;
 }catch(err){
  kinds=[];profiles=[];
  status('Ошибка загрузки видов отчётов: '+userError(err),true);
  return [];
 }finally{controls();}
})();
})();
