// Optional Tauri-only workspace for declarative, independent report families.
// Existing day/week/month analytics and reports are intentionally not modified.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
if(!api?.nativeMetricKinds)return;
let kinds=[],source=null,review=null,busy=false;
const make=(tag,value)=>{
 const e=document.createElement(tag);
 if(value!==undefined)e.textContent=String(value);
 return e;
};
function status(message,error=false){
 $('metricKindStatus').textContent=message;
 $('metricKindStatus').className='status '+(error?'error':'muted');
}
function controls(){
 $('metricKindSelect').disabled=busy;
 $('metricKindFrom').disabled=busy;
 $('metricKindTo').disabled=busy;
 $('metricKindCalculate').disabled=busy||!kinds.length;
 $('metricKindChoose').disabled=busy;
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
function findKind(id){return kinds.find(k=>k.id===id);}
async function calculate(){
 if(busy||!kinds.length)return;
 const id=$('metricKindSelect').value,kind=findKind(id);
 if(!kind)return;
 const from=$('metricKindFrom').value||'0001-01-01';
 const to=$('metricKindTo').value||'9999-12-31';
 if(from>to){status('Начальная дата позже конечной.',true);return;}
 busy=true;controls();
 try{
  const summary=await api.nativeMetricSummary(id,from,to);
  render(summary.values,kind,'metricKindTotals');
  $('metricKindCount').textContent='Отчётов: '+summary.reportCount+
   '. Строк: '+summary.rowCount+'. Суммы по загруженным данным, без проверки полноты периодов.';
  status('Показатели рассчитаны по отдельной схеме вида отчёта.');
 }catch(err){
  $('metricKindTotals').replaceChildren();
  status('Ошибка расчёта: '+String(err?.message||err),true);
 }finally{busy=false;controls();}
}
$('metricKindCalculate').addEventListener('click',calculate);
$('metricKindSelect').addEventListener('change',()=>{
 $('metricKindTotals').replaceChildren();$('metricKindCount').textContent='Нажмите «Рассчитать».';
});
for(const id of ['metricKindFrom','metricKindTo']){
 $(id).addEventListener('change',()=>{
  $('metricKindTotals').replaceChildren();$('metricKindCount').textContent='Диапазон изменён. Повторите расчёт.';
 });
}
$('metricKindChoose').addEventListener('click',async()=>{
 if(busy)return;
 busy=true;controls();
 try{
  const selected=await api.chooseMetricJson();
  if(!selected)return;
  source=selected;review=null;
  $('metricKindPreview').replaceChildren();
  $('metricKindFile').textContent='Проверяется: '+selected.split(/[\\/]/).pop();
  const result=await api.nativeMetricPreview(selected);
  if(!result||!findKind(result.kind)||!result.reviewToken)
   throw Error('Неверный план импорта.');
  review=result;
  const kind=findKind(result.kind);
  render(result.values,kind,'metricKindPreview');
  status('Проверено: '+kind.name+', '+result.start+' — '+result.end+
   ', '+result.rowCount+' строк. Запись начнётся только после подтверждения.');
 }catch(err){
  review=null;
  $('metricKindPreview').replaceChildren();
  status('Импорт не подготовлен: '+String(err?.message||err),true);
 }finally{busy=false;controls();}
});
$('metricKindApply').addEventListener('click',async()=>{
 if(busy||!review||!source)return;
 if(!window.confirm('Добавить отчёт «'+review.reportId+'» в SQLite без перезаписи существующих?'))return;
 busy=true;controls();
 try{
  const receipt=await api.nativeMetricApply(source,review.reviewToken);
  const importedKind=review.kind;
  source=null;review=null;
  $('metricKindPreview').replaceChildren();
  $('metricKindSelect').value=importedKind;
  status('Отчёт добавлен: '+receipt.reportId+'. Строк: '+receipt.rowCount+'.');
 }catch(err){
  source=null;review=null;
  $('metricKindPreview').replaceChildren();
  status('Импорт отклонён: '+String(err?.message||err)+'. Выберите файл заново.',true);
 }finally{
  busy=false;controls();
 }
});
controls();
window.metricKindWorkspaceReady=(async()=>{
 try{
  kinds=await api.nativeMetricKinds();
  if(!Array.isArray(kinds)||!kinds.length)throw Error('Каталог видов отчётов пуст');
  const select=$('metricKindSelect');select.replaceChildren();
  for(const kind of kinds){
   const option=make('option',kind.name);option.value=kind.id;select.append(option);
  }
  select.value=kinds[0].id;
  status('Доступно видов отчётов: '+kinds.length+'. Можно добавить новый вид через декларативный каталог.');
  return kinds;
 }catch(err){
  status('Ошибка загрузки видов отчётов: '+String(err?.message||err),true);
  return [];
 }finally{controls();}
})();
})();
