// Tauri-only SQLite inspector: page headers, never full report JSON.
// The legacy app bootstrap is unchanged; this feature is not a memory migration.
(function nativeReportInspector(){
  'use strict';
  const api=window.desktopAPI;
  const mount=document.getElementById('upload');
  if(!api?.nativeRevision||!api?.nativeReportPage||!mount)return;

  const pageSize=100;
  const node=(tag,text)=>{
    const element=document.createElement(tag);
    if(text!==undefined)element.textContent=text;
    return element;
  };
  const panel=node('section');
  panel.className='panel';
  panel.setAttribute('aria-label','Постраничный просмотр SQLite');
  const heading=node('h2','Постраничный просмотр SQLite');
  const description=node('p','Экспериментальный просмотр: читает только метаданные текущей страницы SQLite, без загрузки текстов отчётов. Основной интерфейс пока использует прежнюю загрузку всей истории.');
  description.className='muted small';
  const controls=node('div');controls.className='actions';
  const toggle=node('button','Открыть список отчётов');toggle.type='button';toggle.className='button secondary';
  const label=node('label');label.append(node('span','Тип отчёта: '));
  const type=node('select');type.setAttribute('aria-label','Фильтр типа отчёта в SQLite');
  for(const [value,text] of [['','Все'],['day','Дни'],['week','Недели'],['month','Месяцы']]){
    const option=node('option',text);
    option.value=value;
    type.append(option);
  }
  label.append(type);
  const refresh=node('button','С начала');
  const back=node('button','Назад');
  const next=node('button','Далее');
  for(const button of [refresh,back,next]){button.type='button';button.className='button secondary';}
  const status=node('p');
  status.className='muted small';status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  const items=node('div');
  const body=node('div');body.hidden=true;
  controls.append(label,refresh,back,next);
  body.append(controls,status,items);
  const newMode=node('a','Перейти в отдельный SQL-режим (без загрузки истории)');
  newMode.id='openNativeWorkspace';
  newMode.href='native-workspace.html';
  panel.append(heading,description,newMode,toggle,body);
  mount.insertBefore(panel,mount.firstChild);

  let revision=null,after=null,history=[],busy=false,stale=false,nextCursor=null;
  const filter=()=>({report_type:type.value||null});
  const setStatus=(message,error=false)=>{
    status.textContent=message;
    status.className=(error?'warn':'muted')+' small';
  };
  const updateButtons=()=>{
    toggle.disabled=busy;
    type.disabled=busy;
    refresh.disabled=busy;
    back.disabled=busy||history.length===0||stale;
    next.disabled=busy||!nextCursor||stale;
  };
  async function loadPage(cursor,reset=false){
    if(busy)return false;
    busy=true;updateButtons();
    try{
      const nextRevision=(reset||revision===null)?await api.nativeRevision():revision;
      const rows=await api.nativeReportPage(nextRevision,filter(),cursor,pageSize);
      if(!Array.isArray(rows)||rows.length>pageSize)
        throw Error('Некорректная страница SQLite.');
      if(reset){history=[];stale=false;}
      revision=nextRevision;
      after=cursor;
      nextCursor=rows.length===pageSize?{
        start:rows[rows.length-1].period_start,
        id:rows[rows.length-1].id
      }:null;
      items.replaceChildren();
      if(!rows.length){
        items.append(node('p','Для этого фильтра отчёты не найдены.'));
      }else{
        const table=node('table');
        const header=node('tr');
        for(const name of ['Начало','Конец','Тип','Источник','Идентификатор']){
          const th=node('th',name);th.scope='col';header.append(th);
        }
        const head=node('thead');head.append(header);
        const tbody=node('tbody');
        for(const report of rows){
          const tr=node('tr');
          for(const value of [report.period_start,report.period_end,report.report_type,report.source_name||'—',report.id]){
            tr.append(node('td',String(value??'')));
          }
          tbody.append(tr);
        }
        table.append(head,tbody);items.append(table);
      }
      setStatus('Ревизия '+revision+' · записей на странице: '+rows.length+'. При изменении базы обновите список.');
      return true;
    }catch(error){
      const message=String(error?.message||error);
      if(/stale|revision|устар|изменен/i.test(message))stale=true;
      setStatus('Не удалось прочитать SQLite: '+message+(stale?' Нажмите «С начала».':''),true);
      return false;
    }finally{
      busy=false;updateButtons();
    }
  }
  toggle.addEventListener('click',async()=>{
    body.hidden=!body.hidden;
    toggle.textContent=body.hidden?'Открыть список отчётов':'Свернуть список отчётов';
    if(!body.hidden&&revision===null)await loadPage(null,true);
  });
  refresh.addEventListener('click',()=>{void loadPage(null,true);});
  type.addEventListener('change',()=>{void loadPage(null,true);});
  next.addEventListener('click',async()=>{
    if(!nextCursor||busy)return;
    const current=after;
    if(await loadPage(nextCursor))history.push(current);
    updateButtons();
  });
  back.addEventListener('click',async()=>{
    if(!history.length||busy)return;
    const previous=history[history.length-1];
    if(await loadPage(previous))history.pop();
    updateButtons();
  });
  updateButtons();
})();
