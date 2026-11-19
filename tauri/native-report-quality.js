(function nativeReportQuality(){
 'use strict';

 const $=id=>document.getElementById('reportQuality'+id);
 const dialog=$('Dialog');
 if(!dialog)return;
 const limit=50;
 const api=window.desktopAPI;
 const local=$('Local'),source=$('Source'),close=$('Close');
 const back=$('Back'),next=$('Next'),rows=$('Rows');
 let revision=null,selection=null,request=0;
 let scope='report',anchor=null,offset=0,nextOffset=null,busy=false;
 const types={invalid:'Некорректное значение',skipped:'Пропуск',mismatch:'Расхождение',warning:'Предупреждение'};
 const show=value=>value??'—';

 function updateControls(){
  local.hidden=source.hidden=!anchor;
  local.setAttribute('aria-pressed',String(scope==='report'));
  source.setAttribute('aria-pressed',String(scope==='source'));
  back.setAttribute('aria-disabled',String(busy||offset===0));
  next.setAttribute('aria-disabled',String(busy||nextOffset===null));
  $('Page').textContent='Страница '+(offset/limit+1);
  $('Anchor').hidden=scope!=='source'||!anchor;
  $('Anchor').textContent=scope==='source'&&anchor
   ? 'Замечания по всему источнику · отчёт хранения: '+anchor : '';
 }

 function setStatus(message,error=false){
  $('Status').textContent=message;
  $('Status').classList.toggle('error',error);
 }

 function render(issues){
  rows.replaceChildren();
  for(const issue of issues){
   const tr=document.createElement('tr');
   const refs=[['Работа',issue.work_cell],['Количество',issue.count_cell],['Сумма',issue.amount_cell]];
   const named=refs.filter(([,cell])=>cell).map(([name,cell])=>name+' '+cell);
   const extras=(issue.cells??[]).filter(cell=>!refs.some(([,namedCell])=>namedCell===cell));
   const type=Object.hasOwn(types,issue.kind)?types[issue.kind]:show(issue.kind);
   const classification=issue.relevant===false?type+' · отложено':type;
   for(const value of [classification,issue.reason,issue.sheet,issue.organization,issue.source_row,issue.date,[...named,...extras].join(' · ')||null]){
    const td=document.createElement('td');
    td.textContent=show(value);
    tr.append(td);
   }
   rows.append(tr);
  }
  $('Table').hidden=issues.length===0;
 }

 async function load(targetScope,targetOffset){
  if(!selection||busy)return;
  const ticket=++request,expected=selection;
  scope=targetScope;offset=targetOffset;nextOffset=null;busy=true;
  render([]);
  updateControls();
  setStatus('Загрузка сохранённых замечаний…');
  try{
   const result=await api.nativeReportQualityPage(expected.revision,expected.id,scope,offset,limit);
   if(ticket!==request||!dialog.open||selection!==expected)return;
   if(!Array.isArray(result?.issues)||result.issues.length>limit||
      (result.next_offset!==null&&result.next_offset!==offset+limit)||
      (result.source_anchor!==null&&typeof result.source_anchor!=='string')){
    throw Error('Некорректная страница сохранённых замечаний.');
   }
   anchor=result.source_anchor;
   nextOffset=result.next_offset;
   render(result.issues);
   $('Pager').hidden=offset===0&&nextOffset===null;
   const malformed=result.invalid_entries>0
    ? ' Записей неподдерживаемого формата: '+result.invalid_entries+'.' : '';
   if(!result.issues.length){
    setStatus((malformed?'Нет читаемых замечаний. Полнота данных этим не подтверждается.'
     :'Сохранённых замечаний нет. Полнота данных этим не подтверждается.')+malformed);
   }else{
    const detail=result.warning_fallback?' Сохранены предупреждения без подробной расшифровки.':'';
    setStatus('На странице: '+result.issues.length+'.'+detail+malformed);
   }
  }catch(error){
   if(ticket!==request||!dialog.open||selection!==expected)return;
   const message=String(error?.message??error);
   setStatus(/^Stale storage revision/.test(message)
    ? 'Данные изменились. Закройте окно и обновите список отчётов.'
    : 'Не удалось прочитать сохранённые замечания. '+(window.analizUserError?.(error)??''),true);
  }finally{
   if(ticket===request&&dialog.open&&selection===expected){
    busy=false;
    updateControls();
   }
  }
 }

 function dismiss(restoreFocus=true){
  const trigger=selection?.trigger;
  request++;selection=null;busy=false;
  render([]);
  if(dialog.open)dialog.close();
  if(restoreFocus){
   if(trigger?.isConnected)trigger.focus();
   else document.getElementById('nativeReportType')?.focus();
  }
 }

 function addReportAction(container,report,expectedRevision){
  const button=document.createElement('button');
  button.type='button';
  button.textContent='Замечания';
  button.classList.add('report-quality-action');
  button.setAttribute('aria-haspopup','dialog');
  button.setAttribute('aria-label','Замечания к отчёту '+report.period_start+' — '+report.period_end);
  button.addEventListener('click',()=>{
   if(dialog.open||revision!==expectedRevision)return;
   selection={id:report.id,revision:expectedRevision,trigger:button};
   anchor=null;
   $('Context').textContent=[report.period_start+' — '+report.period_end,report.source_name,report.id].filter(Boolean).join(' · ');
   $('Pager').hidden=true;
   dialog.showModal();
   close.focus();
   void load('report',0);
  });
  container.append(button);
 }
 close.addEventListener('click',()=>dismiss());
 dialog.addEventListener('cancel',event=>{event.preventDefault();dismiss();});
 dialog.addEventListener('close',()=>{if(!dialog.open&&selection)dismiss();});
 local.addEventListener('click',()=>{if(scope!=='report')void load('report',0);});
 source.addEventListener('click',()=>{if(scope!=='source'&&anchor)void load('source',0);});
 back.addEventListener('click',()=>{if(offset>0)void load(scope,offset-limit);});
 next.addEventListener('click',()=>{if(nextOffset!==null)void load(scope,nextOffset);});
 window.nativeReportQuality=Object.freeze({
  addReportAction,
  setRevision(value){
   if(revision!==value&&selection){
    dismiss(false);
    document.getElementById('nativeReportType')?.focus();
   }
   revision=value;
  }
 });
})();
