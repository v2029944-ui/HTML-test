/* Bounded coverage evidence and on-demand diagnostics for one saved report. */
(function(){
'use strict';
const UI=window.unifiedUI,api=UI.api;
const el=(...args)=>UI.el(...args);
const date=iso=>/^\d{4}-\d{2}-\d{2}$/.test(iso)?iso.slice(8,10)+'.'+iso.slice(5,7)+'.'+iso.slice(0,4):iso;
const range=(from,to)=>date(from)+(from===to?'':' — '+date(to));
const errorText=error=>window.analizUserError?window.analizUserError(error):(error?.message||String(error));
const views=new WeakMap();
function button(text,run){const node=el('button',text,'button secondary');node.type='button';node.addEventListener('click',run);return node;}
function status(cell){
 const labels=[];
 if(cell.available)labels.push(cell.numeric_zero?(cell.has_activity?'Нулевой итог':'Нулевые значения'):'Есть данные');
 else if(!cell.matching_reports)labels.push(cell.shifted_reports?'Другое начало периода':'Нет отчёта');
 if(cell.invalid_reports)labels.push('Некорректный отчёт');
 if(cell.duplicate_reports)labels.push('Повтор');
 if(cell.overlap_reports)labels.push('Пересечение');
 if(cell.warned_reports)labels.push('Есть замечания');
 return labels.join(' · ')||'Нет пригодного отчёта';
}
function sourceEvidence(root,evidence,scope){
 const data=evidence||{},section=el('div',null,'coverage-source-evidence');
 const count=key=>Number.isSafeInteger(data[key])&&data[key]>=0?data[key]:0;
 if(data.sheet_name)section.append(el('p','Лист отчёта: '+data.sheet_name,'muted small'));
 if(data.coverage)section.append(el('p','Заявленный охват листа: '+range(data.coverage.start,data.coverage.end)+'. Границы не подтверждают полноту источника.','muted small'));
 if(scope==='source')section.append(el('p','Признак нуля отчёта с общими замечаниями не доказывает ноль выбранной ячейки.','muted small'));
 else section.append(el('p',data.explicit_zero===true?'Сохранён признак явного нуля. Это не подтверждает полноту источника.':data.explicit_zero===false?'Признак явного нуля: нет.':'Признак явного нуля не сохранён.','muted small'));
 const missing=count('missing_day_entries');
 section.append(el('p',missing?'Записи о пропусках дней источника: '+missing+'. Это не устанавливает пропуск у выбранного подразделения.':'Полнота источника не установлена.',missing?'warn small':'muted small'));
 if(count('deferred_day_entries'))section.append(el('p','Отложенные замечания о пропусках дней: '+data.deferred_day_entries+'. В сохранённой диагностике отмечены как неактуальные.','muted small'));
 if(count('unclassified_day_entries'))section.append(el('p','Записи о пропусках без достаточных сведений об актуальности или дате: '+data.unclassified_day_entries+'.','muted small'));
 if(count('omitted_row_entries'))section.append(el('p',scope==='source'?'Исключённые строки в общих замечаниях: '+data.omitted_row_entries+'. Их связь с выбранным отчётом может быть неизвестна.':'Явно исключённые строки отчёта: '+data.omitted_row_entries+'. Сведения относятся к отчёту целиком.','warn small'));
 if(count('invalid_fields'))section.append(el('p','Некорректных полей сохранённых сведений: '+data.invalid_fields+'. Эти поля не подтверждают полноту или ноль.','warn small'));
 root.append(section);
}
async function render(root,s,active){
 views.get(root)?.dispose();const hosts=new Set(),view={dispose(){for(const host of hosts)window.sqlRowScroll?.reset(host);hosts.clear();}};views.set(root,view);
 const rendering=()=>active()&&views.get(root)===view;
 const host=(name,cls)=>{const node=el('div',null,cls);node.setAttribute('data-sql-scroll-host',name);hosts.add(node);return node;};
 const kind=s.type,pageSize=kind==='day'?31:12;
 const first=Date.parse(s.from+'T00:00:00Z'),last=Date.parse(s.to+'T00:00:00Z');
 if(!Number.isFinite(first)||!Number.isFinite(last)||first>last)return;
 const count=kind==='month'?(Number(s.to.slice(0,4))-Number(s.from.slice(0,4)))*12+Number(s.to.slice(5,7))-Number(s.from.slice(5,7))+1:Math.floor((last-first)/(86400000*(kind==='week'?7:1)))+1;
 const pages=Math.ceil(count/pageSize);let page=0,sequence=0,inspectionSequence=0;
 const matrixHost=host('coverage-periods','coverage-matrix-scroll'),matrixSlot=el('div'),pager=el('div',null,'coverage-matrix-pager'),inspection=el('section',null,'coverage-inspection'),unassigned=el('div');
 inspection.hidden=true;inspection.setAttribute('aria-label','Подробности выбранной ячейки');matrixSlot.append(matrixHost);
 root.replaceChildren(el('p','Статус не подтверждает полноту источника. Замечание относится к отчёту целиком; его связь с подразделением проверяйте в подробностях. Нулевой итог может включать ненулевые строки.','muted small'),matrixSlot,pager,inspection,unassigned);
 const clearInspection=keep=>{for(const node of hosts)if(node!==matrixHost&&node!==keep){window.sqlRowScroll?.reset(node);hosts.delete(node);}};
 const shift=(start,n)=>{const d=new Date(start+'T00:00:00Z');if(kind==='month')d.setUTCMonth(d.getUTCMonth()+n);else d.setUTCDate(d.getUTCDate()+n*(kind==='week'?7:1));return d.toISOString().slice(0,10);};
 async function drawPage(){
  const request=++sequence;inspectionSequence++;
  const from=shift(s.from,page*pageSize),candidateTo=shift(from,pageSize-1),to=candidateTo>s.to?s.to:candidateTo;
  const current=()=>rendering()&&request===sequence;
  clearInspection();inspection.hidden=true;inspection.replaceChildren();unassigned.replaceChildren();
  root.setAttribute('aria-busy','true');matrixHost.inert=true;pager.inert=true;if(!matrixHost.children.length)matrixHost.replaceChildren(el('p','Загрузка таблицы качества…','muted small'));
  try{
   let periods=null;const rows=[];
   for(let offset=0;offset<s.organizations.length;offset+=8){
    const organizations=s.organizations.slice(offset,offset+8),result=await api.nativeQualityMatrixPage(s.revision,kind,from,to,organizations);
    if(!current())return;
    if(result.revision!==s.revision||result.report_type!==kind||!Array.isArray(result.periods)||!result.periods.length||result.periods.length>pageSize||result.periods[0].start!==from||result.periods.at(-1).start!==to||!Array.isArray(result.rows)||result.rows.length!==organizations.length)throw Error('Некорректная страница матрицы качества.');
    if(periods&&JSON.stringify(periods)!==JSON.stringify(result.periods))throw Error('Несовпадающие периоды матрицы качества.');
    periods=result.periods;
    for(const organization of organizations){
     const row=result.rows.find(row=>row.organization===organization);
     if(!row||!Array.isArray(row.cells)||row.cells.length!==periods.length||row.cells.some((cell,i)=>cell.start!==periods[i].start))throw Error('Несовпадающие ячейки матрицы качества.');
     rows.push(row);
    }
   }
   if(!current())return;
   pager.replaceChildren();
   const table=el('table',null,'table coverage-matrix'),head=el('thead'),heading=el('tr'),body=el('tbody');heading.append(el('th','Подразделение'));
   for(const period of periods){const th=el('th',range(period.start,period.end));th.setAttribute('scope','col');heading.append(th);}head.append(heading);table.append(head);
   for(const row of rows){
    const tr=el('tr'),th=el('th',row.organization);th.setAttribute('scope','row');tr.append(th);
    for(const [i,cell] of row.cells.entries()){
     const label=status(cell),flagged=cell.invalid_reports||cell.duplicate_reports||cell.overlap_reports||cell.warned_reports;
     const td=el('td',null,!cell.available?'coverage-missing':flagged?'coverage-warning':'coverage-ok'),action=button(label,()=>openCell(inspection,row.organization,periods[i],action,current));action.className='coverage-cell-action';
     action.setAttribute('aria-label',row.organization+' · '+range(periods[i].start,periods[i].end)+': '+label+'. Показать отчёты.');
     action.title=cell.available?'Есть пригодный для аналитики отчёт. Это не подтверждает полноту источника.':'Нет пригодного для аналитики отчёта. Пропуск не означает ноль.';td.append(action);tr.append(td);
    }
    body.append(tr);
   }
   table.append(body);matrixHost.replaceChildren(table);
   const index=page,go=i=>{if(current()&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index){page=i;return drawPage();}return false;};
   if(!window.sqlRowScroll?.attach(matrixHost,{axis:'x',index,size:pageSize,total:count,count:periods.length,label:'Периоды качества данных',direct:true,go,footer:pager})&&pages>1){const previous=button('← Ранее',()=>go(index-1)),next=button('Позднее →',()=>go(index+1));previous.disabled=index===0;next.disabled=index+1>=pages;pager.append(previous,el('span',range(from,periods.at(-1).end),'muted small'),next);}
   for(const period of periods)if(period.unassigned_invalid){const note=el('div',null,'coverage-unassigned');note.append(el('p',range(period.start,period.end)+' · Без распознанного подразделения: '+period.unassigned_invalid+'. Эти ошибки не отнесены к строкам матрицы.','warn small'));const action=button('Показать отчёты без подразделения',()=>openCell(inspection,null,period,action,current));note.append(action);unassigned.append(note);}
   }catch(error){if(current()){window.sqlRowScroll?.reset(matrixHost);pager.replaceChildren();matrixHost.replaceChildren(el('p','Не удалось показать качество данных: '+errorText(error),'warn small'));}}
  finally{if(current()){matrixHost.inert=false;pager.inert=false;root.setAttribute('aria-busy','false');}}
 }
 function openCell(root,organization,period,trigger,pageCurrent){
  clearInspection();
  const selected=++inspectionSequence;let listSequence=0;
  const current=()=>pageCurrent()&&selected===inspectionSequence;
  const heading=el('h4',(organization||'Без распознанного подразделения')+' · '+range(period.start,period.end));heading.setAttribute('tabindex','-1');
  const content=el('div'),cards=host('coverage-reports'),close=()=>{if(!current())return;inspectionSequence++;clearInspection();root.hidden=true;trigger.focus?.();};
  const reportsStatus=el('div');content.append(cards);root.hidden=false;root.replaceChildren(heading,button('Закрыть подробности',close),content,reportsStatus);heading.focus?.({preventScroll:true});root.scrollIntoView?.({block:'nearest'});
  root.onkeydown=event=>{
   if(event.key!=='Escape')return;
   const detail=event.target.closest?.('details[open]');
   if(detail&&root.contains(detail)){detail.open=false;detail.querySelector('summary')?.focus();}
   else close();
   event.preventDefault?.();event.stopPropagation?.();
  };
  async function load(index=0){
   clearInspection(cards);
   const request=++listSequence;cards.replaceChildren(el('p','Загрузка отчётов…','muted small'));
   const valid=()=>current()&&request===listSequence;
   try{
    const result=await api.nativeQualityMatrixReportsAtPage(s.revision,kind,period.start,organization,index,20);
    if(!valid())return;
    if(result.revision!==s.revision||!Array.isArray(result.rows)||result.rows.length>20||!Number.isSafeInteger(result.total_rows)||result.total_rows<0)throw Error('Некорректная страница отчётов.');
    cards.replaceChildren();reportsStatus.replaceChildren();
    if(!result.rows.length)cards.append(el('p','Нет сохранённого отчёта с распознанным подразделением и нужным началом периода. Причина отсутствия неизвестна; значение не заменено нулём.','muted small'));
    for(const report of result.rows){
     const card=el('article',null,'coverage-report');card.append(el('strong',report.source_name||'Источник не сохранён'),el('p',range(report.start,report.end)+(report.shifted?' · Дата начала отличается от выбранного периода.':''),'small'));
     if(!report.valid)card.append(el('p',({invalid_rows:'Некорректная структура строк отчёта.',invalid_bounds:'Некорректные границы отчётного периода.'}[report.issue]||report.issue||'Отчёт не прошёл проверку структуры.'),'warn small'));
     if(report.duplicate||report.overlap)card.append(el('p',(report.duplicate?'Повторный отчёт. ':'')+(report.overlap?'Пересечение с другим отчётом.':''),'warn small'));
     if(report.warned)card.append(el('p','Есть замечания к отчёту. Их связь с выбранным подразделением может быть не указана.','warn small'));
     const details=el('details'),notes=el('div');details.append(el('summary','Замечания отчёта'),notes);let loaded=false;
     details.addEventListener('toggle',()=>{if(details.open&&!loaded&&valid()){loaded=true;void loadNotes(notes,report.id,valid);}});card.append(details);cards.append(card);
    }
    const pages=Math.max(1,Math.ceil(result.total_rows/20)),go=i=>{if(valid()&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return load(i);return false;};
    if(!window.sqlRowScroll?.attach(cards,{index,size:20,count:result.rows.length,total:result.total_rows,label:'Отчёты выбранной ячейки',direct:true,go,footer:reportsStatus})){const previous=button('← Предыдущие отчёты',()=>go(index-1)),next=button('Следующие отчёты →',()=>go(index+1));previous.disabled=index===0;next.disabled=index+1>=pages;reportsStatus.append(previous,el('span','Показано '+result.rows.length+' из '+result.total_rows,'muted small'),next);}
   }catch(error){if(valid()){window.sqlRowScroll?.reset(cards);content.replaceChildren(el('p','Не удалось прочитать отчёты: '+errorText(error),'warn small'));}}
  }
  void load();
 }
 async function loadNotes(root,id,current){
  let scope='report',sequence=0;const issues=host('coverage-notes'),summary=el('div'),notesStatus=el('div');root.replaceChildren(summary,issues,notesStatus);
  async function load(offset=0){
   const request=++sequence;issues.replaceChildren(el('p','Загрузка сохранённых замечаний…','muted small'));
   try{
    const result=await api.nativeReportQualityPage(s.revision,id,scope,offset,50);if(!current()||request!==sequence)return;
    if(!Array.isArray(result.issues)||result.issues.length>50||!Number.isSafeInteger(result.total_rows)||result.total_rows<0)throw Error('Некорректная страница замечаний.');summary.replaceChildren();notesStatus.replaceChildren();
    if(scope==='source')summary.append(el('p','Общие замечания источника могут относиться к другим датам и подразделениям.','muted small'));
    sourceEvidence(summary,result.evidence,scope);issues.replaceChildren();
    for(const issue of result.issues){
     const item=el('div',null,'coverage-issue');item.append(el('p',issue.reason||'Описание замечания не сохранено.'));
     const known=[];if(issue.organization)known.push('Подразделение: '+issue.organization);if(issue.date)known.push('Дата: '+date(issue.date));if(issue.sheet)known.push('Лист: '+issue.sheet);
     if(Number.isSafeInteger(issue.source_row)&&issue.source_row>0)known.push('Строка: '+issue.source_row);
     if(issue.scope==='source-period')item.append(el('p','Замечание периода источника. Его связь с выбранным подразделением может быть неизвестна.','muted small'));
     if(issue.relevant===false)item.append(el('p','Сохранённая пометка: неактуально.','muted small'));
     if(issue.relevant===true)item.append(el('p','Сохранённая пометка: актуально.','muted small'));
     if(issue.skipped===true)item.append(el('p','Строка помечена как исключённая.','warn small'));
     const cells=[...(Array.isArray(issue.cells)?issue.cells:[]),issue.work_cell,issue.count_cell,issue.amount_cell].filter(Boolean);if(cells.length)known.push('Ячейки: '+[...new Set(cells)].join(', '));
     item.append(el('p',known.join(' · ')||'Лист, ячейки, дата и подразделение для этого замечания не сохранены.','muted small'));issues.append(item);
    }
    if(!result.issues.length)summary.append(el('p','Подробные замечания не найдены. Это не подтверждает полноту источника.','muted small'));
    if(result.warning_fallback)summary.append(el('p','Есть признак замечания без сохранённого подробного описания.','warn small'));
    if(result.invalid_entries)summary.append(el('p','Неподдерживаемых записей диагностики: '+result.invalid_entries+'.','warn small'));
    const index=Math.floor(offset/50),pages=Math.max(1,Math.ceil(result.total_rows/50)),go=i=>{if(current()&&request===sequence&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return load(i*50);return false;};
    if(!window.sqlRowScroll?.attach(issues,{index,size:50,count:result.issues.length,total:result.total_rows,label:'Сохранённые замечания',direct:true,go,footer:notesStatus})){const previous=button('← Предыдущие замечания',()=>go(index-1)),next=button('Следующие замечания →',()=>go(index+1));previous.disabled=index===0;next.disabled=index+1>=pages;notesStatus.append(previous,el('span','Показано '+result.issues.length+' из '+result.total_rows,'muted small'),next);}
    if(scope==='report'&&result.source_anchor)notesStatus.append(button('Замечания источника',()=>{if(current()){scope='source';window.sqlRowScroll?.reset(issues);void load();}}));
    if(scope==='source')notesStatus.append(button('Замечания выбранного отчёта',()=>{if(current()){scope='report';window.sqlRowScroll?.reset(issues);void load();}}));
   }catch(error){if(current()&&request===sequence){window.sqlRowScroll?.reset(issues);root.replaceChildren(el('p','Не удалось прочитать замечания: '+errorText(error),'warn small'));}}
  }
  await load();
 }
 await drawPage();
}
window.unifiedCoverage=Object.freeze({render});
})();
