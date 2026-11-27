/* One filter scope for the two reference screens and the native SQL detail tabs. */
(function(){
'use strict';
const C=window.referenceCalendar,UI=window.unifiedUI;
const kinds=['day','week','month','year'];
const baselines=[['prev','Предыдущий равный период'],['prevWeek','Прошлая неделя'],['prevMonth','Прошлый месяц'],['year','Тот же период год назад'],['custom','Свой период…']];
const plural=new Intl.PluralRules('ru');
const panel=document.getElementById('reportShell');
const one=selector=>panel.querySelector(selector),many=selector=>[...panel.querySelectorAll(selector)];
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=text;return el;};
const button=(text,fn)=>{const el=make('button',text);el.type='button';el.onclick=fn;return el;};
const size=(lo,hi)=>1+Math.round((Date.parse(hi)-Date.parse(lo))/86400000);
const interval=(lo,hi)=>lo===hi?C.stamp(lo):(lo.slice(0,4)===hi.slice(0,4)?C.stamp(lo).slice(0,5):C.stamp(lo))+' — '+C.stamp(hi);
const duration=(lo,hi)=>{const n=size(lo,hi);return n+' '+({one:'день',few:'дня',many:'дней',other:'дня'}[plural.select(n)])+' · дата «По» включительно';};
const today=()=>{const d=new Date();return C.dateAt(d.getFullYear(),d.getMonth(),d.getDate());};
let initialized=false,scope=null,revision=null,snapshot=null,latestDay=null,lastTicket=0,latestType=null,userDates=false,syncFields=null;
let workspaceKey=null;
let orgs=new Set(),orgNames=[],allOrgs=true,allNames=true,nameRows=new Map(),nameCount=0,metric='amount',workCounts=null;
let workCountTicket=0,workCountNotice='';
let kind='week',source='daily',mode='range',rule='prev',custom=null,customEnd=null,from,to;
let importedWeekday=1,boundsPending=false,boundsFailed=false,nameMetadata=null;
const detailPanels=['chartPanel','structurePanel','comparison','usualPanel','regularityPanel','overviewPanel','changes'];
function previousYear(value){
 const d=C.dateOf(value),y=d.getUTCFullYear()-1,m=d.getUTCMonth();
 return C.dateAt(y,m,Math.min(d.getUTCDate(),C.dateOf(C.dateAt(y,m+1,0)).getUTCDate()));
}
function filterPeriod(value,unit=kind){return C.period(value,unit,source==='imported'?importedWeekday:1);}
function importedBound(value){return filterPeriod(value)[0];}
function baseRange(){
 const n=size(from,to);
 if(rule==='custom'&&custom&&customEnd)return [custom,customEnd];
 if(rule==='year'){
  const end=previousYear(to);
  return [previousYear(from),kind==='month'?C.period(end,'month')[1]:end];
 }
 if(rule==='prevWeek'||rule==='prevMonth'){
  const unit=rule==='prevWeek'?'week':'month';
  return filterPeriod(C.day(filterPeriod(from,unit)[0],-1),unit);
 }
 if(source==='imported'&&kind==='month'){
  const idx=date=>Number(date.slice(0,4))*12+Number(date.slice(5,7));
  const span=idx(to)-idx(from)+1,d=C.dateOf(rule==='custom'?custom:from);
  const start=C.dateAt(d.getUTCFullYear(),d.getUTCMonth()-(rule==='custom'?0:span));
  const f=C.dateOf(start);
  return [start,C.dateAt(f.getUTCFullYear(),f.getUTCMonth()+span,0)];
 }
 const lo=rule==='custom'?custom:C.day(from,-n);
 return [lo,C.day(lo,n-1)];
}
function allowRule(value){
 if(source==='daily'||kind==='day')return true;
 if(value==='prevWeek')return kind==='week';
 if(value==='prevMonth')return kind==='month';
 if(value==='year')return kind!=='week';
 return true;
}
function dates(start,end=start){
 const lo=filterPeriod(start)[0],hi=filterPeriod(mode==='single'?start:end)[1];
 if(!C.valid(lo)||!C.valid(hi)||lo>hi)return false;
 from=lo;to=hi;return true;
}
function quick(index){
 if(!latestDay)return null;
 const unit=index<3?'week':index===3?'month':'year';
 const start=C.day(latestDay,index===1?-7:index===2?-21:0);
 return {kind:unit,mode:index===2?'range':'single',from:C.period(start,unit)[0],to:C.period(index===1?start:latestDay,unit)[1]};
}
function applyQuick(index){
 if(!snapshot||boundsPending||boundsFailed)return;
 const p=quick(index);if(!p)return;
 kind=p.kind;mode=p.mode;source='daily';userDates=true;dates(p.from,p.to);if(custom)custom=filterPeriod(custom)[0];
 return publish();
}
function shift(direction){
 if(!snapshot||boundsPending||boundsFailed)return;
 let lo,hi;
 if(kind==='month'||kind==='year'){
  const a=C.dateOf(from),b=C.dateOf(to);
  const delta=direction*((b.getUTCFullYear()-a.getUTCFullYear())*12+b.getUTCMonth()-a.getUTCMonth()+1);
  lo=C.dateAt(a.getUTCFullYear(),a.getUTCMonth()+delta);hi=C.dateAt(b.getUTCFullYear(),b.getUTCMonth()+delta);
 }else{const delta=direction*size(from,to);lo=C.day(from,delta);hi=C.day(to,delta);}
 if(C.valid(lo)&&C.valid(hi)&&dates(lo,hi)){userDates=true;publish();}
}
function chooseDate(target,anchor){
 if(!snapshot||boundsPending||boundsFailed)return;
 const opened=snapshot,openedKind=kind,openedSource=source,weekly=source==='imported'&&kind==='week';
 const weeksForDay=weekly?C.savedWeeksForDay(UI.api,opened.revision,()=>snapshot===opened&&source===openedSource&&kind===openedKind):undefined;
 const current=target==='base'?baseRange()[0]:target==='baseEnd'?baseRange()[1]:target==='end'?to:from;
 C.open({anchor,title:target==='base'?'Начало базового периода':target==='baseEnd'?'Конец базового периода':mode==='single'?'Период':target==='start'?'Начало диапазона':'Конец диапазона',unit:kind,initial:current,spanFor:(target==='base'||target==='baseEnd')&&weekly?(v,k)=>C.period(v,k,C.dateOf(baseRange()[0]).getUTCDay()):filterPeriod,weeksForDay,apply(value,unit,weekStart){
  if(snapshot!==opened||kind!==openedKind||source!==openedSource||boundsPending||boundsFailed){UI.notice('Периоды изменились. Откройте календарь повторно.',true);return;}
  if(weekly){
   if(target==='base'){rule='custom';custom=weekStart||C.period(value,'week',C.dateOf(current).getUTCDay())[0];customEnd=null;publish();return;}
   if(target==='baseEnd'){
    const first=baseRange()[0],lastWeek=weekStart||C.period(value,'week',C.dateOf(first).getUTCDay())[0];
    if(lastWeek<first||C.dateOf(lastWeek).getUTCDay()!==C.dateOf(first).getUTCDay()){
     UI.notice('Конечная неделя должна начинаться в тот же день недели и не раньше начальной.',true);return;
    }
    rule='custom';custom=first;customEnd=C.day(lastWeek,6);publish();return;
   }
   if(weekStart){
    const weekday=C.dateOf(weekStart).getUTCDay();
    if(mode==='range'&&weekday!==importedWeekday&&(target==='end'?weekStart>from:C.day(weekStart,6)<to))
     UI.notice('Открыта неделя с другим днём начала. Для диапазона выберите вторую неделю с тем же днём начала.');
    if(weekday!==importedWeekday){from=weekStart;to=C.day(weekStart,6);}
    importedWeekday=weekday;value=weekStart;
   }
  }
  if(target==='base'){rule='custom';custom=filterPeriod(value)[0];customEnd=null;}
  else if(target==='baseEnd'){
   const start=baseRange()[0],end=filterPeriod(value)[1];
   if(end<start){UI.notice('Конец базы не может быть раньше начала.',true);return;}
   rule='custom';custom=start;customEnd=end;
  }else {userDates=true;if(target==='end')dates(value<from?value:from,value);else dates(value,value>to?value:to);}
  publish();
 }});
}
function paint(lo,hi){
 const range=mode==='range',caption=interval(from,to);
 one('[data-period-selection]').value=kind+':'+mode;
 one('.report-dates').classList.toggle('single-date',!range);
 for(const key of ['[data-start-label]','[data-base-start-label]'])one(key).textContent=range?'С':'Период';
 for(const key of ['[data-end-field]','[data-base-end-field]','[data-base-duration]','.period-duration'])one(key).hidden=!range;
 const current=one('[data-date=start]'),end=one('[data-date=end]');
 current.textContent=range?C.stamp(from):caption;
 end.textContent=C.stamp(to);current.setAttribute('aria-label',(range?'Начало диапазона':'Выбрать период')+', '+current.textContent);end.setAttribute('aria-label','Конец диапазона, '+end.textContent);
 one('.period-duration').textContent=range?duration(from,to):'';
 const baseStart=one('[data-base-start]'),baseEnd=one('[data-base-end]');
 baseStart.textContent=range?C.stamp(lo):interval(lo,hi);baseEnd.textContent=C.stamp(hi);
 baseStart.setAttribute('aria-label','Начало базового периода: '+C.stamp(lo)+'. Выбрать другую дату');
 baseEnd.setAttribute('aria-label','Конец базового периода: '+C.stamp(hi)+'. Выбрать другую дату');
 one('[data-base-duration]').textContent=range?duration(lo,hi):'';
 for(const action of many('[data-baseline-quick]')){
  action.setAttribute('aria-pressed',String(rule===action.dataset.baselineQuick));
  action.disabled=boundsPending||boundsFailed||!snapshot||!allowRule(action.dataset.baselineQuick);
 }
 one('[data-compare]').value=rule;
 for(const opt of one('[data-compare]').options)opt.disabled=!allowRule(opt.value);
 one('[data-measure]').value=metric;one('#referenceSource').value=source;one('#referenceSource option[value=imported]').disabled=kind==='year';
 one('[data-pick=orgs]').textContent=orgs.size===1?[...orgs][0]:allOrgs&&orgs.size?'Все ('+orgs.size+')':'Выбрано '+orgs.size+' из '+orgNames.length;
 const workFilter=one('[data-pick=works]');
 workFilter.textContent=workCountNotice||(workCounts
  ? window.analysisSummary.workSelectionText({selected:allNames?workCounts.total:workCounts.selected,total:workCounts.total})
  : allNames?'Все работы и группы':'Выбрано '+nameRows.size+' из '+nameCount);
 one('.filter-summary').textContent=caption+', база '+interval(lo,hi)+(allOrgs?'':', подразделений '+orgs.size+' из '+orgNames.length);
 const note=one('.period-length-note');note.hidden=size(lo,hi)===size(from,to);note.textContent=note.hidden?'':'Интервалы разной длительности. Сравниваются итоги за каждый интервал.';
 many('.quick-actions button').forEach((b,i)=>{const p=quick(i);b.disabled=!p||!snapshot||boundsPending||boundsFailed;b.setAttribute('aria-pressed',String(Boolean(p&&source==='daily'&&p.kind===kind&&p.from===from&&p.to===to)));if(p)b.title=interval(p.from,p.to);else b.removeAttribute('title');});
}
let appliedDetailScope;
function syncDetails(){
 if(boundsPending)return;
 const current=scope?.revision===UI.revision?scope:null;
 // Revisiting a tool keeps its local choices until a shared filter actually changes.
 if(current===appliedDetailScope)return;
 appliedDetailScope=current;
 return window.unifiedAnalytics.applyReferenceScope(current);
}
function publish(appliedInOverview=false){
 if(boundsPending)return;
 if(boundsFailed&&snapshot)return readBounds(snapshot);
 const [baseFrom,baseTo]=baseRange();
 if(![from,to,baseFrom,baseTo].every(C.valid)||baseFrom>baseTo){
  UI.notice('Период за пределами поддерживаемого календаря.',true);scope=null;window.referenceResults.refresh(null);return;
 }
 paint(baseFrom,baseTo);
 scope=revision===null?null:{revision,type:source==='daily'?'day':kind,displayType:kind,source,overviewMode:mode,
  from,to:source==='daily'?to:importedBound(to),baseFrom:source==='daily'||kind==='week'?baseFrom:importedBound(baseFrom),baseTo:source==='daily'?baseTo:kind==='week'?C.day(baseTo,-6):importedBound(baseTo),
  displayFrom:from,displayTo:to,displayBaseFrom:baseFrom,displayBaseTo:baseTo,organizations:[...orgs],metric,
  allNames,nameRows:[...nameRows.values()],selectedNames:allNames?[]:[...nameRows.values()].map(row=>row.name),selectedKeys:allNames?[]:[...nameRows.keys()]};
 const result=window.referenceResults.refresh(scope);
 // The overview can publish its own work selection without replaying every shared control.
 if(appliedInOverview===true&&window.referenceDetail?.active==='overviewPanel')appliedDetailScope=scope;
 else if(detailPanels.includes(window.referenceDetail?.active))void syncDetails();
 return result;
}
function setMetric(value){if(value!=='amount'&&value!=='count')return;metric=value;return publish();}
function openImportedPeriod(range){
 if(!range||!['day','week','month'].includes(range.kind)||!C.valid(range.from)||!C.valid(range.to)||range.from>range.to)return false;
 kind=range.kind;source=kind==='day'?'daily':'imported';mode='range';rule='prev';custom=null;userDates=true;
 from=range.from;to=range.to;
 if(kind==='week'){
  const lastStart=C.day(to,-6);
  // Mixed weekday schedules cannot be described by one weekly SQL sequence.
  if(range.mixedWeekStarts||C.dateOf(from).getUTCDay()!==C.dateOf(lastStart).getUTCDay()){
   from=lastStart;
   UI.notice('Загружены недели с разными днями начала. Открыта последняя загруженная неделя.');
  }
  importedWeekday=C.dateOf(from).getUTCDay();
 }
 return boundsPending&&snapshot?readBounds(snapshot):publish();
}
function refreshWorkCount(){
 const mine=++workCountTicket,opened=snapshot;
 workCountNotice='';
 if(!workCounts||allNames||!nameRows.size){
  if(workCounts)workCounts={...workCounts,selected:allNames?workCounts.total:0};
  return;
 }
 const label=one('[data-pick=works]'),keys=[...nameRows.keys()];
 label.textContent=workCountNotice='Подсчёт работ…';
 Promise.resolve().then(()=>UI.api.unifiedSelectedNameOptions(opened.revision,keys)).then(page=>{
  if(mine!==workCountTicket||opened!==snapshot)return;
  if(!page.work_counts)throw Error('Не удалось подсчитать работы');
  workCounts=page.work_counts;
  workCountNotice='';
  label.textContent=window.analysisSummary.workSelectionText(workCounts);
 }).catch(()=>{if(mine===workCountTicket&&opened===snapshot)label.textContent=workCountNotice='Не удалось подсчитать работы';});
}
function setWorks(everything,selection,appliedInOverview=false){
 allNames=everything;nameRows=new Map(selection);refreshWorkCount();
 return publish(appliedInOverview);
}
function setOrganizations(values){orgs=new Set(values.filter(x=>orgNames.includes(x)));allOrgs=orgs.size===orgNames.length;return publish();}
function close(box){box.closest('dialog')?.close();}
function listShell(title,anchor,described){
 const content=window.referenceDesktop.dialog(title,anchor);
 const box=make('div');box.className='filter-list-dialog';content.append(box);
 if(described)box.append(make('p',described));
 const query=make('input');query.type='search';query.setAttribute('aria-label','Найти в списке');
 const actions=make('div'),list=make('div'),count=make('output'),pages=make('div');
 actions.className='filter-list-actions';list.className='filter-checkboxes';pages.className='filter-list-actions';
 box.append(query,actions,list,pages);return {box,query,actions,list,count,pages};
}
function checkbox(list,text,checked,change,textClass){
 const label=make('label'),input=make('input');input.type='checkbox';input.checked=checked;
 const caption=make('span',text);if(textClass)caption.className=textClass;
 input.onchange=()=>change(input);label.append(input,caption);list.append(label);return input;
}
function chooseOrgs(anchor){
 const {box,query,actions,list,count}=listShell('Подразделения',anchor);
 const opened=snapshot,selected=new Set(orgs);
 query.placeholder='Найти подразделение';query.hidden=orgNames.length<=8;
 const visible=()=>orgNames.filter(o=>o.toLocaleLowerCase('ru').includes(query.value.trim().toLocaleLowerCase('ru')));
 const all=button('Выбрать все',()=>{visible().forEach(x=>selected.add(x));draw();});
 const none=button('Снять выбор',()=>{visible().forEach(x=>selected.delete(x));draw();});
 const update=()=>count.textContent='Выбрано '+selected.size+' из '+orgNames.length;
 actions.append(all,none,count);
 function draw(){
  const items=visible();list.replaceChildren();for(const name of items)checkbox(list,name,selected.has(name),input=>{if(input.checked)selected.add(name);else selected.delete(name);update();});
  if(!items.length)list.append(make('p',orgNames.length?'Совпадений нет.':'Список пуст.'));
  all.textContent=query.value.trim()?'Выбрать найденные':'Выбрать все';all.disabled=none.disabled=!items.length;update();
 }
 query.oninput=draw;const apply=button('Применить',()=>{if(!opened||opened!==snapshot){UI.notice('Данные изменились. Откройте выбор повторно.',true);close(box);return;}close(box);setOrganizations([...selected]);});window.referenceDesktop.setDialogApply(apply);draw();if(!query.hidden)query.focus();
}
function chooseWorks(anchor){
 const {box,query,actions,list,count,pages}=listShell('Работы в подробной аналитике',anchor,'Выбор применяется к подробной аналитике. В сводке учитываются все работы.');
 query.placeholder='Найти работу';query.setAttribute('aria-label',query.placeholder);list.dataset.sqlScrollHost='reference-work-options';
 const opened=snapshot,selected=new Map(nameRows);
 let everything=allNames,rows=[],index=0,ticket=0,matchingTotal=0,hasUnselectable=false;
 const limit=50;
 let countTicket=0;
 const update=()=>{
  const mine=++countTicket;
  if(!workCounts){count.textContent=everything?'Выбраны все работы и группы':'Выбрано '+selected.size+' из '+nameCount;return;}
  const format=window.analysisSummary.workSelectionText;
  if(everything||!selected.size){count.textContent=format({selected:everything?workCounts.total:0,total:workCounts.total});return;}
  count.textContent='Подсчёт работ…';
  UI.api.unifiedSelectedNameOptions(opened.revision,[...selected.keys()]).then(page=>{
   if(mine===countTicket&&box.isConnected&&opened===snapshot)count.textContent=format(page.work_counts);
  }).catch(()=>{if(mine===countTicket&&box.isConnected&&opened===snapshot)count.textContent='Не удалось подсчитать работы';});
 };
 const all=button('Выбрать все',()=>{
  if(!query.value.trim()){everything=true;selected.clear();}
  else if(!everything){const added=new Map(selected);for(const row of rows)added.set(row.key,row);if(added.size>limit){UI.notice('Можно выбрать до 50 отдельных работ или все работы.',true);return;}for(const row of rows)selected.set(row.key,row);}
  draw();
 });
 const clear=button('Снять выбор',()=>{if(query.value.trim()&&!everything){for(const row of rows)selected.delete(row.key);}else{everything=false;selected.clear();}draw();});
 actions.append(all,clear,count);
 const apply=button('Применить',()=>{if(!opened||opened!==snapshot){UI.notice('Данные изменились. Откройте выбор повторно.',true);close(box);return;}close(box);setWorks(everything,selected);});window.referenceDesktop.setDialogApply(apply);
 function draw(){
  list.replaceChildren();update();
  const search=Boolean(query.value.trim());all.textContent=search?(matchingTotal>limit?'Выбрать показанные':'Выбрать найденные'):'Выбрать все';
  clear.textContent=search&&!everything?'Снять выбор с показанных':'Снять выбор';all.disabled=search&&!rows.length;
  if(hasUnselectable)list.append(make('p',everything
   ?'В общий итог входят также работы и группы с совпадающими или слишком длинными названиями. Для индивидуального выбора нажмите «Снять выбор».'
   :'Для индивидуального выбора доступны только работы и группы с уникальными названиями допустимой длины.'));
  const explicitOnly=search||nameCount>limit||hasUnselectable||Boolean(workCounts);
  const checkboxes=[];
  const updateCheckboxes=()=>{
   for(const {row,input} of checkboxes){
    const inherited=row.ancestor_keys?.some(key=>selected.has(key));
    input.checked=everything||inherited||selected.has(row.key);
    input.disabled=Boolean(inherited||(everything&&explicitOnly));
    if(inherited)input.setAttribute('aria-description','Входит в выбранную группу');
    else input.removeAttribute('aria-description');
   }
  };
  for(const row of rows){
   const group=row.kind==='group';
   const input=checkbox(list,row.name+(group?' (группа)':''),everything||selected.has(row.key),field=>{
    if(everything){
     if(explicitOnly){field.checked=true;UI.notice('Для индивидуального выбора нажмите «Снять выбор».',true);return;}
     everything=false;for(const item of rows)selected.set(item.key,item);
    }
    if(field.checked&&selected.size===limit&&!selected.has(row.key)){field.checked=false;UI.notice('Можно выбрать до 50 отдельных работ или все работы.',true);return;}
    if(field.checked)selected.set(row.key,row);else selected.delete(row.key);updateCheckboxes();update();
   },group?'work-group-option':null);
   checkboxes.push({row,input});
  }
  updateCheckboxes();
  if(!rows.length)list.append(make('p',search?'Совпадений нет.':'Нет позиций для индивидуального выбора.'));
  window.sqlRowScroll.attach(list,{index,size:limit,count:rows.length,total:matchingTotal,footer:pages,label:'Страницы списка работ',go:i=>{
   if(!box.isConnected||opened!==snapshot)return false;
   return load(i);
  }});
 }
 async function load(requested){
  const mine=++ticket;all.disabled=clear.disabled=true;
  try{
   const page=!opened?{rows:[],total_rows:0}:await UI.api.unifiedNameOptionsAtPage(opened.revision,query.value.trim(),requested,limit);
   if(mine!==ticket||!box.isConnected||opened!==snapshot)return;
   rows=page.rows;matchingTotal=page.total_rows;hasUnselectable=page.has_omitted_names===true;index=requested;clear.disabled=false;draw();
  }catch(error){if(mine===ticket&&box.isConnected&&opened===snapshot){window.sqlRowScroll.reset(list);list.replaceChildren(make('p','Не удалось открыть список. '+(window.analizUserError?.(error)||error.message)));}}
 }
 query.oninput=()=>{window.sqlRowScroll.reset(list);list.replaceChildren(make('p','Загрузка…'));return load(0);};void load(0);query.focus();
}
async function readBounds(snap){
 const mine=++lastTicket;
 setBoundsPending(true);
 window.referenceResults.beginScopeChange(snap.revision);
 const reportType=source==='daily'?'day':kind;
 try{
  const daily=UI.api.unifiedPeriodOptions(snap.revision,'day',null,null,1);
  const selectedKeys=allNames?[]:[...nameRows.keys()];
  const selection=JSON.stringify(selectedKeys);
  if(nameMetadata?.selection!==selection){
   const request=selectedKeys.length?UI.api.unifiedSelectedNameOptions(snap.revision,selectedKeys):UI.api.unifiedNameOptions(snap.revision,'',null,1);
   const cached={selection,request};nameMetadata=cached;
   Promise.resolve(request).catch(()=>{if(nameMetadata===cached)nameMetadata=null;});
  }
  const metadata=nameMetadata.request;
  const [last,selected,names]=await Promise.all([daily,reportType==='day'?daily:UI.api.unifiedPeriodOptions(snap.revision,reportType,null,null,1),metadata]);
  if(mine!==lastTicket||UI.revision!==snap.revision)return;
  latestDay=last.max;latestType=selected.max;nameCount=names.total_rows;
  workCounts=names.work_counts||null;workCountNotice='';
  if(selectedKeys.length){
   const available=new Map(names.rows.map(row=>[row.key,row]));
   // Preserve changes the user made while this metadata read was in flight.
   for(const key of selectedKeys)if(nameRows.has(key)){
    if(available.has(key))nameRows.set(key,available.get(key));else nameRows.delete(key);
   }
  }
  if(selection!==JSON.stringify(allNames?[]:[...nameRows.keys()]))refreshWorkCount();
  if(!userDates){
   if(source==='imported'&&kind==='week')importedWeekday=latestType?C.dateOf(latestType).getUTCDay():1;
   dates(latestType||today());
  }
  boundsFailed=false;setBoundsPending(false);return publish();
 }catch(error){
  if(mine!==lastTicket||UI.revision!==snap.revision)return;
  boundsFailed=true;setBoundsPending(false);scope=null;
  if(detailPanels.includes(window.referenceDetail?.active))void syncDetails();
  const message='Не удалось прочитать периоды. '+(window.analizUserError?.(error)||error.message);
  window.referenceResults.failScopeChange(message);UI.notice(message,true);
 }
}
function setBoundsPending(busy){
 boundsPending=busy;
 const blocked=busy||boundsFailed||!snapshot;
 for(const b of [...many('[data-date]'),...many('[data-shift]'),one('[data-period-selection]'),...many('[data-baseline-quick]'),one('[data-base-start]'),one('[data-base-end]'),one('[data-compare]')])b.disabled=blocked;
 if(blocked)for(const b of many('.quick-actions button'))b.disabled=true;
 const retry=one('[data-retry-periods]');retry.hidden=!boundsFailed;retry.disabled=busy;
 for(const id of [...detailPanels,'analyticsDetails','changeDetails']){
  const node=document.getElementById(id);if(node)node.inert=busy;
 }
}
function configure(snap){
 ++lastTicket;++workCountTicket;nameMetadata=null;workCounts=null;workCountNotice='';boundsFailed=false;
 if(!snap){snapshot=null;revision=null;scope=null;setBoundsPending(false);window.referenceResults.refresh(null);return;}
 // A new shared generation can keep the same catalog. A different database can
 // reuse a group key for unrelated work even when the numeric revisions match.
 const workspace=window.sharedWorkspace?.state;
 const key=workspace?.mode==='shared'?workspace.folder:null;
 if(workspaceKey!==key){allNames=true;nameRows.clear();allOrgs=true;}
 workspaceKey=key;
 snapshot=snap;revision=snap.revision;
 orgNames=snap.org_names.slice();orgs=new Set(allOrgs?orgNames:[...orgs].filter(x=>orgNames.includes(x)));
 return readBounds(snap);
}
function reset(){
 window.referenceResults.resetSelection?.();
 source='daily';kind='week';mode='range';rule='prev';metric='amount';custom=null;customEnd=null;userDates=false;
 allNames=true;nameRows.clear();refreshWorkCount();orgs=new Set(orgNames);allOrgs=true;
 dates(latestDay||today());return boundsPending&&snapshot?readBounds(snapshot):publish();
}
function setPeriodSelection(value){
 const [unit,selectionMode]=value.split(':');
 if(!kinds.includes(unit)||!['single','range'].includes(selectionMode)||boundsPending||boundsFailed)return;
 const unitChanged=kind!==unit;
 if(!unitChanged&&mode===selectionMode)return;
 kind=unit;mode=selectionMode;
 if(unitChanged){
  if(kind==='year')source='daily';
  if(!allowRule(rule))rule='prev';
  if(custom)custom=filterPeriod(custom)[0];
  customEnd=null;
 }
 dates(from,to);
 if(unitChanged&&snapshot&&source==='imported')return readBounds(snapshot);
 return publish();
}
function sourceChanged(){
 source=one('#referenceSource').value;
 if(!allowRule(rule))rule='prev';
 customEnd=null;
 userDates=false;latestType=null;
 if(snapshot)void readBounds(snapshot);else publish();
}
// A single disclosure owns the controls in both layouts. When it is pinned
// above a scrolled report, keep its existing space in the document while it
// expands or collapses; the same report row remains under the pointer.
function atSide(){
 return getComputedStyle(panel).getPropertyValue('--filter-at-side').trim()==='1';
}
function keepFilterSpace(){
 if(atSide() || panel.hasAttribute('data-floating') || window.scrollY===0 || panel.getBoundingClientRect().top>0.5)return;
 panel.style.setProperty('--filter-slot', panel.offsetHeight+'px');
 panel.setAttribute('data-floating','');
}
function openFilters(){
 const fold=one('.filter-fold');
 keepFilterSpace();
 panel.removeAttribute('data-auto-folded');
 fold.open=true;
 fold.querySelector('summary').focus({preventScroll:true});
}
function closeFilters(){
 const fold=one('.filter-fold');
 if(fold.open)keepFilterSpace();
 panel.removeAttribute('data-auto-folded');
 fold.open=false;
}
function initPlacement(){
 const root=document.getElementById('refRoot'),fold=one('.filter-fold'),summary=one('.filter-fold > summary');
 const header=document.getElementById('sharedHeader'),brand=header.querySelector('.brand');
 const placement=document.getElementById('appearanceFilterPlacement');
 const layoutKey='analiz.desktop.filterPlacement';
 function fitSidebar(){
  const gap=Math.ceil(Math.max(0,panel.getBoundingClientRect().top));
  panel.style.setProperty('--filter-top-gap',gap+'px');
 }
 function releaseSpace(){
  if(panel.hasAttribute('data-auto-folded'))fold.open=true;
  panel.removeAttribute('data-auto-folded');
  panel.removeAttribute('data-floating');
  panel.style.removeProperty('--filter-slot');
 }
 function syncSidebar(){
  const style=getComputedStyle(panel),zoom=panel.currentCSSZoom||Number(style.zoom)||1;
  const maxWidth=parseFloat(style.getPropertyValue('--filter-sidebar-max-width'));
  const minWidth=parseFloat(style.getPropertyValue('--filter-sidebar-min-width'));
  const gap=parseFloat(style.getPropertyValue('--filter-sidebar-gap'));
  const available=brand.getBoundingClientRect().left-root.getBoundingClientRect().left;
  const width=Math.min(maxWidth,Math.floor(available/zoom-2*gap));
  const fits=root.dataset.filterPlacement==='side'&&width>=minWidth;
  if(fits)panel.style.setProperty('--filter-sidebar-width',width+'px');
  else panel.style.removeProperty('--filter-sidebar-width');
  if(fits===Boolean(root.dataset.filterSidebar))return;
  if(fits)root.dataset.filterSidebar='true';else delete root.dataset.filterSidebar;
  releaseSpace();
  if(fits)fold.open=true;
 }
 function display(value){
  const layout=value==='side'?'side':'top';
  placement.value=layout;
  root.dataset.filterPlacement=layout;
  releaseSpace();
  syncSidebar();
 }
 let initial='top';
 try{initial=window.localStorage.getItem(layoutKey);}catch{/* Storage may be unavailable. */}
 fold.open=!matchMedia('(max-width:680px)').matches;
 display(initial);
 placement.onchange=()=>{
  display(placement.value);
  try{window.localStorage.setItem(layoutKey,placement.value);}
  catch{UI.notice('Расположение фильтра изменено на этот сеанс. Сохранить настройку на устройстве не удалось.');}
 };
 summary.addEventListener('click',event=>{
  // One click also opens a partly scrolled-away filter.
  const reveal=fold.open && !atSide() && !panel.hasAttribute('data-floating') && panel.getBoundingClientRect().top < -1;
  keepFilterSpace();
  panel.removeAttribute('data-auto-folded');
  if(reveal)event.preventDefault();
 });
 // When the initial inline controls leave the viewport, only the summary
 // remains. Leave a focused input or an explicitly opened pinned panel alone.
 function updatePinnedState(){
  if(atSide()){fitSidebar();return;}
  if(!fold.open || panel.hasAttribute('data-floating') || (document.activeElement!==summary && panel.contains(document.activeElement)))return;
  const bounds=panel.getBoundingClientRect();
  if(bounds.top>=0 || bounds.bottom>summary.getBoundingClientRect().height+4)return;
  keepFilterSpace();
  panel.setAttribute('data-auto-folded','');
  fold.open=false;
 }
 window.addEventListener('scroll',updatePinnedState,{passive:true});
 const headerObserver=new IntersectionObserver(([entry])=>{if(entry.isIntersecting)releaseSpace();});
 headerObserver.observe(header);
 const sizeObserver=new ResizeObserver(()=>{
  syncSidebar();
  // viewport units and DOMRect use window coordinates; panel height is in its own CSS pixels.
  panel.style.setProperty('--filter-zoom',panel.currentCSSZoom || Number(getComputedStyle(panel).zoom) || 1);
  const summaryHeight=Math.ceil(summary.getBoundingClientRect().height)+'px';
  panel.style.setProperty('--filter-heading-height',summaryHeight);
  panel.style.setProperty('--filter-scroll-away',Math.max(0,panel.offsetHeight-summary.offsetHeight-2)+'px');
  root.style.setProperty('--sticky-filter-height',atSide()?'0px':summaryHeight);
  updatePinnedState();
 });
 sizeObserver.observe(summary);sizeObserver.observe(panel);sizeObserver.observe(root);sizeObserver.observe(header);
}
function init(){
 if(initialized)return;
 initialized=true;dates(today());
 const sourceLabel=make('label','Источник');sourceLabel.className='lbl';const input=make('select');input.id='referenceSource';input.className='sel';
 for(const [value,text] of [['daily','Дневные отчёты'],['imported','Отчёты выбранного типа']]){const option=make('option',text);option.value=value;input.append(option);}
 const compare=one('[data-compare]');compare.replaceChildren();
 sourceLabel.append(input);compare.parentElement.before(sourceLabel);input.onchange=sourceChanged;
 for(const [value,text] of baselines){const option=make('option',text);option.value=value;compare.append(option);}
 compare.onchange=()=>{if(compare.value==='custom'){compare.value=rule;chooseDate('base',compare);}else{rule=compare.value;customEnd=null;publish();}};
 for(const button of many('[data-baseline-quick]'))button.onclick=()=>{if(!allowRule(button.dataset.baselineQuick))return;rule=button.dataset.baselineQuick;customEnd=null;publish();};
 initPlacement();
 const period=one('[data-period-selection]');period.onchange=()=>setPeriodSelection(period.value);
 many('[data-date]').forEach(b=>b.onclick=()=>chooseDate(b.dataset.date,b));
 many('[data-shift]').forEach(b=>b.onclick=()=>shift(Number(b.dataset.shift)));
 const start=one('[data-base-start]'),end=one('[data-base-end]');
 start.onclick=()=>chooseDate('base',start);end.onclick=()=>chooseDate('baseEnd',end);
 const measure=one('[data-measure]');measure.onchange=()=>setMetric(measure.value);
 const org=one('[data-pick=orgs]'),work=one('[data-pick=works]');
 org.onclick=()=>chooseOrgs(org);work.onclick=()=>chooseWorks(work);
 many('.quick-actions button').forEach((b,i)=>b.onclick=()=>applyQuick(i));one('.filter-reset').onclick=reset;
 const native=document.getElementById('refNative');if(native)syncFields=C.enhance(native);
 one('[data-retry-periods]').onclick=()=>snapshot&&readBounds(snapshot);
 UI.onSnapshot(configure);paint(...baseRange());setBoundsPending(false);
 if(UI.snapshot)void configure(UI.snapshot);
}
function refresh(){syncFields?.();return publish();}
window.referenceFilters=Object.freeze({init,refresh,openFilters,closeFilters,atSide,setMetric,setOrganizations,setWorks,openImportedPeriod,syncDetails,get scope(){return scope;}});
})();
