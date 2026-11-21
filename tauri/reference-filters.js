/* One filter scope for the two reference screens and the native SQL detail tabs. */
(function(){
'use strict';
const C=window.referenceCalendar,UI=window.unifiedUI;
const kinds=['day','week','month','year'];
const baselines=[['prev','Предыдущий равный период'],['prevWeek','Прошлая неделя'],['prevMonth','Прошлый месяц'],['year','Тот же период год назад'],['custom','Свой период…']];
const calendarMonth=new Intl.DateTimeFormat('ru-RU',{month:'long',year:'numeric',timeZone:'UTC'});
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
let orgs=new Set(),orgNames=[],allOrgs=true,allNames=true,nameRows=new Map(),nameCount=0,metric='amount';
let kind='week',source='daily',mode='range',rule='prev',custom=null,from,to;
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
 const current=target==='base'?baseRange()[0]:target==='end'?to:from;
 C.open({anchor,title:target==='base'?'Базовый период':mode==='single'?'Период':target==='start'?'Начало диапазона':'Конец диапазона',unit:kind,initial:current,spanFor:target==='base'&&weekly?(v,k)=>C.period(v,k,C.dateOf(current).getUTCDay()):filterPeriod,weeksForDay,apply(value,unit,weekStart){
  if(snapshot!==opened||kind!==openedKind||source!==openedSource||boundsPending||boundsFailed){UI.notice('Периоды изменились. Откройте календарь повторно.',true);return;}
  if(weekly){
   if(target==='base'){rule='custom';custom=weekStart||C.period(value,'week',C.dateOf(current).getUTCDay())[0];publish();return;}
   if(weekStart){
    const weekday=C.dateOf(weekStart).getUTCDay();
    if(mode==='range'&&weekday!==importedWeekday&&(target==='end'?weekStart>from:C.day(weekStart,6)<to))
     UI.notice('Открыта неделя с другим днём начала. Для диапазона выберите вторую неделю с тем же днём начала.');
    if(weekday!==importedWeekday){from=weekStart;to=C.day(weekStart,6);}
    importedWeekday=weekday;value=weekStart;
   }
  }
  if(target==='base'){rule='custom';custom=filterPeriod(value)[0];}
  else {userDates=true;if(target==='end')dates(value<from?value:from,value);else dates(value,value>to?value:to);}
  publish();
 }});
}
function fieldGroups(selector,active,options){
 many(selector+' button').forEach((b,i)=>{const on=active===options[i];b.classList.toggle('on',on);b.setAttribute('aria-pressed',String(on));});
}
function paint(lo,hi){
 const range=mode==='range',caption=interval(from,to);
 fieldGroups('[data-filter="type"]',kind,kinds);fieldGroups('[data-filter="mode"]',mode,['single','range']);
 one('.report-dates').classList.toggle('single-date',!range);
 for(const key of ['[data-start-label]','[data-base-start-label]'])one(key).textContent=range?'С':'Период';
 for(const key of ['[data-end-field]','[data-base-end-field]','[data-base-duration]','.period-duration'])one(key).hidden=!range;
 const current=one('[data-date=start]'),end=one('[data-date=end]');
 current.textContent=range?C.stamp(from):kind==='year'?from.slice(0,4):kind==='month'?calendarMonth.format(C.dateOf(from)):caption;
 end.textContent=C.stamp(to);current.setAttribute('aria-label',(range?'Начало диапазона':'Выбрать период')+', '+current.textContent);end.setAttribute('aria-label','Конец диапазона, '+end.textContent);
 one('.period-duration').textContent=range?duration(from,to):'';
 one('[data-base-start]').textContent=range?C.stamp(lo):interval(lo,hi);one('[data-base-end]').textContent=C.stamp(hi);one('[data-base-duration]').textContent=range?duration(lo,hi):'';
 one('[data-compare]').value=rule;
 for(const opt of one('[data-compare]').options)opt.disabled=!allowRule(opt.value);
 one('[data-measure]').value=metric;one('#referenceSource').value=source;one('#referenceSource option[value=imported]').disabled=kind==='year';
 one('[data-pick=orgs]').textContent=orgs.size===1?[...orgs][0]:allOrgs&&orgs.size?'Все ('+orgs.size+')':'Выбрано '+orgs.size+' из '+orgNames.length;
 one('[data-pick=works]').textContent=allNames?'Все работы и группы':'Выбрано '+nameRows.size+' из '+nameCount;
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
function publish(){
 if(boundsPending)return;
 if(boundsFailed&&snapshot)return readBounds(snapshot);
 const [baseFrom,baseTo]=baseRange();
 if(![from,to,baseFrom,baseTo].every(C.valid)){
  UI.notice('Период за пределами поддерживаемого календаря.',true);scope=null;window.referenceResults.refresh(null);return;
 }
 paint(baseFrom,baseTo);
 scope=revision===null?null:{revision,type:source==='daily'?'day':kind,displayType:kind,source,overviewMode:mode,
  from,to:source==='daily'?to:importedBound(to),baseFrom:source==='daily'||kind==='week'?baseFrom:importedBound(baseFrom),baseTo:source==='daily'?baseTo:kind==='week'?C.day(baseTo,-6):importedBound(baseTo),
  displayFrom:from,displayTo:to,displayBaseFrom:baseFrom,displayBaseTo:baseTo,organizations:[...orgs],metric,
  allNames,nameRows:[...nameRows.values()],selectedNames:allNames?[]:[...nameRows.values()].map(row=>row.name),selectedKeys:allNames?[]:[...nameRows.keys()]};
 const result=window.referenceResults.refresh(scope);
 if(detailPanels.includes(window.referenceDetail?.active))void syncDetails();
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
function setWorks(everything,selection){allNames=everything;nameRows=new Map(selection);return publish();}
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
function checkbox(list,text,checked,change){
 const label=make('label'),input=make('input');input.type='checkbox';input.checked=checked;
 input.onchange=()=>change(input);label.append(input,make('span',text));list.append(label);return input;
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
 query.oninput=draw;const apply=button('Применить',()=>{if(!opened||opened!==snapshot){UI.notice('Данные изменились. Откройте выбор повторно.',true);close(box);return;}close(box);setOrganizations([...selected]);});apply.className='filter-apply';box.append(apply);draw();if(!query.hidden)query.focus();
}
function chooseWorks(anchor){
 const {box,query,actions,list,count,pages}=listShell('Работы в подробной аналитике',anchor,'Выбор применяется к подробной аналитике. В сводке учитываются все работы.');
 query.placeholder='Найти работу';list.dataset.sqlScrollHost='reference-work-options';
 const opened=snapshot,selected=new Map(nameRows);
 let everything=allNames,rows=[],index=0,ticket=0,matchingTotal=0,hasUnselectable=false;
 const limit=50;
 const update=()=>{count.textContent=everything?'Выбраны все работы и группы':'Выбрано '+selected.size+' из '+nameCount;};
 const all=button('Выбрать все',()=>{
  if(!query.value.trim()){everything=true;selected.clear();}
  else if(!everything){const added=new Map(selected);for(const row of rows)added.set(row.key,row);if(added.size>limit){UI.notice('Можно выбрать до 50 отдельных работ или все работы.',true);return;}for(const row of rows)selected.set(row.key,row);}
  draw();
 });
 const clear=button('Снять выбор',()=>{if(query.value.trim()&&!everything){for(const row of rows)selected.delete(row.key);}else{everything=false;selected.clear();}draw();});
 actions.append(all,clear,count);
 const apply=button('Применить',()=>{if(!opened||opened!==snapshot){UI.notice('Данные изменились. Откройте выбор повторно.',true);close(box);return;}close(box);setWorks(everything,selected);});apply.className='filter-apply';box.append(apply);
 function draw(){
  list.replaceChildren();update();
  const search=Boolean(query.value.trim());all.textContent=search?(matchingTotal>limit?'Выбрать показанные':'Выбрать найденные'):'Выбрать все';
  clear.textContent=search&&!everything?'Снять выбор с показанных':'Снять выбор';all.disabled=search&&!rows.length;
  if(hasUnselectable)list.append(make('p',everything
   ?'В общий итог входят также работы и группы с совпадающими или слишком длинными названиями. Для индивидуального выбора нажмите «Снять выбор».'
   :'Для индивидуального выбора доступны только работы и группы с уникальными названиями допустимой длины.'));
  const explicitOnly=search||nameCount>limit||hasUnselectable;
  for(const row of rows){
   const input=checkbox(list,row.name+(row.kind==='group'?' (группа)':''),everything||selected.has(row.key),field=>{
    if(everything){
     if(explicitOnly){field.checked=true;UI.notice('Для индивидуального выбора нажмите «Снять выбор».',true);return;}
     everything=false;for(const item of rows)selected.set(item.key,item);
    }
    if(field.checked&&selected.size===limit&&!selected.has(row.key)){field.checked=false;UI.notice('Можно выбрать до 50 отдельных работ или все работы.',true);return;}
    if(field.checked)selected.set(row.key,row);else selected.delete(row.key);update();
   });
   if(everything&&explicitOnly)input.disabled=true;
  }
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
  if(selectedKeys.length){
   const available=new Map(names.rows.map(row=>[row.key,row]));
   // Preserve changes the user made while this metadata read was in flight.
   for(const key of selectedKeys)if(nameRows.has(key)){
    if(available.has(key))nameRows.set(key,available.get(key));else nameRows.delete(key);
   }
  }
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
 for(const b of [...many('[data-date]'),...many('[data-shift]'),...many('[data-filter=mode] button'),one('.filter-base-edit'),one('[data-compare]')])b.disabled=blocked;
 if(blocked)for(const b of many('.quick-actions button'))b.disabled=true;
 const retry=one('[data-retry-periods]');retry.hidden=!boundsFailed;retry.disabled=busy;
 for(const id of [...detailPanels,'analyticsDetails','changeDetails']){
  const node=document.getElementById(id);if(node)node.inert=busy;
 }
}
function configure(snap){
 ++lastTicket;nameMetadata=null;boundsFailed=false;
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
 source='daily';kind='week';mode='range';rule='prev';metric='amount';custom=null;userDates=false;
 allNames=true;nameRows.clear();orgs=new Set(orgNames);allOrgs=true;
 dates(latestDay||today());return boundsPending&&snapshot?readBounds(snapshot):publish();
}
function setKind(value){
 if(kind===value)return;
 kind=value;
 if(kind==='year')source='daily';
 if(!allowRule(rule))rule='prev';
 if(custom)custom=filterPeriod(custom)[0];
 dates(from,to);
 if(snapshot&&(source==='imported'||boundsPending))return readBounds(snapshot);
 return publish();
}
function sourceChanged(){
 source=one('#referenceSource').value;
 if(!allowRule(rule))rule='prev';
 userDates=false;latestType=null;
 if(snapshot)void readBounds(snapshot);else publish();
}
function init(){
 if(initialized)return;
 initialized=true;dates(today());
 const sourceLabel=make('label','Источник');sourceLabel.className='lbl';const input=make('select');input.id='referenceSource';input.className='sel';
 for(const [value,text] of [['daily','Дневные отчёты'],['imported','Отчёты выбранного типа']]){const option=make('option',text);option.value=value;input.append(option);}
 sourceLabel.append(input);one('.filter-periods').append(sourceLabel);input.onchange=sourceChanged;
 const compare=one('[data-compare]');compare.replaceChildren();
 for(const [value,text] of baselines){const option=make('option',text);option.value=value;compare.append(option);}
 compare.onchange=()=>{if(compare.value==='custom'){compare.value=rule;chooseDate('base',compare);}else{rule=compare.value;publish();}};
 const fold=one('.filter-fold');fold.open=!matchMedia('(max-width:680px)').matches;
 const syncHeight=()=>document.getElementById('refRoot').style.setProperty('--sticky-filter-height',Math.ceil(panel.getBoundingClientRect().height)+'px');
 new ResizeObserver(syncHeight).observe(panel);
 many('[data-filter=type] button').forEach((b,i)=>b.onclick=()=>setKind(kinds[i]));
 many('[data-filter=mode] button').forEach((b,i)=>b.onclick=()=>{mode=i?'range':'single';dates(from,to);publish();});
 many('[data-date]').forEach(b=>b.onclick=()=>chooseDate(b.dataset.date,b));
 many('[data-shift]').forEach(b=>b.onclick=()=>shift(Number(b.dataset.shift)));
 const edit=one('.filter-base-edit');edit.onclick=()=>chooseDate('base',edit);
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
window.referenceFilters=Object.freeze({init,refresh,setMetric,setOrganizations,setWorks,openImportedPeriod,syncDetails,get scope(){return scope;}});
})();
