/* Original Excel, calendar and quality surfaces backed by bounded SQLite reads. */
(function(){
'use strict';
let started=false;
function init(){
 if(started)return;started=true;
 const UI=window.unifiedUI,{api,$,el}=UI;
 const LIMIT=50,kinds={day:'День',week:'Неделя',month:'Месяц'},types={invalid:'Некорректное значение',skipped:'Пропуск строки',mismatch:'Расхождение',warning:'Предупреждение',missing:'Пропущен период',duplicate:'Повторный период',overlap:'Пересечение','empty-name':'Пустое название',import:'Импорт'};
 let generation=0,reportTicket=0,calendarTicket=0,qualityTicket=0,detailTicket=0;
 let reportIndex=0,qualityOffset=0;
 let year=null,calendarData=null,monthPoints=[],dayPoints=new Map(),selectedMonth=null;
 let sourcePath=null,sheets=[],review=null,options=null,importBusy=false,sourceTicket=0,parameterTicket=0;
 const button=(text,fn)=>{const b=el('button',text,'button secondary');b.type='button';b.addEventListener('click',fn);return b;};
 const activeWorkspace=()=>window.sharedWorkspace?.state?.mode==='shared'?window.sharedWorkspace.state.folder:null;
 let wideSheetRows=[],sourceFolderContext=null,workspaceFolder=activeWorkspace();
 let uploadMode='import';
 const uploadPanels={import:$('excelFile').closest?.('.panel'),calendar:$('reportPeriods'),quality:$('qualityHeading').closest?.('.panel')};
 const listPanel=el('section',undefined,'panel');listPanel.id='unifiedSavedReports';
 const uploaded=$('uploadedReports');if(uploaded){uploaded.open=true;listPanel.append(uploaded);$('upload').append(listPanel);}uploadPanels.list=listPanel;
 const uploadNav=el('div',undefined,'actions');uploadNav.id='unifiedUploadNav';
 for(const [mode,title]of [['import','Разовая загрузка'],['calendar','Календарь отчётов'],['list','Список отчётов'],['quality','Контроль качества']]){const b=button(title,()=>openUploadView(mode));b.id='unifiedUpload'+mode[0].toUpperCase()+mode.slice(1);b.dataset.uploadMode=mode;uploadNav.append(b);}
 if(uploadPanels.import&&$('additionalMetrics'))uploadPanels.import.append($('additionalMetrics'));
 $('upload').prepend(uploadNav);
 function openUploadView(mode){if(!Object.hasOwn(uploadPanels,mode))return;uploadMode=mode;if(mode==='list'&&$('uploadedReports'))$('uploadedReports').open=true;for(const [key,panel]of Object.entries(uploadPanels))if(panel)panel.hidden=key!==mode;for(const b of uploadNav.children)b.setAttribute('aria-pressed',String(b.dataset.uploadMode===mode));return refreshUpload();}
 if(uploadPanels.import){const intro=uploadPanels.import.querySelector('p');if(intro)intro.textContent='Выберите книгу → проверьте распознавание → просмотрите результат и сохраните отчёты.';for(const p of uploadPanels.import.querySelectorAll('p'))if(p.id==='ordinaryImportHint')p.textContent='Укажите дату первого периода. Даты отдельных наборов можно изменить после проверки файла.';}
 $('excelFileStatus').textContent='Выбрать файл Excel…';
 const yearDetails=el('details');yearDetails.id='unifiedYearDetails';yearDetails.append(el('summary','Уточнить год дневных колонок'),$('dailyYear').parentElement);$('dailyWideOptions').prepend(yearDetails);
 const wideManual=el('details');wideManual.id='unifiedWideManual';wideManual.append(el('summary','Указать столбцы дневного листа вручную'));

 const plural=new Intl.PluralRules('ru'),reportNoun=count=>({one:'отчёт',few:'отчёта',many:'отчётов',other:'отчёта'}[plural.select(count)]);
 const show=v=>v===null||v===undefined?'—':String(v);
 const live=(rev,ticket,current)=>UI.revision===rev&&ticket===current;
 function status(id,text){$(id).textContent=text;}
 function page(root,caption,headers,rows,help){
  root.replaceChildren();
  const wrap=el('div',undefined,'table-scroll'),table=el('table');table.append(el('caption',caption));
  const head=el('thead'),hr=el('tr');for(const h of headers){const th=el('th',h);th.scope='col';hr.append(th);}head.append(hr);table.append(head);
  const body=el('tbody');for(const row of rows){const tr=el('tr');for(const value of row){const td=el('td');if(value&&typeof value==='object'&&value.nodeType)td.append(value);else td.textContent=show(value);tr.append(td);}body.append(tr);}table.append(body);wrap.append(table);
  if(help){const section=el('section');section.append(wrap);root.append(section);UI.tableHelp(section,table,headers,help);}
  else root.append(wrap);
 }
 function reportFilter(extra={}){return {report_type:null,organization:null,from:null,to:null,...extra};}
 const reportPager=el('div',undefined,'unified-pager'),reportPageStatus=el('span','');
 reportPager.id='unifiedReportsPager';reportPageStatus.id='unifiedReportsPage';reportPageStatus.setAttribute('role','status');
 reportPager.append(reportPageStatus);$('reports').after(reportPager);
 window.sqlRowScroll.retire($('qualityPrev'),$('qualityNext'));
 async function reportOrgs(id,root,rev,index=0){
  const current=()=>UI.revision===rev&&root.isConnected;
  try{
   const {rows,total_rows:total}=await api.managementReportOrgWindow(rev,id,index,LIMIT);
   if(!current())return;
   root.replaceChildren(...rows.map(row=>el('span',row[1]+' ')));
   root.dataset.sqlScrollHost='report-organizations';
   window.sqlRowScroll.attach(root,{index,size:LIMIT,count:rows.length,total,label:'Подразделения отчёта',go:i=>current()&&reportOrgs(id,root,rev,i)});
  }catch(error){if(current()){window.sqlRowScroll.reset(root);root.textContent='Не удалось прочитать подразделения: '+String(error.message||error);}}
 }
 function qualityAction(report){return button('Контроль качества',()=>{
  $('qualitySource').value='saved';syncQualityPeriods();$('qualityPeriod').value='all';setQualityDates(report.start,report.start);
  $('qualitySearch').value='';$('qualityReportSource').value=report.source_name||'';qualityOffset=0;void openUploadView('quality');$('qualityHeading').scrollIntoView?.({block:'start',behavior:'smooth'});
 });}
 function reportCard(r,rev){
  const card=el('section',undefined,'calendar-source');
  card.append(el('strong',(kinds[r.report_type]||r.report_type)+' · '+r.start+' — '+r.end),el('p','Excel: '+(r.source_name||'источник не сохранён')),el('p','Строк: '+show(r.rows)+' · Сумма: '+UI.fmt(r.amount,'amount')+' · Замечаний: '+show(r.warnings)));
  const orgs=el('details'),list=el('p');orgs.append(el('summary','Подразделения ('+r.organizations+')'),list);let loaded=false;orgs.addEventListener('toggle',()=>{if(orgs.open&&!loaded){loaded=true;void reportOrgs(r.id,list,rev);}});card.append(orgs,qualityAction(r));
  if(api.nativeDeleteReport){let deleting=false;const action=button('Удалить отчёт',async()=>{
   if(deleting||UI.revision!==rev)return;deleting=true;action.disabled=true;
   try{if(await window.confirm('Удалить отчёт '+r.start+' — '+r.end+'?')!==true)return;
    if(UI.revision!==rev)throw Error('Данные изменились. Обновите список отчётов.');
    await api.nativeDeleteReport(rev,r.id);
     try{await UI.refresh();}catch(error){UI.notice?.('Отчёт удалён из SQLite, но не удалось обновить отображение: '+String(error.message||error)+'. Обновите данные.',true);}
   }catch(error){UI.notice?.('Не удалось удалить отчёт: '+String(error.message||error),true);}finally{deleting=false;action.disabled=false;}
  });card.append(action);}

  if(r.shared_source_artifact)card.append(el('p','Сохранены общие замечания исходного файла; ссылка на отчёт хранения показана в таблице качества.','muted small'));
  return card;
 }
 async function loadReports(index=reportIndex){
  const rev=UI.revision,ticket=++reportTicket,root=$('reports');
  if(!Number.isSafeInteger(rev)){reportPageStatus.textContent='Данные обновляются';return;}
  try{
   const filter=reportFilter();
   const [{rows,total_rows:total},summary]=await Promise.all([api.managementReportWindow(rev,filter,index,LIMIT),api.managementReportSummary(rev,filter)]);
   if(!live(rev,ticket,reportTicket))return;
   if(!Array.isArray(rows)||rows.length>LIMIT)throw Error('Некорректная страница отчётов');
   $('reportCount').textContent=summary.reports+' периодов · '+summary.rows+' строк · '+UI.fmt(summary.amount,'amount');
   root.replaceChildren();
   for(const r of rows){root.append(reportCard(r,rev));if(kinds[r.report_type]&&validDate(r.start))savedQualityPeriods.add(r.report_type+':'+r.start);}
   if($('qualitySource').value!=='pending')syncQualityPeriods();
   if(!rows.length)root.append(el('p','Отчёты не найдены.'));
   reportIndex=index;reportPageStatus.textContent='';
   window.sqlRowScroll.attach(root,{index,size:LIMIT,count:rows.length,total,footer:reportPager,label:'Страницы загруженных отчётов',go:i=>live(rev,ticket,reportTicket)&&loadReports(i)});
   if(year===null)year=Number((summary.latest||new Date().toISOString()).slice(0,4));
  }catch(error){if(live(rev,ticket,reportTicket)){window.sqlRowScroll.reset(root);reportPageStatus.textContent='Список недоступен';status('reportCount','Не удалось прочитать отчёты: '+String(error.message||error));root.replaceChildren();}}
 }
 // Keep the year trigger mounted while the request changes status, so both
 // confirmation and Escape return focus to the very button that opened it.
 const yearPicker=button('',()=>{
  if(!Number.isSafeInteger(UI.revision)||year===null)return;
  const initial=String(year).padStart(4,'0')+'-01-01';
  window.referenceCalendar.open({anchor:yearPicker,title:'Год календаря отчётов',initial,unit:'year',
   allowed:{has:date=>Number(date.slice(0,4))>=1&&Number(date.slice(0,4))<=9998},
   apply:date=>{year=Number(date.slice(0,4));void loadCalendar();}});
 });
 yearPicker.id='referenceReportYear';yearPicker.setAttribute('aria-haspopup','dialog');
 function calendarStatus(text){
  if(!window.referenceCalendar){status('calendarRange',text);return;}
  if(yearPicker.parentElement!==$('calendarRange'))$('calendarRange').replaceChildren(yearPicker);
  yearPicker.textContent=text;yearPicker.setAttribute('aria-label','Выбрать год календаря отчётов · '+year);
 }
 function monthName(n){return new Intl.DateTimeFormat('ru-RU',{month:'long',timeZone:'UTC'}).format(new Date(String(year).padStart(4,'0')+'-'+String(n+1).padStart(2,'0')+'-01T12:00:00Z'));}
 function monthBounds(n){const from=String(year).padStart(4,'0')+'-'+String(n+1).padStart(2,'0')+'-01';const end=new Date(from+'T12:00:00Z');end.setUTCMonth(end.getUTCMonth()+1,0);return [from,end.toISOString().slice(0,10)];}
 function clearDetails(focus){++detailTicket;$('calendarDetails').replaceChildren();selectedMonth=null;$('reportCalendar').querySelectorAll('button').forEach(b=>b.setAttribute('aria-expanded','false'));focus?.focus();}
 async function readRollups(rev,from,to,granularity){
  const points=[];let after=null;
  do{const result=await api.nativeDailyRollup(rev,from,to,granularity,UI.snapshot.org_names,after,50);if(UI.revision!==rev)return [];if(!Array.isArray(result.points)||result.points.length>50)throw Error('Некорректная сводка календаря');points.push(...result.points);after=result.next_cursor;if(points.length>366)throw Error('Превышен размер календаря');}while(after);
  return points;
 }
 async function loadCalendar(){
  const rev=UI.revision,ticket=++calendarTicket;clearDetails();$('reportCalendar').replaceChildren();calendarData=null;dayPoints=new Map();monthPoints=[];
  if(!Number.isSafeInteger(rev))return;
  if(year===null)year=new Date().getUTCFullYear();calendarStatus(year+' год · загрузка…');$('calendarPrev').disabled=$('calendarNext').disabled=true;
  const from=String(year).padStart(4,'0')+'-01-01',to=String(year).padStart(4,'0')+'-12-31';
  try{const [headers,monthly,daily,annual]=await Promise.all([api.unifiedCalendar(rev,year),readRollups(rev,from,to,'month'),readRollups(rev,from,to,'day'),readRollups(rev,from,to,'year')]);
   if(!live(rev,ticket,calendarTicket))return;calendarData=headers;monthPoints=monthly;dayPoints=new Map(daily.map(p=>[p.start,p]));
   const y=annual[0];calendarStatus(year+' год'+(y?' · '+(y.complete?'Дневные итоги: ':'Дневные, доступная часть: ')+UI.fmt((y.complete?y.total:y.available_total)?.amount,'amount'):''));
   $('calendarPrev').disabled=year<=1;$('calendarNext').disabled=year>=9998;
   for(let n=0;n<12;n++){
    const [start,end]=monthBounds(n),p=monthly.find(x=>x.start===start),days=headers.days.filter(x=>x.date>=start&&x.date<=end),warnings=days.reduce((s,d)=>s+d.warnings,0),loaded=days.filter(d=>d.day_reports>0).length;
    const complete=p?.complete===true,hasSources=days.length>0,kind=complete&&!warnings?'complete':p?.missing||warnings?'error':hasSources?'warn':'missing';
    const b=button('',()=>void openMonth(n,b));b.className='calendar-period calendar-'+kind;b.dataset.month=start.slice(0,7);b.setAttribute('aria-expanded','false');b.setAttribute('aria-controls','calendarDetails');
    const sources=['day','week','month'].filter(t=>days.some(d=>d[t+'_reports']>0)).map(t=>kinds[t]);
    b.append(el('strong',monthName(n)),el('small',complete?'Полнота подтверждена':hasSources?'Полнота не подтверждена':'Нет загруженных данных'),el('small','Дней: '+loaded+' · Пропусков: '+show(p?.missing)),el('small','Замечаний: '+warnings),el('small','Источники: '+(sources.join(', ')||'нет')));
    if(p?.available_reports)b.append(el('small',(complete?'Дневные итоги: ':'Дневные, доступная часть: ')+UI.fmt((complete?p.total:p.available_total)?.count,'count')+' · '+UI.fmt((complete?p.total:p.available_total)?.amount,'amount')));
    $('reportCalendar').append(b);
   }
  }catch(error){if(live(rev,ticket,calendarTicket)){calendarStatus(year+' год · Не удалось прочитать календарь: '+String(error.message||error));$('reportCalendar').replaceChildren();clearDetails();}}
 }
 async function sourceList(root,filter,rev,index=0){
  const token=detailTicket,current=()=>live(rev,token,detailTicket)&&root.isConnected;
  try{
   const {rows,total_rows:total}=await api.managementReportWindow(rev,reportFilter(filter),index,LIMIT);
   if(!current())return;
   root.replaceChildren(...rows.map(r=>reportCard(r,rev)));
   if(!rows.length)root.append(el('p','Источники не найдены. Отсутствие отчёта не означает нулевые показатели.'));
   root.dataset.sqlScrollHost='report-sources';
   window.sqlRowScroll.attach(root,{index,size:LIMIT,count:rows.length,total,label:'Источники выбранного периода',go:i=>current()&&sourceList(root,filter,rev,i)});
  }catch(error){if(current()){window.sqlRowScroll.reset(root);root.replaceChildren(el('p','Не удалось прочитать источники: '+String(error.message||error)));}}
 }
 async function openMonth(n,trigger){
  clearDetails();selectedMonth=n;const rev=UI.revision,[from,to]=monthBounds(n),root=$('calendarDetails'),p=monthPoints.find(p=>p.start===from);
  trigger.setAttribute('aria-expanded','true');root.append(el('h3',monthName(n)+' '+year+' · '+(p?.complete?'Полнота подтверждена':'Полнота не подтверждена')),button('Закрыть подробности месяца',()=>clearDetails(trigger)),el('p','Дневные отчёты · пропусков: '+show(p?.missing)+'. Недельные и месячные суммы по дням не распределяются.'));
  const grid=el('div',undefined,'calendar-day-grid'),detail=el('div',undefined,'calendar-day-details');detail.setAttribute('aria-live','polite');
  for(const name of ['Пн','Вт','Ср','Чт','Пт','Сб','Вс'])grid.append(el('span',name,'calendar-weekday'));
  for(let i=0;i<(new Date(from+'T12:00:00Z').getUTCDay()+6)%7;i++)grid.append(el('span'));
  for(let d=1;d<=Number(to.slice(8));d++){
   const date=from.slice(0,8)+String(d).padStart(2,'0'),point=dayPoints.get(date),header=calendarData.days.find(h=>h.date===date),exists=point?.available_reports>0;
   const b=button('',()=>{
    detail.replaceChildren(el('h4',date),button('Закрыть подробности дня',()=>{detail.replaceChildren();b.focus();}));
    detail.append(el('p',point?.complete?'Полнота подтверждена':exists?'Доступна часть дневных данных; полнота не подтверждена.':'Дневной отчёт отсутствует. Это не нулевой показатель.'));
    if(exists)detail.append(el('p','Количество: '+UI.fmt((point.complete?point.total:point.available_total)?.count,'count')+' · Сумма: '+UI.fmt((point.complete?point.total:point.available_total)?.amount,'amount')));
    const list=el('div');detail.append(list);void sourceList(list,{from:date,to:date},rev);
   });b.className='calendar-day calendar-'+(point?.complete&&!header?.warnings?'complete':header?'warn':'missing');b.append(el('strong',d),el('small',point?.complete?'Есть':header?'Неполнота':'—'));b.setAttribute('aria-label',date+': '+(point?.complete?'Полнота подтверждена':header?'Полнота не подтверждена':'Нет дневных данных'));
   if(exists)b.append(el('small','Кол.: '+UI.fmt((point.complete?point.total:point.available_total)?.count,'count')),el('small','Сум.: '+UI.fmt((point.complete?point.total:point.available_total)?.amount,'amount')));grid.append(b);
  }
  root.append(grid,detail);
  for(const [type,title] of [['week','Недельные источники'],['month','Месячные источники']]){const group=el('details'),list=el('div');group.append(el('summary',title+' — отдельно от дневных итогов'),list);let loaded=false;group.addEventListener('toggle',()=>{if(group.open&&!loaded){loaded=true;const start=new Date(from+'T12:00:00Z');if(type==='week')start.setUTCDate(start.getUTCDate()-6);void sourceList(list,{from:start.toISOString().slice(0,10),to,report_type:type},rev);}});root.append(group);}
 }
 $('calendarPrev').addEventListener('click',()=>{if(year>1){year--;void loadCalendar();}});$('calendarNext').addEventListener('click',()=>{if(year<9998){year++;void loadCalendar();}});
 // Canonical quality filters include native global search and source filename.
 let previewRead=Promise.resolve();
 const savedQualityPeriods=new Set();
 const qualityActions=el('div',undefined,'actions');$('qualitySummary').after(qualityActions);
 function pendingPeriods(){
  const periods=new Set();
  if(review?.qualityPeriodMonths){
   for(const [key,mask] of Object.entries(review.qualityPeriodMonths)){
    const [type,month]=key.split(':');
    if(!kinds[type]||!/^\d{4}-\d{2}$/.test(month)||!Number.isSafeInteger(mask)||mask<0)continue;
    for(let n=1;n<=31;n++){const date=month+'-'+String(n).padStart(2,'0');if((mask&(1<<(n-1)))&&validDate(date))periods.add(type+':'+date);}
   }
  }else for(const r of review?.sampleReports||[])if(kinds[r.kind]&&validDate(r.start))periods.add(r.kind+':'+r.start);
  return periods;
 }
 function syncQualityCalendar(){
  const field=$('qualityPeriod'),all=field.children[0];
  if(all?.value==='all')all.textContent=$('qualityFrom')?.value||$('qualityTo')?.value?'Диапазон по датам':'Все периоды';
  window.referenceCalendar?.syncField(field);
 }
 function setQualityDates(from='',to=''){
  for(const [id,date] of [['qualityFrom',from],['qualityTo',to]]){
   const field=$(id);field.value=date;window.referenceCalendar?.syncField(field);
  }
  syncQualityCalendar();
 }
 function syncQualityPeriods(){
  const field=$('qualityPeriod'),previous=field.value,pending=$('qualitySource').value==='pending';
  const options=pending?pendingPeriods():savedQualityPeriods;
  const all=el('option','Все периоды');all.value='all';field.replaceChildren(all);
  for(const key of [...options].sort()){const [kind,date]=key.split(':'),o=el('option',kinds[kind]+' · '+date);o.value=key;field.append(o);}
  field.value=options.has(previous)?previous:'all';syncQualityCalendar();
  $('qualityPeriodHint').textContent=pending&&review&&(review.qualityPeriodsPartial===true||(!review.qualityPeriodMonths&&review.reportCount>(review.sampleReports?.length||0)))?'Календарь показывает часть подтверждённых периодов Excel. Другие даты задайте в полях «С» и «По».':'';
 }
 const periodHint=el('small',undefined,'muted');periodHint.id='qualityPeriodHint';$('qualityPeriod').after(periodHint);
 for(const [id,label] of [['qualitySearch','Поиск замечаний'],['qualityReportSource','Файл источника']]){const field=el('label',label),input=el('input');input.id=id;input.type='search';input.maxLength=1024;field.append(input);$('qualityType').parentElement.parentElement.append(field);}
 $('qualityType').replaceChildren();for(const [value,label] of [['all','Все проблемы'],...Object.entries(types)]){const o=el('option',label);o.value=value;$('qualityType').append(o);}
 for(const [id,title]of [['qualityFrom','С'],['qualityTo','По']]){const field=el('label',title),input=el('input');input.id=id;input.type='date';field.append(input);$('qualityType').parentElement.parentElement.append(field);input.addEventListener('change',()=>{$('qualityPeriod').value='all';syncQualityCalendar();qualityOffset=0;void loadQuality();});}
 function qualityFilter(){
  const period=$('qualityPeriod').value,exact=period==='all'?null:period.split(':');
  return {from:exact?exact[1]:$('qualityFrom').value||null,to:exact?exact[1]:$('qualityTo').value||null,report_type:exact?exact[0]:null,
   organization:$('qualityOrg').value==='all'?null:$('qualityOrg').value,kind:$('qualityType').value==='all'?null:$('qualityType').value,search:$('qualitySearch').value.trim()||null,source:$('qualityReportSource').value.trim()||null};
 }
 const repairHints={
  'empty-name':'Заполните название работы в указанной ячейке Excel и повторно проверьте файл.',
  invalid:'Проверьте количество и сумму в указанных ячейках Excel, включая формулы, пустые значения и десятичный разделитель.',
  skipped:'Проверьте название работы и ближайший заголовок подразделения в этом наборе столбцов.',
  mismatch:'Сравните контрольные количество и сумму с детализацией подразделения или всего набора. Проверьте также строки над контрольным итогом.',
  missing:'Проверьте, существует ли отчёт за этот период и подразделение. Если он есть, загрузите его.',
  duplicate:'Сверьте даты отчётов и проверьте данные перед подтверждением замены.',
  overlap:'Сверьте даты пересекающихся отчётов и повторите импорт с правильными датами.'
 };
 function qualityReason(issue,address){
  const reason=el('div',issue.reason),advice=el('details',undefined,'quality-advice');
  if(issue.expected&&issue.actual)reason.append(el('small','Количество: факт '+UI.fmt(issue.actual.count,'count')+' / итог Excel '+UI.fmt(issue.expected.count,'count')+' · Сумма: факт '+UI.fmt(issue.actual.amount,'amount')+' / итог Excel '+UI.fmt(issue.expected.amount,'amount')));
  if(Array.isArray(issue.raw))reason.append(el('small','Значения при чтении: '+issue.raw.map(v=>v??'[нет значения]').join(' | ')));
  advice.append(el('summary','Как проверить и исправить'),el('p',repairHints[issue.kind]||'Проверьте исходный лист Excel и предупреждения импорта. После исправления повторно проверьте файл.'));
  if(address){
   const copy=button('Скопировать адрес ячейки',async()=>{
    try{await navigator.clipboard.writeText(address);copy.textContent='Адрес скопирован';}
    catch{UI.notice?.('Выделите адрес в столбце «Ячейка Excel» и скопируйте вручную.',true);}
   });advice.append(copy);
  }
  reason.append(advice);return reason;
 }
 function renderQualityIssues(issues){
  const date=value=>value?.replace(/^(\d{4})-(\d{2})-(\d{2})$/,'$3.$2.$1');
  page($('qualityTable'),'Проблемы отчётов',['Период','Подразделение','Проблема','Источник','Строка Excel','Ячейка Excel','Причина'],issues.map(i=>{
   const cells=[...new Set([...(i.cells||[]),i.work_cell,i.count_cell,i.amount_cell].filter(c=>typeof c==='string'&&/^[A-Z]{1,3}[1-9]\d{0,6}$/.test(c)))];
   const sheet=!i.sheet?'':/^[A-Za-zА-Яа-яЁё0-9_]+$/.test(i.sheet)?i.sheet:"'"+i.sheet.replace(/'/g,"''")+"'";
   const address=cells.map(c=>sheet?sheet+'!'+c:c).join(', ');
   const period=i.date?date(i.date):i.period_start===i.period_end?date(i.period_start):[date(i.period_start),date(i.period_end)].filter(Boolean).join(' — ');
   return [period||'Даты не сохранены',i.organization||'Не указано / общий итог',types[i.kind]||i.kind,[i.source_name,i.sheet,i.source_anchor?'Отчёт хранения: '+i.source_anchor:null].filter(Boolean).join(' · ')||'Отчёт отсутствует',i.source_row||i.row,address||'—',qualityReason(i,address)];
  }),[null,null,null,null,'Номер строки исходного листа, если он сохранён при проверке или импорте.',
   'Адреса ячеек на исходном листе, если они сохранены. При расхождении итогов указаны контрольные ячейки. Причина расхождения может находиться в строках детализации.',null]);
 }
 async function loadQuality(){
  const rev=UI.revision,ticket=++qualityTicket,pending=$('qualitySource').value==='pending',checked=review;
  qualityActions.replaceChildren();$('qualityTable').replaceChildren();
  status('qualityPage','');status('qualitySummary','Загрузка замечаний…');
  if(pending&&!checked){window.sqlRowScroll?.reset($('qualityTable'));status('qualitySummary','Сначала проверьте Excel.');return;}
  if(!Number.isSafeInteger(rev))return;
  const filter=qualityFilter(),query=pending?{source:sourcePath,selection:{format:$('importFormat').value==='daily-wide'?'daily-wide':'legacy',options},reviewToken:checked.reviewToken,filter}:null;
  const current=()=>live(rev,ticket,qualityTicket)&&(!pending||review===checked);
  try{
   const offset=qualityOffset;
   let response;
   if(pending){
    // Reopening a large workbook takes time. Coalesce edits made while an
    // earlier scan is running and execute only the latest surviving request.
    response=previewRead.then(()=>current()?api.nativePreviewQualityPage(query,offset):null);
    previewRead=response.then(()=>{},()=>{});
   }else response=api.unifiedQualityPage(rev,filter,offset,LIMIT);
   const result=await response;
   if(!current())return;
   if(!Array.isArray(result.issues)||result.issues.length>LIMIT)throw Error('Некорректная страница качества');
   renderQualityIssues(result.issues);
   const categories=Object.entries(result.counts).map(([key,n])=>(types[key]||(key==='other'?'Другие замечания':key))+': '+n).join(' · ');
   const found=(pending?'Предпросмотр · ':'')+'Найдено: '+result.total;
   status('qualitySummary',[found,categories,'Отсутствие замечаний само по себе не подтверждает полноту'].filter(Boolean).join('. ')+'.');
   if(result.total){
    const save=button('Скачать CSV',async()=>{
     if(!current()||save.disabled)return;save.disabled=true;
     try{
      const destination=await api.chooseCsvPath('analiz-quality.csv');
      if(!destination||!current())return;
      if(pending)await api.nativePreviewQualityCsv(query,destination);
      else await api.unifiedQualityCsv(rev,filter,destination);
      UI.notice?.('Замечания сохранены в CSV.');
     }catch(error){UI.notice?.('Не удалось сохранить CSV: '+String(error.message||error),true);}
     finally{save.disabled=false;if(save.isConnected&&current())save.focus();}
    });
    save.id='qualityExportCsv';qualityActions.append(save);
   }
   const pageLabel=$('qualityPage');pageLabel.textContent='';
   window.sqlRowScroll.attach($('qualityTable'),{index:Math.floor(offset/LIMIT),size:LIMIT,count:result.issues.length,total:result.total,footer:pageLabel.parentElement,label:'Страницы замечаний к Excel',go:i=>{
    if(!current())return false;
    qualityOffset=i*LIMIT;return loadQuality();
   }});
  }catch(error){if(current()){window.sqlRowScroll?.reset($('qualityTable'));status('qualitySummary','Не удалось прочитать замечания: '+String(error.message||error));$('qualityTable').replaceChildren();}}
 }
 for(const id of ['qualitySource','qualityPeriod','qualityOrg','qualityType','qualityReportSource','qualitySearch'])$(id).addEventListener(id==='qualitySearch'||id==='qualityReportSource'?'input':'change',()=>{qualityOffset=0;if(id==='qualitySource')syncQualityPeriods();if(id==='qualityPeriod')setQualityDates();void loadQuality();});
 // The canonical file input is intercepted before the browser opens a File:
 // Rust needs the real path and owns parsing, review tokens and atomic writes.
 const acknowledgements=el('div',undefined,'import-date-grid');acknowledgements.id='unifiedImportAcknowledgements';acknowledgements.hidden=true;
 for(const id of ['unifiedAcceptOverlap','unifiedAcceptQuality']){const label=el('label',undefined,'checkline'),input=el('input');label.hidden=true;input.type='checkbox';input.id=id;label.append(input,el('span'));acknowledgements.append(label);}
 $('importPreview').before(acknowledgements);
 function syncAcknowledgements(result){
  const c=result?.conflicts||{},q=result?.quality||{},overlap=$('unifiedAcceptOverlap'),quality=$('unifiedAcceptQuality');
  const reasons=[['замечаний в отчётах',q.issues],['отчётов с расхождениями',q.mismatch_reports],['значимых замечаний источника',q.relevant_source_issues]].filter(([,count])=>count>0);
  const replace=$('replaceExisting');replace.checked=false;replace.parentElement.hidden=!(c.replacements>0);
  overlap.checked=quality.checked=false;overlap.parentElement.hidden=!(c.cross_type_overlaps>0);quality.parentElement.hidden=!reasons.length;
  overlap.parentElement.children[1].textContent='Подтверждаю пересечения типов отчётов: '+show(c.cross_type_overlaps)+'. Периоды пересекаются с отчётами другого типа. Проверьте даты и типы перед сохранением.';
  quality.parentElement.children[1].textContent='Подтверждаю найденные замечания качества: '+reasons.map(([name,count])=>name+' — '+show(count)).join('; ')+'. Просмотрите замечания в предварительном просмотре перед сохранением.';
  acknowledgements.hidden=overlap.parentElement.hidden&&quality.parentElement.hidden;
 }
 const importStatus=el('p',undefined,'muted small');importStatus.id='unifiedImportStatus';importStatus.setAttribute('aria-live','polite');$('importPreview').before(importStatus);
 function importLock(){for(const id of ['excelFile','importFormat','importType','importWeek','importMonth','sheetSelect','dailyYear','importMappingMode','replaceExisting','unifiedAcceptOverlap','unifiedAcceptQuality'])$(id).disabled=importBusy||(id==='sheetSelect'&&!sourcePath)||(id==='importType'&&$('importFormat').value==='daily-wide');$('inspectFile').disabled=importBusy||!sourcePath;$('saveImport').disabled=importBusy||!review;$('addImportMapping').disabled=importBusy;$('saveDailyFolderRule').disabled=importBusy||!review;for(const root of [$('importColumnMappings'),$('importPeriodDates'),$('dailySheetChoices')])for(const control of root.querySelectorAll('input,select,textarea,button'))control.disabled=importBusy;}
 function resetReview(message){review=null;options=null;$('importPreview').replaceChildren();if(uploadMode==='quality'&&$('qualitySource').value==='pending'){qualityOffset=0;syncQualityPeriods();void loadQuality();}$('saveDailyFolderRule').hidden=true;$('saveImport').disabled=true;syncAcknowledgements();if(message)status('unifiedImportStatus',message);}
 function changed(){++parameterTicket;resetReview('Настройки изменены. Проверьте Excel заново.');}
 function periodLayoutChanged(){
  $('importPeriodDates').replaceChildren(el('p','Даты наборов будут показаны после проверки Excel.','muted small'));
  changed();
 }
 function formatChanged(){const wide=$('importFormat').value==='daily-wide';if($('ordinaryImportHint'))$('ordinaryImportHint').hidden=wide;$('sheetSelect').parentElement.hidden=wide;$('importType').parentElement.hidden=wide;if($('importAdvanced'))$('importAdvanced').hidden=wide;$('dailyWideOptions').hidden=!wide;$('importType').disabled=wide||importBusy;$('weekStartLabel').hidden=wide||$('importType').value==='month';$('monthStartLabel').hidden=wide||$('importType').value!=='month';$('importDateCaption').textContent=$('importType').value==='day'?'Дата первого дня':'Начало первой недели';periodLayoutChanged();}
 function mappingRow(value={}){
  const row=el('div',undefined,'import-date-grid');row.dataset.mapping='true';
  for(const [key,label,placeholder] of [['header_row','Строка заголовка','1'],['work_col','Работа','A'],['count_col','Количество','B'],['amount_col','Сумма','C']]){const field=el('label',label),input=el('input');input.dataset.field=key;input.value=value[key]||'';input.placeholder=placeholder;input.maxLength=6;field.append(input);row.append(field);input.addEventListener('input',periodLayoutChanged);}
  row.append(button('Удалить набор',()=>{row.remove();periodLayoutChanged();}));$('importColumnMappings').append(row);return row;
 }
 function columnName(n){let text='';for(;n>0;n=Math.floor((n-1)/26))text=String.fromCharCode(65+(n-1)%26)+text;return text;}
 function column(text){if(!/^[A-Z]{1,3}$/i.test(text.trim()))throw Error('Укажите буквы столбца Excel.');let n=0;for(const c of text.trim().toUpperCase())n=n*26+c.charCodeAt(0)-64;if(n>2048)throw Error('Столбец вне допустимого диапазона.');return n;}
 function mappings(){
  if($('importMappingMode').value!=='manual')return null;
  const rows=[...$('importColumnMappings').querySelectorAll('[data-mapping]')],used=new Set();
  if(!rows.length||rows.length>64)throw Error('Укажите от 1 до 64 наборов столбцов.');
  return rows.map(row=>{
   const read=k=>row.querySelector('[data-field="'+k+'"]').value.trim(),header=Number(read('header_row'));
   if(!Number.isInteger(header)||header<1||header>60000)throw Error('Укажите номер строки заголовка от 1 до 60000.');
   const m={header_row:header,work_col:column(read('work_col')),count_col:column(read('count_col')),amount_col:column(read('amount_col'))};
   const columns=[m.work_col,m.count_col,m.amount_col];
   if(new Set(columns).size!==3)throw Error('Работа, количество и сумма должны находиться в разных столбцах.');
   for(const col of columns){
    if(used.has(col))throw Error('Столбец '+columnName(col)+' указан в нескольких наборах.');
    used.add(col);
   }
   return m;
  });
 }
 $('addImportMapping').addEventListener('click',()=>{mappingRow();periodLayoutChanged();});
 $('importMappingMode').addEventListener('change',()=>{$('importManualWrap').hidden=$('importMappingMode').value!=='manual';if(!$('importColumnMappings').children.length)mappingRow();periodLayoutChanged();});
 for(const id of ['importFormat','importType'])$(id).addEventListener('change',formatChanged);
 for(const id of ['importWeek','importMonth','sheetSelect'])$(id).addEventListener('change',periodLayoutChanged);
 $('dailyYear').addEventListener('change',changed);
 function updateWideSheetSummary(){
  const explicit=$('unifiedWideSheetMode').value==='selected',chosen=wideSheetRows.filter(row=>row.box.checked);
  const summary=$('unifiedWideSheets');summary.value=explicit?chosen.map(row=>row.name).join('; '):'';summary.hidden=!explicit;
  $('unifiedSequentialHint').hidden=!explicit||!chosen.some(row=>row.period.value!=='dates');
 }
 function renderWideSheetChoices(found){
  const root=$('dailySheetChoices');root.replaceChildren();wideSheetRows=[];
  root.append(el('p','Можно загрузить несколько листов за одну проверку, например четыре квартала с дневными значениями. Пропуски не считаются нулями.','muted small'));
  const label=el('label','Листы для загрузки'),mode=el('select');mode.id='unifiedWideSheetMode';
  for(const [value,text] of [['auto','Все подходящие дневные листы'],['selected','Только отмеченные листы']]){const option=el('option',text);option.value=value;mode.append(option);}mode.value='auto';label.append(mode);root.append(label);
  mode.addEventListener('change',()=>{updateWideSheetSummary();changed();});
  const summary=el('input');summary.id='unifiedWideSheets';summary.readOnly=true;summary.setAttribute('aria-label','Отмеченные листы');root.append(summary);
  for(const [i,sheet] of found.entries()){
   const row=el('div',undefined,'daily-sheet-option'),checkLabel=el('label',undefined,'checkline'),box=el('input');box.type='checkbox';box.id='unifiedWideSheet'+i;
   checkLabel.append(box,el('span',sheet.name));row.append(checkLabel);
   const periodLabel=el('label','Даты дневных колонок'),period=el('select');period.id='unifiedWidePeriod'+i;
   for(const [value,text] of [['dates','Даты указаны в Excel'],['q1','Последовательно: I квартал'],['q2','Последовательно: II квартал'],['q3','Последовательно: III квартал'],['q4','Последовательно: IV квартал'],['year','Последовательно: весь год']]){const option=el('option',text);option.value=value;period.append(option);}period.value='dates';periodLabel.append(period);row.append(periodLabel);
   wideSheetRows.push({name:sheet.name,box,period});
   box.addEventListener('change',()=>{mode.value='selected';updateWideSheetSummary();changed();});
   period.addEventListener('change',()=>{if(period.value!=='dates')yearDetails.open=true;box.checked=true;mode.value='selected';updateWideSheetSummary();changed();});root.append(row);
  }
  const hint=el('p','Для последовательных колонок укажите год выше и квартал каждого отмеченного листа. Требуется ровно одна пара «Количество / Сумма» на каждый день; итог квартала по дням не распределяется. Даты и количество отчётов проверьте до сохранения.','muted small');hint.id='unifiedSequentialHint';root.append(hint);
  const wideMap=el('textarea');wideMap.id='unifiedWideMapping';wideMap.rows=3;wideMap.maxLength=8192;wideMap.placeholder='Ручные столбцы по листам: Имя листа; строка заголовка; работа; первый день; итог количества; итог суммы';wideMap.addEventListener('input',changed);wideManual.replaceChildren(el('summary','Указать столбцы дневного листа вручную'),wideMap);
  for(const sheet of found.filter(sheet=>mappingSheetLabel(sheet.name)!==sheet.name)){
   wideManual.append(button('Добавить строку для «'+sheet.name+'»',()=>{
    wideMap.value+=(wideMap.value.trim()?'\n':'')+mappingSheetLabel(sheet.name)+'; 1; A; B';
    changed();wideMap.focus();
   }));
  }
  root.append(wideManual);
  updateWideSheetSummary();return wideMap;
 }
 function applyFolderRule(rule,found,wideMap){
  if(rule.format==='daily-wide'||rule.format==='auto'&&found.some(sheet=>sheet.dailyWide)){
   $('importFormat').value='daily-wide';
   $('dailyYear').value=String(rule.year||'');yearDetails.open=!!rule.year;
   const available=new Set(found.map(sheet=>sheet.name));
   const mappings=Object.entries(rule.mappings||{});
   const missing=[...(rule.sheets||[]),...mappings.map(([name])=>name)].some(name=>!available.has(name));
   if(rule.sheets?.length||missing){
    $('unifiedWideSheetMode').value='selected';
    for(const row of wideSheetRows)row.box.checked=!missing&&rule.sheets.includes(row.name);
    updateWideSheetSummary();
   }
   wideMap.value=mappings.filter(([name])=>available.has(name)).map(([name,m])=>
    [mappingSheetLabel(name),m.headerRow+1,columnName(m.workCol+1),columnName(m.firstDayCol+1),
     m.totalCountCol===undefined?'':columnName(m.totalCountCol+1),
     m.totalAmountCol===undefined?'':columnName(m.totalAmountCol+1)].join('; ')).join('\n');
   formatChanged();
   if(missing)status('unifiedImportStatus','Лист из сохранённого правила отсутствует. Отметьте листы для проверки и при необходимости уточните столбцы.');
   return;
  }
  $('importFormat').value='legacy';$('importType').value=rule.type||'week';
  $('importWeek').value=rule.start||'';$('importMonth').value=(rule.start||'').slice(0,7);
  const selected=rule.sheet?found.find(sheet=>sheet.name===rule.sheet):found.length===1?found[0]:null;
  const select=$('sheetSelect');
  if(!selected){
   const prompt=el('option','Выберите лист…');prompt.value='';prompt.disabled=true;select.prepend(prompt);
  }
  select.value=selected?.component||'';formatChanged();
  if(!selected)status('unifiedImportStatus',rule.sheet?'Лист из сохранённого правила отсутствует. Выберите лист для проверки.':'В книге несколько листов. Выберите лист для проверки.');
 }
 function clearSourceOverrides(){
  $('importWeek').value='';$('importMonth').value='';$('dailyYear').value='';
  $('importMappingMode').value='auto';$('importColumnMappings').replaceChildren();$('importManualWrap').hidden=true;
  $('replaceExisting').checked=false;$('importPeriodDates').replaceChildren();
 }
 async function chooseSource(event,requestedPath=null){
  event?.preventDefault();event?.stopImmediatePropagation?.();if(importBusy)return;
  let ticket=sourceTicket,retargeting=false;importBusy=true;importLock();
  try{const path=requestedPath||await api.chooseWideXlsx();if(ticket!==sourceTicket)return;if(!path){status('unifiedImportStatus','Выбор файла отменён.');return;}
   ticket=++sourceTicket;retargeting=true;sourceFolderContext=null;resetReview();const previousPath=sourcePath;
   sourcePath=null;sheets=[];
   const context=await api.nativeImportFolderContext(path);if(ticket!==sourceTicket)return;
   const rule=context?.rule;
   if(rule||(previousPath&&previousPath!==path))clearSourceOverrides();
   const found=await api.nativeImportSheets(path,rule?.format==='auto'?rule:null);if(ticket!==sourceTicket)return;if(!Array.isArray(found)||!found.length||found.length>64||found.some(sheet=>typeof sheet.name!=='string'||!sheet.name||typeof sheet.component!=='string'||!sheet.component)||new Set(found.map(sheet=>sheet.name)).size!==found.length)throw Error('В XLSX не найдены доступные листы.');
   sourcePath=path;sourceFolderContext=context;sheets=found;$('sheetSelect').replaceChildren();for(const sheet of found){const o=el('option',sheet.name);o.value=sheet.component;$('sheetSelect').append(o);}
   $('sheetSelect').value=found[0].component;
   status('excelFileStatus',path.split(/[\\/]/).pop());status('unifiedImportStatus','Файл выбран. Проверьте лист и даты, затем нажмите «Проверить Excel».');
   const wideMap=renderWideSheetChoices(found);
   $('importPeriodDates').replaceChildren(el('p','Даты наборов будут показаны после проверки Excel.','muted small'));
   if(rule?.format==='auto'&&found.some(sheet=>typeof sheet.dailyWide!=='boolean'))throw Error('Нативное распознавание формата правила недоступно. Проверьте файл заново.');
   if(rule)applyFolderRule(rule,found,wideMap);

  }catch(error){if(ticket!==sourceTicket)return;if(retargeting){clearSourceOverrides();sourcePath=null;sourceFolderContext=null;sheets=[];$('sheetSelect').replaceChildren();status('excelFileStatus','Выберите файл');}status('unifiedImportStatus','Не удалось выбрать Excel: '+String(error.message||error));}
  finally{importBusy=false;importLock();}
 }
 $('excelFile').addEventListener('click',chooseSource);
 function validDate(text){return /^\d{4}-\d{2}-\d{2}$/.test(text)&&Number.isFinite(Date.parse(text+'T00:00:00Z'))&&new Date(text+'T00:00:00Z').toISOString().slice(0,10)===text;}
 // Preserve native sheet names when the editor uses one mapping per line.
 function mappingSheetLabel(name){return /[\r\n]/.test(name)||name.startsWith('"')?JSON.stringify(name):name;}
 function wideMappingFields(line){
  const fields=line.split(';');
  // The suffix has a header row, two mandatory columns and up to two totals.
  // Match the full workbook sheet name, which can itself contain semicolons.
  for(let count=3;count<=5&&count<fields.length;count++){
   if(!Number.isFinite(Number(fields.at(-count))))continue;
   const prefix=fields.slice(0,-count).join(';');
   const sheet=sheets.find(s=>mappingSheetLabel(s.name)===prefix)||sheets.find(s=>mappingSheetLabel(s.name)===prefix.trim());
   if(sheet)return [sheet.name,...fields.slice(-count).map(s=>s.trim())];
  }
  throw Error('Проверьте имя листа и столбцы в ручной настройке.');
 }
 function getOptions(){
  const orgs=UI.snapshot?.org_names;if(!Array.isArray(orgs)||orgs.length!==4||orgs.some(v=>typeof v!=='string'||!v.trim()))throw Error('Настройте четыре подразделения.');
  if($('importFormat').value==='daily-wide'){
   const value=$('dailyYear').value.trim(),year=value?Number(value):null;if(year!==null&&(!Number.isInteger(year)||year<1900||year>9999))throw Error('Укажите год от 1900 до 9999.');
   const explicit=$('unifiedWideSheetMode').value==='selected',chosen=explicit?wideSheetRows.filter(row=>row.box.checked).map(row=>row.name):[],map=Object.create(null),sequential_periods=Object.create(null);if(explicit&&!chosen.length)throw Error('Отметьте хотя бы один лист.');if(chosen.length>12)throw Error('Не более 12 листов за импорт.');
   for(const row of wideSheetRows)if(explicit&&row.box.checked&&row.period.value!=='dates'){
    if(year===null)throw Error('Подтвердите год для последовательных дневных колонок.');
    if(!['q1','q2','q3','q4','year'].includes(row.period.value))throw Error('Укажите корректный квартал.');
    sequential_periods[row.name]={year,quarter:row.period.value==='year'?null:Number(row.period.value.slice(1))};
   }
   for(const line of ($('unifiedWideMapping')?.value||'').split(/\r?\n/).filter(s=>s.trim())){
    const [name,header,work,day,totalCount,totalAmount]=wideMappingFields(line);
    if(Object.hasOwn(map,name))throw Error('Лист «'+name+'» указан в ручной настройке дважды. Оставьте одну строку для этого листа.');
    if(explicit&&!chosen.includes(name))throw Error('Отметьте лист «'+name+'» или удалите его строку из ручной настройки.');
    if(totalAmount&&!totalCount)throw Error('Для листа «'+name+'» укажите столбец итогового количества вместе с итоговой суммой.');
    if(!header||!work||!day)throw Error('Неполная ручная настройка листа.');
    const n=Number(header);if(!Number.isInteger(n)||n<1||n>100000)throw Error('Укажите номер заголовка листа.');
    map[name]={header_row:n,work_col:column(work),first_day_col:column(day),total_count_col:totalCount?column(totalCount):null,total_amount_col:totalAmount?column(totalAmount):null};
   }
   return {year,sheets:chosen,mappings:map,organizations:orgs.slice(),sequential_periods};
  }
  const kind=$('importType').value,start=kind==='month'?$('importMonth').value+'-01':$('importWeek').value;if(!validDate(start))throw Error('Укажите первую дату отчётного периода.');
  if(!sheets.some(sheet=>sheet.component===$('sheetSelect').value))throw Error('Выберите лист для проверки.');
  const mapped=mappings(),result={kind,start,sheet:$('sheetSelect').value,organizations:orgs.slice(),mappings:mapped};
  const dates=[...$('importPeriodDates').querySelectorAll('input[data-ordinal]')];if(dates.some(input=>input.value!==input.dataset.original)){result.periodDates=dates.map(input=>{if(!validDate(input.value))throw Error('Некорректная дата набора.');return input.value;});}
  return result;
 }
 function renderReview(result){
  syncAcknowledgements(result);
  const c=result.conflicts||{},q=result.quality||{},root=$('importPreview');root.replaceChildren();const summary=el('div',undefined,'kpi-grid');
  const cells=result.processedCellCount;
  const metrics=[
   ['Отчётов',result.reportCount], ['Строк работ',result.rowCount],
   ['Обработано ячеек в книге',Number.isSafeInteger(cells)&&cells>=0?cells.toLocaleString('ru-RU'):null],
   ['Новых отчётов',c.new_reports], ['Замен',c.replacements], ['Пересечений',c.cross_type_overlaps],
   ['Ошибок',q.issues], ['Замечаний источника',q.relevant_source_issues]
  ];
  for(const [title,value] of metrics){
   const box=el('div',undefined,'kpi');
   box.append(el('strong',show(value)),el('span',title));
   summary.append(box);
  }
  root.append(summary);
  const rows=el('div');page(rows,'Найденные периоды — ограниченная выборка',['Период','До','Тип','Строк','Замечаний'],(result.sampleReports||[]).map(r=>[r.start,r.end,kinds[r.kind]||r.kind,r.rows,r.issues]));root.append(rows);if(result.periodRange)root.prepend(el('p','Распознаны периоды: '+result.periodRange.from+' — '+result.periodRange.to+' · '+(kinds[result.periodRange.kind]||result.periodRange.kind)));
  root.append(button('Просмотреть все замечания',()=>{ $('qualitySource').value='pending';syncQualityPeriods();return openUploadView('quality');}));
  for(const issue of [...(result.sampleReportIssues||[]),...(result.sampleSourceIssues||[])])root.append(el('p',(issue.reason||issue.type||'Замечание')+' · '+[issue.sheet,...(issue.cells||[]),issue.work_cell,issue.count_cell,issue.amount_cell].filter(Boolean).join(' · ')));
  $('importPeriodDates').replaceChildren();if($('importFormat').value!=='daily-wide'){
   if(!Number.isSafeInteger(result.reportCount)||result.reportCount>684)throw Error('Слишком много наборов дат.');
   const base=options.start,kind=options.kind;
   for(let i=0;i<result.reportCount;i++){const date=new Date(base+'T12:00:00Z');if(kind==='month')date.setUTCMonth(date.getUTCMonth()+i);else date.setUTCDate(date.getUTCDate()+i*(kind==='week'?7:1));const initial=options.periodDates?.[i]||date.toISOString().slice(0,10),label=el('label','Набор '+(i+1)),input=el('input');input.type='date';input.value=initial;input.dataset.ordinal=String(i);input.dataset.original=options.periodDates?date.toISOString().slice(0,10):initial;input.addEventListener('change',changed);label.append(input);$('importPeriodDates').append(label);}
  }
  const sequential=Object.keys(options.sequential_periods||{}).length>0;
  $('saveDailyFolderRule').hidden=$('importFormat').value!=='daily-wide'||!api.nativeSetRuleChecked||!sourceFolderContext||sequential;
  if(sequential)root.append(el('p','Последовательные дневные колонки загружаются вручную после проверки. Настройка автоимпорта для них не сохраняется.','muted small'));
  if(result.catalogEntries>0)root.append(el('p','Услуг в справочнике Excel: '+result.catalogEntries+'. Новые названия будут добавлены вместе с отчётами.'));

  status('unifiedImportStatus','Проверка завершена. Просмотрите периоды, замены, пересечения и замечания до сохранения.');
 }
 function sameFolderRule(left,right){
  if(left===right)return true;
  if(!left||!right||typeof left!=='object'||typeof right!=='object')return false;
  const keys=Object.keys(left);
  return keys.length===Object.keys(right).length&&keys.every(key=>Object.hasOwn(right,key)&&sameFolderRule(left[key],right[key]));
 }
 async function refreshRuleContext(source,rev){
  const previous=sourceFolderContext;
  if(!previous||rev<=previous.revision)return;
  const fresh=await api.nativeImportFolderContext(source);
  if(previous!==sourceFolderContext||!fresh||fresh.revision!==rev||fresh.folder!==previous.folder||fresh.name!==previous.name||
     fresh.nativeSignature!==previous.nativeSignature||!sameFolderRule(previous.rule??null,fresh.rule??null)){
   throw Error('Файл, папка или его правило изменились. Выберите файл заново.');
  }
  sourceFolderContext=fresh;
 }
 $('inspectFile').addEventListener('click',async()=>{
  if(importBusy||!sourcePath)return;importBusy=true;resetReview();importLock();const rev=UI.revision,ticket=sourceTicket,parameters=parameterTicket,epoch=generation;
  const assertCurrent=()=>{if(epoch!==generation||ticket!==sourceTicket||UI.revision!==rev||parameters!==parameterTicket)throw Error('Данные или файл изменились. Проверьте Excel заново.');};
  try{const wide=$('importFormat').value==='daily-wide';await refreshRuleContext(sourcePath,rev);assertCurrent();options=getOptions();const result=await (wide?api.nativeWideImportPreview(sourcePath,options):api.nativeLegacyImportPreview(sourcePath,options));assertCurrent();if(!result||typeof result.reviewToken!=='string'||!/^[a-f0-9]{64}$/.test(result.reviewToken))throw Error('Некорректный токен проверки импорта.');review=result;renderReview(result);if(uploadMode==='quality'&&$('qualitySource').value==='pending'){syncQualityPeriods();void loadQuality();}}
  catch(error){resetReview();if($('importFormat').value==='daily-wide'){if(/year|год/i.test(String(error.message||error)))yearDetails.open=true;else wideManual.open=true;}else if($('importAdvanced'))$('importAdvanced').open=true;status('unifiedImportStatus','Не удалось проверить Excel: '+String(error.message||error));$('importPreview').replaceChildren();}
  finally{importBusy=false;importLock();}
 });
 $('saveImport').addEventListener('click',async()=>{
  if(importBusy||!review||!sourcePath||!options)return;
  const c=review.conflicts||{},q=review.quality||{},replace=$('replaceExisting').checked,overlap=$('unifiedAcceptOverlap').checked,quality=$('unifiedAcceptQuality').checked;
  if((c.replacements>0&&!replace)||(c.cross_type_overlaps>0&&!overlap)||((q.issues>0||q.mismatch_reports>0||q.relevant_source_issues>0)&&!quality)){status('unifiedImportStatus','Подтвердите найденные замены, пересечения и замечания.');return;}
  const checked={revision:UI.revision,generation,sourceTicket,parameterTicket,source:sourcePath,options,review,wide:$('importFormat').value==='daily-wide'};
  const current=()=>Number.isSafeInteger(checked.revision)&&UI.revision===checked.revision&&generation===checked.generation&&sourceTicket===checked.sourceTicket&&parameterTicket===checked.parameterTicket&&sourcePath===checked.source&&options===checked.options&&review===checked.review;
  importBusy=true;importLock();
  try{if(await window.confirm('Повторно проверить Excel и атомарно сохранить '+checked.review.reportCount+' '+reportNoun(checked.review.reportCount)+(checked.review.catalogEntries>0?' и новые названия из справочника Excel':'')+'?')!==true){status('unifiedImportStatus','Сохранение отменено.');return;}
   if(!current())throw Error('Данные, файл или параметры изменились. Проверьте Excel заново.');
   let backupDestination=null;if(c.replacements>0){backupDestination=await api.chooseSqliteBackupPath('analiz-before-import-'+new Date().toISOString().slice(0,10)+'.sqlite3');if(!backupDestination){status('unifiedImportStatus','Сохранение отменено: для замены требуется резервная копия SQLite.');return;}}
   if(!current())throw Error('Данные, файл или параметры изменились. Проверьте Excel заново.');
   const flags={replaceExact:replace,acceptCrossType:overlap,acceptQuality:quality,backupDestination};const result=await (checked.wide?api.nativeWideImportApply(checked.source,checked.options,checked.review.reviewToken,flags):api.nativeLegacyImportApply(checked.source,checked.options,checked.review.reviewToken,flags));
    // This commit changed reports only. Keep the verified book and its file
    // signature available if the user previews it again to save a folder rule.
    if(sourceFolderContext?.revision===checked.revision&&Number.isSafeInteger(result.revision))
     sourceFolderContext={...sourceFolderContext,revision:result.revision};
    resetReview();$('importPreview').replaceChildren();
    const saved='Сохранено отчётов: '+result.reports+' · Строк работ: '+result.rows+' · Ревизия: '+result.revision+'.';
    try{await UI.refresh();status('unifiedImportStatus',saved);}
    catch(error){status('unifiedImportStatus',saved+' Не удалось обновить отображение: '+String(error.message||error)+'. Нажмите «Обновить данные».');}
    const range=checked.review.periodRange;if(range&&validDate(range.from)&&validDate(range.to))$('importPreview').append(button('Открыть загруженный период',()=>{window.referenceFilters.openImportedPeriod(range);window.referenceDesktop.navigate('s1');}));
  }catch(error){resetReview();status('unifiedImportStatus','Не сохранено: '+String(error.message||error)+'. Проверьте Excel заново.');}
  finally{importBusy=false;importLock();}
 });
 $('saveDailyFolderRule').addEventListener('click',async()=>{
  if(importBusy||!review||!options||!sourcePath)return;importBusy=true;importLock();
  try{if(Object.keys(options.sequential_periods||{}).length)throw Error('Последовательные дневные колонки требуют ручной проверки перед импортом.');
   if(!sourceFolderContext)throw Error('Выбранный файл должен находиться в подключённой папке Excel.');
   const {name,revision,folder,nativeSignature}=sourceFolderContext;
   const rule={type:'day',format:'daily-wide',start:'',sheet:'',...(options.year?{year:options.year}:{}),...(options.sheets?.length?{sheets:options.sheets}:{} )};
   if(Object.keys(options.mappings||{}).length){rule.mappings=Object.create(null);for(const [name,m]of Object.entries(options.mappings)){rule.mappings[name]={workCol:m.work_col-1,headerRow:m.header_row-1,firstDayCol:m.first_day_col-1,...(m.total_count_col!==null?{totalCountCol:m.total_count_col-1}:{}),...(m.total_amount_col!==null?{totalAmountCol:m.total_amount_col-1}:{})};}}
   const savedRevision=await api.nativeSetRuleChecked(name,rule,revision,folder,nativeSignature);
   if(Number.isSafeInteger(savedRevision))sourceFolderContext={...sourceFolderContext,revision:savedRevision,rule};
    try{await window.unifiedReports.refreshFolder?.();await UI.refresh();status('unifiedImportStatus','Правило файла сохранено. Автоимпорт включается отдельно на текущий сеанс.');}
    catch(error){status('unifiedImportStatus','Правило файла сохранено в SQLite, но не удалось обновить отображение: '+String(error.message||error)+'. Нажмите «Обновить данные».');}
  }catch(error){status('unifiedImportStatus','Правило не сохранено: '+String(error.message||error));}finally{importBusy=false;importLock();}
 });
 function folderControls(){
  if(!api.list||!api.chooseFolder)return;
  const panel=el('details',undefined,'panel');panel.id='desktopSource';panel.append(el('summary','Папка и автоимпорт'));
  window.unifiedReports.openSource=async path=>{await openUploadView('import');return chooseSource(null,path);};
  function text(tag,id,value){const node=el(tag,value);node.id=id;panel.append(node);return node;}
  const label=el('label',undefined,'checkline'),toggle=el('input');toggle.id='sourceAutoImport';toggle.type='checkbox';label.append(toggle,el('span','Автоматически добавлять новые проверенные отчёты в этом сеансе'));panel.append(label);
  text('p','sourceAutoStatus','По умолчанию выключено. Замены, неоднозначные файлы и замечания требуют ручной проверки.');
  const actions=el('div',undefined,'actions');for(const [id,title] of [['sourceFolderChoose','Выбрать папку Excel'],['sourceFolderRefresh','Проверить файлы']]){const b=button(title,()=>{});b.id=id;actions.append(b);}panel.append(actions);
  for(const [id,value] of [['sourceFolderLocation','Папка не выбрана.'],['sourceFolderCount',''],['sourceFolderStatus',''],['sourceRuleFile','Выберите файл для настройки правила.']])text('p',id,value);
  const editor=el('details');editor.id='sourceRuleEditor';editor.append(el('summary','Правило выбранного файла'));panel.append(editor);const fields=el('div',undefined,'import-grid');
  for(const [id,title,options] of [['sourceRuleFormat','Формат',[['legacy','Обычные периоды'],['auto','Автоматически'],['daily-wide','Дневные данные за квартал или год']]],['sourceRuleType','Тип отчёта',[['day','День'],['week','Неделя'],['month','Месяц']]]]){const label=el('label',title),s=el('select');s.id=id;for(const [v,t]of options){const o=el('option',t);o.value=v;s.append(o);}label.append(s);fields.append(label);}
  for(const [id,title,type] of [['sourceRuleStart','Первая дата','date'],['sourceRuleSheet','Лист Excel','text'],['sourceRuleYear','Год дневной таблицы','number']]){const label=el('label',title),input=el('input');input.id=id;input.type=type;label.append(input);fields.append(label);}editor.append(fields);
  const save=button('Сохранить правило',()=>{});save.id='sourceRuleSave';editor.append(save);text('p','sourceRuleStatus','Правила сохраняются с проверкой ревизии папки и файла.');
  const wrap=el('div',undefined,'table-scroll'),table=el('table'),head=el('thead'),tr=el('tr');for(const title of ['Файл','Размер','Правило импорта','Действие'])tr.append(el('th',title));head.append(tr);table.append(head);const rows=el('tbody');rows.id='sourceFolderRows';table.append(rows);wrap.append(table);panel.append(wrap);
  if(uploadPanels.import)uploadPanels.import.append(panel);else $('upload').append(panel);
  const folderEntry=button('Папка и автоимпорт',()=>{panel.open=true;panel.scrollIntoView?.({block:'start',behavior:'smooth'});});folderEntry.id='unifiedOpenFolder';$('excelFileStatus').parentElement.parentElement.append(folderEntry);
  // Reuse the audited session/lease and checked-rule controllers after their
  // canonical controls exist. No alternate workspace or browser importer.
  const auto=document.createElement('script');auto.src='native-folder-auto-import.js';auto.onerror=()=>UI.notice('Не удалось загрузить управление автоимпортом. Перезапустите приложение.',true);auto.onload=()=>{const folder=document.createElement('script');folder.src='native-folder-status.js';folder.onerror=()=>UI.notice('Не удалось загрузить список файлов Excel. Перезапустите приложение.',true);document.head.append(folder);};document.head.append(auto);
 }
 window.nativeWorkspaceRefresh=async()=>{await UI.refresh();return true;};
 UI.onSnapshot(snapshot=>{
  const folder=activeWorkspace();
  if(folder!==workspaceFolder){
   workspaceFolder=folder;++sourceTicket;sourcePath=null;sourceFolderContext=null;sheets=[];wideSheetRows=[];
   clearSourceOverrides();$('sheetSelect').replaceChildren();$('dailySheetChoices').replaceChildren();status('excelFileStatus','Выберите файл');importLock();
  }
  ++generation;++reportTicket;++calendarTicket;++qualityTicket;++detailTicket;savedQualityPeriods.clear();syncQualityPeriods();if(!snapshot){window.sqlRowScroll?.reset($('reports'));window.sqlRowScroll?.reset($('qualityTable'));qualityActions.replaceChildren();reportPageStatus.textContent='Данные обновляются';resetReview('Обновление данных. Проверьте Excel заново.');$('reports').replaceChildren();$('reportCalendar').replaceChildren();$('calendarDetails').replaceChildren();$('qualityTable').replaceChildren();$('qualityPrev').disabled=$('qualityNext').disabled=true;return;}reportIndex=0;qualityOffset=0;resetReview('Данные обновлены. Перед импортом выполните новую проверку.');
  const selected=$('qualityOrg').value;$('qualityOrg').replaceChildren();for(const [value,label]of [['all','Все подразделения'],...(snapshot.org_names||[]).map(v=>[v,v])]){const o=el('option',label);o.value=value;$('qualityOrg').append(o);}$('qualityOrg').value=(snapshot.org_names||[]).includes(selected)?selected:'all';
  void refreshUpload();
 });
 async function refreshUpload(){if(uploadMode==='calendar')return loadCalendar();if(uploadMode==='list')return loadReports();if(uploadMode==='quality')return loadQuality();}
 UI.onView('upload',()=>openUploadView('import'));
 importLock();formatChanged();folderControls();void openUploadView('import');
 window.unifiedReports.refresh=refreshUpload;window.unifiedReports.openView=openUploadView;
}
window.unifiedReports={init};
})();
