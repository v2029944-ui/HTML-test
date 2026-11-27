// Original analytics controls, backed exclusively by bounded SQLite requests.
(function(){
'use strict';
const UI=window.unifiedUI,$=UI.$,api=UI.api;
const {titles,observed,incomplete,averageNote}=window.analysisSummary;
const usualLabels={normal:'Обычный уровень',high:'Необычно выше',low:'Необычно ниже',insufficient:'Недостаточно данных'};
const regularLabels={frequent:'Часто',rare:'Редко',rarer:'Заметно реже',disappeared:'Перестала встречаться',new:'Впервые в загруженной истории',dormant:'Долго без выполнений',occasional:'Встречается',absent:'Нет выполнений',uncertain:'Есть пропуски',unknown:'Нет сопоставимых отчётов'};
const RESULTS_PAGE_SIZE=50;
const channels=new Map();
let initialized=false,scope=null,activeView='dashboard',dateKind=null,changesKind=null;
let changeOrganizations=new Set(),changeMetrics=new Set(['count','amount']),usualMetrics=new Set(['count','amount','average']),comparisonMetrics=new Set(['amount']);
let customComparisonMetrics=false;
function slot(name){if(!channels.has(name))channels.set(name,{ticket:0,index:0,page:null,captured:null,timer:null});return channels.get(name);}
function clear(id,text='',reset=true){const node=$(id);if(node){if(reset)window.sqlRowScroll?.reset(node);node.replaceChildren();if(text)node.append(UI.el('p',text,'muted small'));}}
function value(id){return $(id)?.value||'';}
 function orientation(){return value('chartOrientation')==='horizontal'?'horizontal':'vertical';}
function selected(){return Boolean(scope&&(scope.allNames===true||scope.selectedNames?.length));}
function metrics(){return [...(UI.state?.selectedMetrics||['count','amount'])].filter(m=>titles[m]);}
function fmt(n,m='count'){return UI.fmt(n,m);}
function percent(n){
 if(!Number.isFinite(n))return '—';
 if(n!==0&&Math.abs(n)<0.05)return '≈ 0%';
 return (n===0?0:n).toLocaleString('ru-RU',{maximumFractionDigits:1})+'%';
}
function delta(n,m){return window.analysisSummary.displayedChange(n,m,fmt).value;}
function rawDifference(now,base){return [Number.isFinite(now)&&Number.isFinite(base)?now-base:null,Number.isFinite(now)&&Number.isFinite(base)&&base!==0?(now-base)/Math.abs(base)*100:null];}
function number(id){const raw=value(id).trim(),n=Number(raw);if(!raw||!Number.isFinite(n)||n<0)throw Error('Укажите неотрицательные числовые пороги.');return n;}
function textError(e){return window.analizUserError?window.analizUserError(e):(e?.message||String(e));}
function current(s,t,revision){return s.ticket===t&&UI.revision===revision;}
function invalidate(name){const s=slot(name);for(const id of s.ids||[]){if($(id)){$(id).inert=false;$(id).setAttribute('aria-busy','false');}window.sqlRowScroll?.reset($(id));}s.ticket++;s.page=null;s.captured=null;s.index=0;delete s.metricTotals;if(s.timer){clearTimeout(s.timer);s.timer=null;}return s;}
function pager(name,result,index){
 const state=slot(name);state.page=result;state.index=index;
 const root=$(name+'Results'),previous=$(name+'Prev'),next=$(name+'Next'),label=$(name+'Page'),revision=UI.revision,mine=state.ticket;
 const count=result.rows?.length??result.count??0,total=result.total_rows,size=RESULTS_PAGE_SIZE,pages=Math.max(1,Math.ceil(total/size));
 const run=name==='changes'?i=>changes(i,true):name==='contribution'?i=>contributions(i,true):i=>patterns(name,i,true);
 const go=i=>{if(current(state,mine,revision)&&state.page===result&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return run(i);return false;};
 if(root&&window.sqlRowScroll?.attach(root,{index,size,count,total,label:'Результаты анализа',direct:true,go,footer:previous?.parentElement})){window.sqlRowScroll?.retire?.(previous,next,label);if(label)label.textContent='';return;}
 if(previous)previous.disabled=index===0;if(next)next.disabled=index+1>=pages;if(label)label.textContent='Показано '+count+' из '+total+' позиций';
}
function loading(name,ids,paging=false){
 const s=slot(name),ticket=++s.ticket,revision=UI.revision,retain=s.renderedRevision===revision;
 s.page=null;s.captured=null;s.ids=ids;
 if(!retain&&$(name+'Page'))$(name+'Page').textContent='Загрузка…';
 for(const id of ids){if(!retain)clear(id,'Загрузка…',!paging);const node=$(id);if(node){node.inert=true;node.setAttribute('aria-busy','true');}}
 for(const suffix of ['Prev','Next'])if($(name+suffix))$(name+suffix).disabled=true;
 let failed=false;
 return {s,ticket,revision,valid:()=>current(s,ticket,revision),done(){
  if(!current(s,ticket,revision))return;
  s.renderedRevision=failed?null:revision;
  for(const id of ids){const node=$(id);if(node){node.inert=false;node.setAttribute('aria-busy','false');}}
 },error(error){if(current(s,ticket,revision)){failed=true;for(const id of ids)clear(id,'Не удалось загрузить данные. '+textError(error));if($(name+'Page'))$(name+'Page').textContent='Не удалось загрузить строки.';}}};
}
function schedule(name,fn){const s=invalidate(name);s.timer=setTimeout(()=>{s.timer=null;fn();},160);}
function checkboxList(id,items,chosen,onChange){if(window.unifiedOriginal?.renderMultiFilter)return window.unifiedOriginal.renderMultiFilter(id,items,chosen,onChange);const root=$(id);if(!root)return;root.replaceChildren();
 for(const [key,label] of items){const row=UI.el('label',null,'multi-choice'),input=UI.el('input');input.type='checkbox';input.value=key;input.checked=chosen.has(key);input.addEventListener('change',()=>{if(input.checked)chosen.add(key);else chosen.delete(key);onChange();});row.append(input,UI.el('span',label));root.append(row);}
}
function nativeRange(kind,start,end,source){const range=UI.range(kind,start,end,source);if(!range)throw Error('Выберите корректный период: начало не позже конца.');return range;}
function fieldsRange(start,end,kind=scope?.displayType,source=scope?.source){return nativeRange(kind,value(start),value(end||start),source);}
function periodEnd(start,kind){const d=new Date(start+'T00:00:00Z');if(kind==='week')d.setUTCDate(d.getUTCDate()+6);if(kind==='month'){d.setUTCMonth(d.getUTCMonth()+1);d.setUTCDate(0);}if(kind==='year'){d.setUTCFullYear(d.getUTCFullYear()+1,0,1);d.setUTCDate(0);}return d.toISOString().slice(0,10);}
function previous(date,kind){const d=new Date(date+'T00:00:00Z');if(kind==='month')d.setUTCMonth(d.getUTCMonth()-1);else if(kind==='year')d.setUTCFullYear(d.getUTCFullYear()-1);else d.setUTCDate(d.getUTCDate()-(kind==='week'?7:1));return d.toISOString().slice(0,10);}
function setDate(id,kind,date,force=false){const node=$(id);if(node&&(!node.dataset.unifiedTouched||force))UI.setPeriodInput(id,kind,date);}
function copyScope(next){return {...next,organizations:[...next.organizations],selectedNames:[...(next.selectedNames||[])],selectedKeys:[...(next.selectedKeys||[])]};}
function scopeFor(range,overrides={}){return {...copyScope(scope),...range,...overrides};}
function request(s,overrides={}){return UI.request(s,overrides);}
// Composition compares the sums of two exact ranges and never needs calendar buckets.
function compositionRequest(s,overrides){
 return request(s.source==='daily'?{...s,displayType:'day'}:s,overrides);
}
function historyGrouping(s){
 if(s.source!=='daily')return null;
 const kind=s.displayType;
 if(kind==='week' && new Date(s.from+'T00:00:00Z').getUTCDay()===1 && new Date(s.to+'T00:00:00Z').getUTCDay()===0)return kind;
 if(kind==='month' && s.from.endsWith('-01') && s.to===periodEnd(s.to.slice(0,7)+'-01',kind))return kind;
 if(kind==='year' && s.from.endsWith('-01-01') && s.to.endsWith('-12-31'))return kind;
 return 'day';
}
function fullExport(task,req,section,metric,fields={}){
 const spec={request:req,section,metric,...fields},panel=window.referenceDetail?.active;
 if(!panel)return null;
 const current=()=>task.valid()&&window.referenceDetail.active===panel;
 return {export:async()=>{
  if(!current())throw Error('Параметры анализа изменились. Рассчитайте заново.');
  const path=await api.chooseCsvPath(section+'-'+metric+'-'+req.query.from+'-'+req.query.to+'.csv');
  if(!path)return;
  if(!current())throw Error('Данные изменились. Рассчитайте заново.');
  await api.referenceDetailCsv(spec,path);
 }};
}
function dateLabel(iso){return /^\d{4}-\d{2}-\d{2}$/.test(iso)?iso.slice(8,10)+'.'+iso.slice(5,7)+'.'+iso.slice(0,4):iso;}
function visibleEnd(range){return range.source==='daily'?range.to:periodEnd(range.to,range.displayType);}
function workScopeText(s){return s.allNames===true?'Область расчёта: все работы.':'Область расчёта: выбранные работы и группы ('+(s.selectedNames?.length||0)+'). Сводка и главный «Анализ изменений» учитывают все работы.';}
function caption(range){const end=visibleEnd(range);return dateLabel(range.from)+(end!==range.from?' — '+dateLabel(end):'');}
function sampleCaption(first,last,kind){const end=periodEnd(last,kind);return dateLabel(first)+(end===first?'':' — '+dateLabel(end));}
function periodLabel(start,kind){return dateLabel(start)+(kind==='week'?' — '+dateLabel(periodEnd(start,'week')):'');}
function workLabel(name){return String(name).trim().replace(/,\s*$/,'');}
function entryLabel(entry,name=entry.name){return name+(entry.kind==='group'?' (группа)':'');}
function itemButton(entry,s,organization=null,target='analytics',label=entryLabel(entry,workLabel(entry.name))){const button=UI.el('button',label,'service-link text-button');button.type='button';button.dataset.historyEntry=JSON.stringify([entry.key,organization,s.from,s.to]);button.addEventListener('click',()=>history({...entry,name:label},s,organization,target,button));return button;}
function plotTimeline(result,entry,s,root,chosenMetrics,seriesType){
 const points=result.points||[],periods=window.analysisSummary.timelinePeriods(points,result.granularity,s,periodEnd);
 const data=points.map(p=>Number.isFinite(observed(p)?.count)?new Map([[entry.key,observed(p)]]):null);
 for(const metric of chosenMetrics){const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');section.append(UI.el('h3',titles[metric]),plot);root.append(section);UI.drawChart(periods,new Set(s.organizations),metric,plot,{chart:'bar',orientation:orientation(),categories:[entry.key],data,label:()=>entry.name,seriesType,pointNotice:i=>incomplete(points[i])?'Неполные данные':''});}
}
const historyTriggers=new Map();
function historyLinks(host,entry){return host?[...host.querySelectorAll('[data-history-entry]')].filter(button=>button.dataset.historyEntry===entry):[];}
function closeHistory(prefix){
 invalidate(prefix+'History');$(prefix==='change'?'changeDetails':'analyticsDetails').hidden=true;
 const origin=historyTriggers.get(prefix);historyTriggers.delete(prefix);if(!origin)return;
 const {button,host,entry,index}=origin;
 const trigger=button.isConnected?button:historyLinks(host,entry)[index];
 trigger?.focus();
}
async function history(entry,s,organization,target,trigger){
 if(!entry.key||entry.other)return;
 const prefix=target==='change'?'change':'analytics',panel=$(prefix==='change'?'changeDetails':'analyticsDetails'),title=$(prefix==='change'?'changeDetailTitle':'analyticsDetailTitle'),content=$(prefix==='change'?'changeDetailContent':'analyticsDetailContent');
 if(!panel||!content)return;
 const host=trigger.closest?.('#changesResults,#comparisonTable,#contributionResults,#structureResults,#usualResults,#regularityResults'),identity=trigger.dataset.historyEntry;
 historyTriggers.set(prefix,{button:trigger,host,entry:identity,index:historyLinks(host,identity).indexOf(trigger)});
 panel.hidden=false;title.textContent=entry.name+(organization?' · '+organization:'');
 title.focus({preventScroll:true});panel.scrollIntoView?.({block:'nearest'});
 const task=loading(prefix+'History',[content.id]);
 try{const grouping=historyGrouping(s);const result=await api.nativeWorkTimeline(s.revision,s.type,s.from,s.to,s.organizations,organization,entry.key,null,grouping,s.selectedNames||[]);if(!task.valid())return;content.replaceChildren();content.append(UI.el('p','Динамика за '+caption(s)+'. Пропуски не считаются нулём. Данные представлены по периодам.','muted small'));plotTimeline(result,entry,grouping?{...s,displayType:grouping}:s,content,target==='change'?[...changeMetrics]:(s.historyMetrics??metrics()));}
 catch(error){task.error(error);}finally{task.done();}
}

function explainSensitivity(){
 const percent=Number(value('changesPercent')),count=Number(value('changesCountMin')),amount=Number(value('changesAmountMin'));
 if(!Number.isFinite(percent)||!Number.isFinite(count)||!Number.isFinite(amount))return;
 const out=$('changesSensitivityValue'),example=$('changesSensitivityExample');
 if(out)out.textContent=percent.toLocaleString('ru-RU',{maximumFractionDigits:1})+'%';
 if(example)example.textContent='Пример: если было 100 единиц количества / объёма, изменение на '+percent.toLocaleString('ru-RU',{maximumFractionDigits:1})+
  ' достигает процентного порога. Для «сильного изменения» также нужна абсолютная разница от '+count.toLocaleString('ru-RU')+
  ' по количеству / объёму или от '+amount.toLocaleString('ru-RU')+' по сумме. Чем выше процентный порог, тем меньше изменений попадёт в список.';
}
function describeActiveFilters(){
 const node=$('changesActiveFilter');if(!node)return;
 const labels={up:'рост',down:'снижение','significant-up':'значительный рост','significant-down':'значительное снижение',new:'новые',gone:'нет выполнений',small:'малая база',negative:'отрицательные',unchanged:'без изменений'};
 const tags=[],direction=value('changesDirection'),search=value('changesSearch').trim();
 if(labels[direction])tags.push('фильтр: '+labels[direction]);
 if(value('changesMode')==='strong')tags.push('только заметные');
 if(search)tags.push('поиск: '+search.slice(0,30));
 node.textContent=tags.join(', ');
}
function changeLayout(){explainSensitivity();describeActiveFilters();const single=value('changesMethod')==='previous';$('changesFromLabel').hidden=single;$('changeDatePair').classList.toggle('single',single);$('changesToText').textContent=single?'Текущий период':'По';if(single)delete $('changesTo').dataset.rangeBoundary;else $('changesTo').dataset.rangeBoundary='end';
 const search=Boolean(value('changesSearch').trim());$('changesSort').disabled=search;$('changesSort').title=search?'Сначала начала названий, затем совпадения внутри, по алфавиту.':'';
 for(const option of $('changesSort').options||[])option.disabled=option.value.includes('-')&&!changeMetrics.has(option.value.split('-')[1]);
 if($('changesSort').selectedOptions?.[0]?.disabled&&changeMetrics.size)$('changesSort').value='significance-'+[...changeMetrics][0];
}
function changeContext(){
 if(!scope||!changeOrganizations.size||!changeMetrics.size)throw Error('Выберите подразделения и хотя бы один показатель.');
 const shared=referencePanel('changes'),kind=shared?scope.displayType:value('changesType');
 const method=shared?(scope.to<=periodEnd(scope.from,kind)?'previous':'range'):value('changesMethod');
 const range=shared?scope:fieldsRange(method==='previous'?'changesTo':'changesFrom','changesTo',kind,scope.source);
 const s=scopeFor(range,{organizations:[...changeOrganizations],selectedNames:[],selectedKeys:[],allNames:true});
 return {scope:s,request:request(s,{method,percent:number('changesPercent'),min_count:number('changesCountMin'),min_amount:number('changesAmountMin')}),options:{metrics:[...changeMetrics],mode:value('changesMode'),search:value('changesSearch'),direction:value('changesDirection'),sort:value('changesSort')}};
}
function changeMeasure(row,metric){
 const m=row[metric]??(metric==='amount'?{base:row.observedAmountBefore,current:row.observedAmountAfter}:{});
 return {...m,smallBase:m.smallBase??m.small_base??false};
}
function changeLabel(row,m,shown){if(m.negative)return 'Изменение с отрицательными значениями';if(row.status==='new')return 'Новая';if(row.status==='gone')return 'Нет выполнений';return shown.direction==='up'?'Рост':shown.direction==='down'?'Снижение':shown.label;}
function changeColor(direction){return direction==='up'?'numeric-rise':direction==='down'?'numeric-fall':'';}
function renderChangeSummary(summary,chosen){clear('changesSummary');const root=$('changesSummary');
 for(const [label,n,cls,direction] of [['Значительно выросло',summary.grown,'numeric-rise','significant-up'],['Значительно снизилось',summary.fallen,'numeric-fall','significant-down'],['Новых услуг',summary.new,'','new'],['Нет выполнений',summary.gone,'','gone']]){
  const card=UI.el('button',null,'kpi kpi-action');card.type='button';
  const explanation=direction.startsWith('significant-')
   ?'Изменение не меньше '+value('changesPercent')+'% и минимальной разницы: '+value('changesCountMin')+' по количеству / объёму или '+value('changesAmountMin')+' по сумме. Без отрицательных значений и малой базы.'
   :direction==='new'?'Работа появилась в загруженном периоде и отсутствовала в базовом.':
    'В базовом периоде работа выполнялась, в текущем выполнений нет. Пропуски не считаются нулём.';
  card.title=explanation;card.setAttribute('aria-label',label+': '+n+'. '+explanation+' Показать строки этой категории.');
  card.append(UI.el('span',label),UI.el('strong',String(n),n?cls:''));
  card.addEventListener('click',()=>{$('changesDirection').value=direction;$('changesMode').value='all';invalidate('changes');void changes();$('changesResults').scrollIntoView?.({block:'start'});});
  root.append(card);
 }
 for(const metric of chosen){const top=summary['top_'+metric],m=top&&changeMeasure(top,metric),shown=window.analysisSummary.displayedChange(m?.delta,metric,fmt),card=UI.el('div',null,'kpi');card.append(UI.el('span','Наибольшее абсолютное изменение ('+titles[metric]+')'),UI.el('strong',shown.value,changeColor(shown.direction)));if(top)card.append(UI.el('small',entryLabel(top,workLabel(top.name))+' · '+top.org+' · '+periodLabel(top.baseStart,value('changesType'))+' / '+periodLabel(top.currentStart,value('changesType'))));root.append(card);}
}
async function changes(index=0,paging=false){
 changeLayout();const task=loading('changes',['changesResults','changesSummary','changesWarnings'],paging);$('changeDetails').hidden=true;
 try{
  const captured=changeContext(),chosen=captured.options.metrics;
  // One 50-record SQL page expands to one 50-row display page per selected metric.
  const sqlIndex=Math.floor(index/chosen.length),first=(index%chosen.length)*RESULTS_PAGE_SIZE;
  const result=await api.unifiedChangesPageAtPage(captured.request,captured.options,sqlIndex);if(!task.valid())return;
  const end=Math.min(first+RESULTS_PAGE_SIZE,result.rows.length*chosen.length);
  clear('changesResults','',false);clear('changesWarnings','',false);renderChangeSummary(result.summary,captured.options.metrics);
  const diagnostic=result.summary.totals;
  const missingAmount=result.summary.rows_with_missing_amount??0;
  if(missingAmount||diagnostic?.missing_pairs||diagnostic?.partial_pairs||diagnostic?.invalid_reports||diagnostic?.duplicate_issues||diagnostic?.overlap_issues||diagnostic?.import_warning_pairs){
   const problems=[];
   if(missingAmount>0)problems.push('Есть строки без суммы. Сравнение суммы учитывает заполненные ячейки, а среднее — сопоставимые количества из тех же строк.');
   if(diagnostic.missing_pairs>0)problems.push('Пар без отчётов за один или оба периода: '+diagnostic.missing_pairs+' (не включены в сопоставление).');
   if(diagnostic.partial_pairs>0)problems.push('Пар по доступным отчётам: '+diagnostic.partial_pairs+' (есть пропуски).');
   if(diagnostic.import_warning_pairs>0)problems.push('Пар с замечаниями импорта: '+diagnostic.import_warning_pairs+' (проверьте источник; замечание не означает автоматического исключения).');
   if(diagnostic.invalid_reports>0)problems.push('Некорректных отчётов: '+diagnostic.invalid_reports+'.');
   if(diagnostic.duplicate_issues>0)problems.push('Повторных отчётов: '+diagnostic.duplicate_issues+'.');
   if(diagnostic.overlap_issues>0)problems.push('Пересекающихся отчётов: '+diagnostic.overlap_issues+'.');
   const total=Number.isSafeInteger(result.total_rows)?result.total_rows:result.summary.rows;
   $('changesWarnings').append(UI.el('p','Сопоставимых строк по текущим фильтрам: '+total+'. '+problems.join(' ')+' Пропуски не считаются нулём.','warn small'));
  }
  for(const pair of result.missing_examples||[])$('changesWarnings').append(UI.el('p',pair.org+': нет отчётов за '+[pair.missing_base?pair.base_start:null,pair.missing_current?pair.current_start:null].filter(Boolean).map(date=>periodLabel(date,captured.scope.displayType)).join(', '),'warn small'));
  if(first>=end)clear('changesResults','Нет сопоставимых записей по выбранным условиям. Измените фильтры или проверьте наличие соседних отчётов.',false);
  const rows=[],raw=[];
  for(let position=first;position<end;position++){
   const row=result.rows[Math.floor(position/chosen.length)],metric=chosen[position%chosen.length];
   const m=changeMeasure(row,metric),name=itemButton(row,{...captured.scope,from:row.baseStart,to:captured.scope.source==='daily'?periodEnd(row.currentStart,captured.scope.displayType):row.currentStart},row.org,'change');const shown=window.analysisSummary.displayedChange(m.delta,metric,fmt),direction=changeLabel(row,m,shown),kind=m.smallBase?' · малая база':'';
   rows.push([name,row.org,periodLabel(row.baseStart,captured.scope.displayType),periodLabel(row.currentStart,captured.scope.displayType),titles[metric],fmt(m.base,metric),fmt(m.current,metric),delta(m.delta,metric),percent(m.percent),UI.el('span',direction+kind,changeColor(shown.direction))]);raw.push([row.name,row.org,row.baseStart,row.currentStart,titles[metric],m.base,m.current,m.delta,m.percent,direction+kind]);}
  if(rows.length)UI.table($('changesResults'),'count',caption(captured.scope),['Работа / группа','Подразделение','Базовый период','Текущий период','Показатель','Было','Стало','Изменение','Изменение, %','Направление'],rows,{title:'Изменения между соседними периодами',totalRow:false,csvRows:raw,csvContext:{text:'Все строки по текущим фильтрам',export:async()=>{
   if(!task.valid())throw Error('Параметры анализа изменились. Рассчитайте заново.');const path=await api.chooseCsvPath('izmeneniya-'+captured.scope.from+'-'+captured.scope.to+'.csv');if(!path)return;if(!task.valid())throw Error('Данные изменились. Рассчитайте заново.');await api.unifiedChangesCsv(captured.request,captured.options,path);
  }}});
  task.s.captured=captured;pager('changes',{count:rows.length,total_rows:result.total_rows*chosen.length},index);
 }catch(error){task.error(error);}finally{task.done();}
}
function referencePanel(id){return window.referenceDetail?.active===id;}
function sharedRanges(){
 return {before:{...scope,from:scope.baseFrom,to:scope.baseTo},after:{...scope}};
}
function comparisonRanges(){
 const local=window.referenceDetail?.currentComparison?.();
 if(local)return {before:{...scope,...local.before},after:{...scope,...local.after}};
 const ranges=value('periodComparisonScope')==='range';
 if(referencePanel('comparison'))return sharedRanges();
 return {before:fieldsRange(ranges?'baseRangeStart':'basePeriod',ranges?'baseRangeEnd':'basePeriod'),after:fieldsRange(ranges?'currentRangeStart':'currentPeriod',ranges?'currentRangeEnd':'currentPeriod')};
}
// Coverage is independent of the work filter. A missing range response never
// contributes a zero-valued member to the comparable sample.
async function sampleFor(s,ranges){
 const pages=await Promise.all(ranges.map(range=>api.nativeAnalysisOrganizations(s.revision,s.type,range.from,range.to,s.organizations,null,null)));
 const complete=pages.map(rows=>new Set(rows.filter(row=>row.complete).map(row=>row.org)));
 const included=s.organizations.filter(org=>complete.every(list=>list.has(org)));
 return {included,excluded:s.organizations.filter(org=>!included.includes(org)),pages};
}
function sampleNote(root,sample,message,compareAvailable=false){
 if(!sample?.excluded.length){if(message)root.append(UI.el('p',message,'warn small'));return;}
 const banner=UI.el('div',null,'comparison-warning'),total=sample.included.length+sample.excluded.length;
 const label=compareAvailable?'Полные отчёты у '+sample.included.length+' из '+total+' подразделений':sample.included.length?'Сопоставимые подразделения: '+sample.included.length+' из '+total:'Нет сопоставимых подразделений';
 banner.append(UI.el('strong',label),
  UI.el('p',message||'Часть подразделений имеет неполные данные.'));
 const reasons=UI.el('details');reasons.append(UI.el('summary','Причины по подразделениям'));
 sample.pages.forEach((rows,index)=>{for(const org of sample.excluded){
  const row=rows.find(item=>item.org===org);
  if(!row?.complete)reasons.append(UI.el('p',org+' · '+(sample.pages.length===1?'анализируемый период':index===0?'базовый период':'анализируемый период')+' · '+(row?'пропусков: '+row.missing:'нет отчёта')));
 }});
 banner.append(reasons);
 if(sample.included.length){const button=UI.el('button',compareAvailable?'Сравнить только с полными отчётами':'Сравнить только сопоставимые','button secondary');
  button.type='button';button.addEventListener('click',()=>{
   window.referenceFilters.setOrganizations(sample.included);const heading=$('comparisonHeading');heading.tabIndex=-1;heading.focus();
  });banner.append(button);
 }
 root.append(banner);
}
function organizationRange(){return referencePanel('comparison')?scope?.overviewMode==='range':value('comparisonScope')==='range';}
function comparisonLayout(){const organizations=value('comparisonMode')==='organizations',ranges=value('periodComparisonScope')==='range',orgRange=value('comparisonScope')==='range';
 $('periodComparisonFilters').hidden=organizations;$('orgComparisonFilters').hidden=!organizations;$('kpis').classList.toggle('kpis',!organizations);const charts=$('comparisonCharts');
 charts.hidden=false;
 charts.setAttribute('aria-label',organizations?'Графики сравнения подразделений':'Графики сравнения периодов');
 $('comparisonDynamics').hidden=!organizations||!organizationRange();$('contributions').hidden=organizations;
 $('periodComparisonSingle').hidden=ranges;$('periodComparisonRanges').hidden=!ranges;$('comparisonPeriodLabel').hidden=orgRange;$('comparisonRangeGroup').hidden=!orgRange;
}
function addBoundedPager(root,state,rows,run,total){
 const index=state.index,mine=state.ticket,revision=UI.revision,pages=Math.max(1,Math.ceil(total/50));
 const go=i=>{if(current(state,mine,revision)&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return run(i);return false;};
 if(window.sqlRowScroll?.attach(root,{index,size:50,total,count:rows.length,label:'Сравнение работ',direct:true,go}))return;
 const bar=UI.el('div',null,'change-pagination'),previous=UI.el('button','← Предыдущая','button secondary'),next=UI.el('button','Следующая →','button secondary');previous.type=next.type='button';previous.disabled=index===0;next.disabled=index+1>=pages;previous.addEventListener('click',()=>go(index-1));next.addEventListener('click',()=>go(index+1));bar.append(previous,UI.el('span','Показано '+rows.length+' из '+total,'muted small'),next);root.append(bar);
}
const contributionFields=['contributionResults','contributionCheck'];
async function comparison(index=0,paging=false){
 if(window.referenceDetail?.active&&!referencePanel('comparison'))return;
 comparisonLayout();if(!scope)return;
 const mode=value('comparisonMode'),state=slot('comparison');
 if(state.renderedMode!==mode){
  // Results for a different comparison type no longer match the active controls.
  state.renderedMode=mode;state.renderedRevision=null;
  slot('contribution').renderedRevision=null;
 }
 const task=loading('comparison',['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis'],paging);$('analyticsDetails').hidden=true;
 const contributionTask=loading('contribution',contributionFields);
 window.sqlRowScroll?.reset($('contributionResults'));
 let contributionInput=null;
 try{if(!selected())throw Error('Отметьте работы или группы для сравнения.');
  if(mode==='organizations'){await departmentComparison(task,index);return;}
  const {before,after}=comparisonRanges(),s=scopeFor(after);
  const sample=referencePanel('comparison')?await sampleFor(s,[before,after]):null;
  if(!task.valid())return;
  const req=request(s,{percent:0,min_count:0,min_amount:0});
  const [dashboard,windowPage]=await Promise.all([api.nativeAnalysisCompare(s.revision,s.type,before.from,before.to,after.from,after.to,s.organizations,null,s.selectedNames),api.unifiedComparisonWindow(req,{from:before.from,to:before.to},null,index,50)]);
  if(!task.valid())return;for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis'])clear(id,'',false);
  const page=windowPage.rows,summary=dashboard.summary;
  $('comparisonHelp').textContent=workScopeText(s)+' Второй интервал сравнивается с первым по отмеченным работам. Прочерк означает, что значение нельзя рассчитать. При нулевой базе процент не рассчитывается.';
  const coverage=$('comparisonCoverage'),warnings=[];
  const beforeComplete=summary.before.complete,afterComplete=summary.after.complete;
  const beforeValues=observed(summary.before),afterValues=observed(summary.after);
  const beforeObserved=beforeComplete||Number.isFinite(beforeValues?.count),afterObserved=afterComplete||Number.isFinite(afterValues?.count);
  const compareAvailable=beforeObserved&&afterObserved;
  if(!beforeComplete||!afterComplete)warnings.push('Неполные данные '+(!beforeComplete&&!afterComplete?'базового и анализируемого периодов':!beforeComplete?'базового периода':'анализируемого периода')+
   (compareAvailable?'. Разница и проценты рассчитаны по доступным отчётам.':'. Показаны доступные показатели.')+' Пропуски не считаются нулём.');
  const missing=summary.before.missing+summary.after.missing;
  if(missing>0)warnings.push('Пропусков: '+missing+' (подразделение × период).');
  if([summary.before,summary.after].some(period=>period.amount_missing_rows||
     (period.complete&&Number.isFinite(observed(period)?.count)&&period.total.amount==null))){
   warnings.push('Итоги и разница по сумме учитывают заполненные ячейки. Покрытие может различаться.');
   for(const [label,period] of [['База',summary.before],['Анализ',summary.after]]){
    const note=averageNote(period);
    if(note)warnings.push(label+' · '+note);
   }
  }
  sampleNote(coverage,sample,warnings.join(' '),compareAvailable);
  for(const [label,total] of [['Первый интервал',summary.before],['Второй интервал',summary.after]]){
   if(total.complete)coverage.append(UI.el('p',label+': Полные данные · периодов '+total.periods,'muted small'));
  }
  contributionInput={before,after};
  if(!beforeObserved&&!afterObserved){
   clear('comparisonTable','Нет доступных значений для сравнения: оба интервала неполны. Проверьте наличие отчётов или выберите другие периоды. Пропуск не означает ноль.');
   return;
  }
  const onlyOne=beforeObserved!==afterObserved;
  const periods=[before,after].map(range=>({
   type:s.displayType,start:range.from,actualStart:range.from,end:visibleEnd(range),partial:true
  }));
  const data=[beforeValues,afterValues].map(total=>new Map([['total',total??{count:null,amount:null,average:null}]]));
  for(const metric of comparisonMetrics){
   const now=afterValues?.[metric],old=beforeValues?.[metric],card=UI.el('div',null,'kpi');
   const totalDiff=metric==='average'&&summary.average
    ?[summary.average.delta,summary.average.percent]
    :rawDifference(now,old);
   card.append(UI.el('span',titles[metric]+' · анализируемый период'),UI.el('strong',fmt(now,metric)),
    UI.el('small',totalDiff[0]==null?'Сравнение недоступно':[delta(totalDiff[0],metric),percent(totalDiff[1])].join(' · ')));
   $('kpis').append(card);
   const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');
   section.append(UI.el('h3',titles[metric]),plot);
   $('comparisonCharts').append(section);
   UI.drawChart(periods,new Set(s.organizations),metric,plot,{
    chart:'bar',comparison:true,categories:['total'],data,label:()=>titles[metric],
    periodLabels:['Базовый период','Анализируемый период']
   });
   const availableRange=afterObserved?after:before;
   const combinedRange={...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to};
   const headers=onlyOne?['Работа / группа',afterObserved?'Значение сейчас':'Значение раньше']:
    ['Работа / группа','Значение раньше','Значение сейчас','Разница','Изменение, %'];
   const raw=[],rows=[];
   function append(label,rawLabel,b,a,d,p){
    if(!Number.isFinite(b)&&!Number.isFinite(a))return;
    raw.push(onlyOne?[rawLabel,afterObserved?a:b]:[rawLabel,b,a,d,p]);
    rows.push(onlyOne?[label,fmt(afterObserved?a:b,metric)]:[label,fmt(b,metric),fmt(a,metric),delta(d,metric),percent(p)]);
   }
   append('Итого по выбранным работам','Итого по выбранным работам',old,now,...totalDiff);
   for(const entry of page){
    const measure=entry[metric]||{};
    const b=entry.observed_before?.[metric]??measure.base??(beforeComplete&&!afterComplete?entry.available?.[metric]:null);
    const a=entry.observed_after?.[metric]??measure.current??(afterComplete&&!beforeComplete?entry.available?.[metric]:null);
    append(itemButton(entry,onlyOne?{...s,...availableRange}:combinedRange),entryLabel(entry),b,a,measure.delta??null,measure.percent??null);
   }
   if(rows.length)UI.table($('comparisonTable'),metric,onlyOne?caption(availableRange):caption(before)+' / '+caption(after),
    headers,rows,{csvRows:raw,csvContext:fullExport(task,req,'comparison',metric,{before:{from:before.from,to:before.to}})});
  }
  task.s.index=index;addBoundedPager($('comparisonTable'),task.s,page,i=>comparison(i,true),windowPage.total_rows);
 }catch(error){contributionInput=null;task.error(error);contributionTask.error(error);}
 finally{
  task.done();contributionTask.done();
  if(task.valid()&&contributionInput){
   task.s.captured=contributionInput;
   if(!$('contributions').hidden)void contributions();
  }
 }
}
function organizationTotals(s,totals){
 const total=[...totals.values()].reduce((sum,row)=>sum+(Number.isFinite(row.amount)?row.amount:0),0);
 const title='Итоги по подразделениям',section=UI.el('section',null,'metric-table');
 const scroll=UI.el('div',null,'table-scroll'),table=UI.el('table',null,'table');
 scroll.tabIndex=0;scroll.setAttribute('role','region');scroll.setAttribute('aria-label',title);
 const head=UI.el('thead'),header=UI.el('tr'),body=UI.el('tbody');
 for(const text of ['Подразделение',titles.amount,titles.count,'Доля суммы',titles.average]){
  const cell=UI.el('th',text);cell.setAttribute('scope','col');header.append(cell);
 }
 head.append(header);
 for(const org of s.organizations){
  const {amount,count,average}=totals.get(org),row=UI.el('tr'),label=UI.el('th',org);
  label.setAttribute('scope','row');row.append(label);
  const share=Number.isFinite(amount)&&Number.isFinite(total)&&total>0?amount/total*100:null;
  for(const text of [fmt(amount,'amount'),fmt(count,'count'),percent(share),fmt(average,'average')])row.append(UI.el('td',text));
  body.append(row);
 }
 table.append(head,body);scroll.append(table);section.append(UI.el('h3',title),scroll);$('kpis').append(section);
}
async function departmentComparison(task,index){
 const range=referencePanel('comparison')?scope:value('comparisonScope')==='range'?fieldsRange('comparisonRangeStart','comparisonRangeEnd'):fieldsRange('comparisonPeriod'),s=scopeFor(range),chosen=[...comparisonMetrics];let baseOrg=value('comparisonBaseOrg');
 const organizations=await api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,s.selectedNames);
 if(!task.valid())return;
 const reference=referencePanel('comparison'),included=s.organizations.filter(org=>organizations.some(row=>row.org===org&&row.complete));
 const sample={included,excluded:s.organizations.filter(org=>!included.includes(org)),pages:[organizations]};
 const reset=()=>{for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis'])clear(id,'',false);};
 if(reference){
  baseOrg=s.organizations.includes(baseOrg)?baseOrg:s.organizations[0];
 }
 const workPage=s.allNames===true?await api.unifiedOrganizationWorkWindow(request(s),index,50):null;
 if(!task.valid())return;
 const perName=workPage?workPage.rows:[];
 if(!workPage){
  const names=(s.nameRows||[]).filter(row=>s.selectedKeys.includes(row.key)).slice(0,50);
  for(let i=0;i<names.length;i+=4){
   const batch=await Promise.all(names.slice(i,i+4).map(async entry=>({entry,rows:await api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,[entry.name])})));
   if(!task.valid())return;
   perName.push(...batch);
  }
 }
 reset();
 const coverage=$('comparisonCoverage'),warnings=[];
 if(organizations.some(org=>!org.complete))warnings.push('Сравнение по доступным отчётам. Разница и проценты требуют данных у обоих подразделений.');
 if(organizations.some(org=>org.amount_missing_rows))warnings.push('Итоги, доли и разница по сумме рассчитаны по заполненным ячейкам. Покрытие может различаться.');
 sampleNote(coverage,reference?sample:null,warnings.join(' '),true);
 if(reference){$('comparisonBaseOrg').replaceChildren(...s.organizations.map(org=>{const option=UI.el('option',org);option.value=org;return option;}));$('comparisonBaseOrg').value=baseOrg;}
 $('comparisonHelp').textContent='Общий интервал для подразделений. Разница относительно '+baseOrg+'. При нулевой базе процент не рассчитывается. Чтобы сравнить одну работу, выберите её в фильтре «Работы» и оставьте одну отметку в «Показатели сравнения».';
 const unknown={count:null,amount:null,average:null};
 const byOrganization=new Map(organizations.map(row=>[row.org,row]));
 const totals=new Map(s.organizations.map(org=>[org,observed(byOrganization.get(org))??unknown]));
 organizationTotals(s,totals);
 const historyScope={...s,historyMetrics:chosen};
 for(const metric of chosen){const raw=[],rows=[];
  const comparable=row=>observed(row)?.[metric];
  const cells=(org,values)=>{
   const row=values.find(o=>o.org===org),base=values.find(o=>o.org===baseOrg);
   const n=comparable(row)??null;
   if(org===baseOrg)return [n];
   const partial=Number.isFinite(observed(row)?.partial_average_quantity)||Number.isFinite(observed(base)?.partial_average_quantity);
   const sameWork=row?.average_source_work!=null&&row.average_source_work===base?.average_source_work;
   const baseValue=metric==='average'&&partial&&!sameWork?null:comparable(base);
   return [n,...rawDifference(n,baseValue)];
  };
  const make=(label,values)=>[label,...s.organizations.flatMap(org=>cells(org,values).map((n,i)=>i===0?fmt(n,metric):i===1?delta(n,metric):percent(n)))];
  const makeRaw=(label,values)=>[label,...s.organizations.flatMap(org=>cells(org,values))];
  rows.push(make('Итого по выбранным работам',organizations));raw.push(makeRaw('Итого по выбранным работам',organizations));
  for(const item of perName){const button=workPage?itemButton(item.entry,historyScope,null,'analytics',item.entry.name):itemButton(item.entry,historyScope);
   rows.push(make(button,item.rows));raw.push(makeRaw(workPage?item.entry.name:entryLabel(item.entry),item.rows));}
  UI.table($('comparisonTable'),metric,caption(range),['Работа / группа',...s.organizations.flatMap(org=>org===baseOrg?[org]:[org,org+' · разница',org+' · %'])],rows,{csvRows:raw,csvContext:fullExport(task,request(s),'organizations',metric,{baseOrg})});
  const plot=UI.el('div',null,'metric-plot');$('comparisonCharts').append(UI.el('h3',titles[metric]+' · '+caption(range)),plot);
  UI.drawChart([{type:s.displayType,start:s.from,end:visibleEnd(s)}],new Set(s.organizations),metric,plot,{chart:'bar',seriesType:'organizations',categories:s.organizations,data:[totals],hideZeroRankingRows:false,rankingHint:!organizationRange()});
 }
 if(workPage){task.s.index=index;addBoundedPager($('comparisonTable'),task.s,perName,i=>comparison(i,true),workPage.total_rows);}
 if(organizationRange())await departmentDynamics(task,s,range,chosen,unknown);
}
async function departmentDynamics(task,s,range,chosen,unknown){
 try{
  const grouping=s.source==='daily'?s.displayType:null;
  const trends=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(0,8),null,grouping,s.selectedNames);
  if(!task.valid())return;
  for(let offset=8;offset<s.organizations.length;offset+=8){
   const page=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(offset,offset+8),null,grouping,s.selectedNames);
   if(!task.valid())return;
   trends.series.push(...page.series);
  }
  const first=trends.series[0]?.points||[];
  if(first.length===1){
   clear('comparisonDynamicsPlots','В диапазоне один период. Выберите более широкий диапазон, чтобы увидеть динамику.',false);
   return;
  }
  const periods=window.analysisSummary.timelinePeriods(first,trends.granularity,s,periodEnd);
  const seriesByOrganization=new Map(trends.series.map(series=>[series.organization,series.points]));
  const data=first.map((_,i)=>new Map(s.organizations.map(org=>[org,observed(seriesByOrganization.get(org)?.[i])??unknown])));
  const pointNotice=(i,shown)=>{
   const missing=shown.filter(org=>{const item=seriesByOrganization.get(org)?.[i];return !item||incomplete(item);});
   return missing.length?'Неполные данные: '+missing.join(', ')+'.':'';
  };
  for(const metric of chosen){
   const plot=UI.el('div',null,'metric-plot');$('comparisonDynamicsPlots').append(UI.el('h3',titles[metric]+' · '+caption(range)),plot);
   UI.drawChart(periods,new Set(s.organizations),metric,plot,{chart:'line',seriesType:'organizations',categories:s.organizations,data,pointNotice});
  }
 }catch(error){if(task.valid())clear('comparisonDynamicsPlots','Не удалось загрузить график. '+textError(error),false);}
}
async function contributions(index=0,paging=false){
 const input=slot('comparison').captured;
 if(!scope||!input||value('comparisonMode')!=='periods')return;
 const task=loading('contribution',contributionFields,paging);
 try{const {before,after}=input,s=scopeFor(after),chosen=metrics().filter(m=>m!=='average');
  if(index===0)task.s.metricTotals={};const metricTotals=task.s.metricTotals||{};
  const result=await Promise.all(chosen.map(metric=>index>0&&Number.isSafeInteger(metricTotals[metric])&&index*50>=metricTotals[metric]?null:api.nativeCompositionPageAtPage(compositionRequest(s,{percent:0,min_count:0,min_amount:0}),{from:before.from,to:before.to},'contributions',metric,index)));
  if(!task.valid())return;clear('contributionResults','',false);clear('contributionCheck','',false);
  $('contributionCheck').append(UI.el('p',s.allNames?'Область расчёта: все работы.':'Область расчёта: отмеченные работы и группы.','muted small'));
  const partial=result.filter(page=>page&&(!page.baseComplete||!page.currentComplete));
  if(partial.length)$('contributionCheck').append(UI.el('p','Показаны доступные значения. Изменение и вклад рассчитаны по работам, для которых показатель доступен в обоих интервалах.','warn small'));
  result.forEach((page,i)=>{const metric=chosen[i];if(!page)return;metricTotals[metric]=page.total_rows;
   if(page.check){
    const check=page.check;$('contributionCheck').append(UI.el('p',titles[metric]+': сумма вкладов '+fmt(check.sum,metric)+'; общее изменение '+fmt(check.delta,metric)+' — '+(check.reconciled?'совпадает':'расхождение '+fmt(check.residual,metric)),check.reconciled?'positive small':'negative small'));
   }else if(!page.rows.length)return;
   const rows=page.rows.map(row=>[itemButton(row,{...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to}),fmt(row.base,metric),fmt(row.current,metric),delta(row.delta,metric),percent(row.percent),percent(row.contribution)]);
   UI.table($('contributionResults'),metric,caption(before)+' / '+caption(after),['Услуга / группа','Значение раньше','Значение сейчас','Изменение значения','Изменение, %','Вклад в общее изменение, %'],rows,{totalRow:false,
    headerHelp:[
     'Работа или группа, которая внесла вклад в изменение общего итога.',
     'Значение работы или группы за выбранный базовый период.',
     'Значение работы или группы за выбранный анализируемый период.',
     'Значение сейчас минус значение раньше. Положительная разница увеличивает общий итог, отрицательная уменьшает.',
     'Изменение значения / модуль прежнего значения × 100%. При нулевой базе процент недоступен.',
     'Изменение работы / изменение общего итога × 100%. Вклад может быть отрицательным или больше 100%, если изменения других работ его компенсируют. При нулевом общем изменении доля недоступна.'
    ],
    csvContext:fullExport(task,compositionRequest(s),'contributions',metric,{before:{from:before.from,to:before.to}}),
    csvRows:page.rows.map(r=>[entryLabel(r),r.base,r.current,r.delta,r.percent,r.contribution])});
  });
  task.s.metricTotals=metricTotals;const total=Math.max(0,...chosen.map(metric=>metricTotals[metric]||0));pager('contribution',{total_rows:total,count:Math.max(0,...result.filter(Boolean).map(p=>p.rows.length))},index);
 }catch(error){task.error(error);}finally{task.done();}
}
async function structure(){
 const task=loading('structure',['structureResults']);
 try{if(!scope)throw Error('Выберите период.');if(value('structureScope')!=='all'&&!selected())throw Error('Отметьте работы или выберите все выполненные работы.');
  const useShared=referencePanel('structurePanel'),shared=useShared?sharedRanges():null;
  const before=shared?.before.from?shared.before:fieldsRange('structureBase');
  const after=shared?shared.after:fieldsRange('structureCurrent');
  const metric=value('structureMetric'),s=scopeFor(after,value('structureScope')==='all'?{selectedNames:[],allNames:true}:{});
  const page=await api.nativeCompositionPage(compositionRequest(s,{percent:0,min_count:0,min_amount:0}),{from:before.from,to:before.to},'structure',metric,null,Number(value('structureLimit')));
  if(!task.valid())return;clear('structureResults');const root=$('structureResults');
  if(!page.currentComplete&&!page.currentAvailable){clear('structureResults','Нет доступных значений за анализируемый период.');return;}
  root.append(UI.el('p',(page.currentComplete?'Итог: ':'Частичный итог: ')+fmt(page.total,metric),'small'));
  const warnings=[];
  const baselineAvailable=page.baseAvailable||page.baseComplete;
  if(!page.currentComplete||(baselineAvailable&&!page.baseComplete))warnings.push(page.sharesValid?'Доли рассчитаны от доступного итога каждого периода.':'Показаны доступные значения.');
  if(!page.sharesValid||(baselineAvailable&&page.baseSharesValid===false))warnings.push('Доли рассчитываются при положительном итоге и отсутствии отрицательных значений.');
  if(!baselineAvailable)warnings.push('За базовый период нет доступных значений.');
  if(warnings.length)root.append(UI.el('p',warnings.join(' '),'warn small'));
  const visible=page.rows.filter(row=>Number.isFinite(row.current)&&row.current!==0);
  const selectedLimit=Number(value('structureLimit'));
  if(visible.length<selectedLimit)root.append(UI.el('p','Показано '+visible.length+' из '+selectedLimit+': у остальных нет ненулевых значений за этот период.','muted small'));
  if(!visible.length){root.append(UI.el('p','Нет ненулевых значений за выбранный период. Нулевые строки рейтинга скрыты.','muted small'));return;}
  const baselineValid=baselineAvailable;
  const label=row=>row.other?row.name:itemButton(row,{...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to});
  const raw=visible.map(row=>baselineValid?
   [entryLabel(row),row.base,row.current,row.baseShare,row.share,row.cumulative,row.shareChange]:
   [entryLabel(row),row.current,row.share,row.cumulative]);
  const rows=visible.map(row=>baselineValid?
   [label(row),fmt(row.base,metric),fmt(row.current,metric),percent(row.baseShare),percent(row.share),percent(row.cumulative),Number.isFinite(row.shareChange)?fmt(row.shareChange,'count')+' п. п.':'—']:
   [label(row),fmt(row.current,metric),percent(row.share),percent(row.cumulative)]);
  const headers=baselineValid?['Услуга / группа','Значение раньше','Значение сейчас','Доля раньше','Доля сейчас','Накопленная доля','Изменение доли']:
   ['Услуга / группа','Значение сейчас','Доля сейчас','Накопленная доля'];
  UI.table(root,metric,caption(before)+' / '+caption(after),headers,rows,{totalRow:false,csvRows:raw,
   headerHelp:headers.map(header=>({
    'Услуга / группа':'Название работы или объединяющей её группы. Строка «Остальные услуги» суммирует работы за пределами выбранного топа.',
    'Значение раньше':'Количество / объём или сумма за базовый период по выбранным подразделениям.',
    'Значение сейчас':'Количество / объём или сумма за анализируемый период по выбранным подразделениям.',
    'Доля раньше':'Значение работы за базовый период / общий итог работ в выбранном составе за этот же период × 100%.',
    'Доля сейчас':'Значение работы за анализируемый период / общий итог работ в выбранном составе за этот же период × 100%.',
    'Накопленная доля':'Сумма текущих долей от первой строки до этой включительно. Строка «Остальные услуги» учитывает оставшиеся работы.',
    'Изменение доли':'Доля сейчас минус доля раньше, в процентных пунктах.'
   })[header]),
   csvContext:fullExport(task,compositionRequest(s),'structure',metric,{before:{from:before.from,to:before.to},limit:selectedLimit})});
  const plot=UI.el('div',null,'metric-plot');root.append(plot);
  const key=row=>row.other?'others':row.key;
  UI.drawChart([{type:s.displayType,start:after.from,end:visibleEnd(after)}],new Set(s.organizations),metric,plot,
   {chart:'bar',orientation:orientation(),categories:visible.map(key),limit:visible.length,
    data:[new Map(visible.map(r=>[key(r),{[metric]:r.current}]))],label:k=>{const row=visible.find(r=>key(r)===k);return entryLabel(row,workLabel(row.name));}});
 }catch(error){task.error(error);}finally{task.done();}
}
async function patterns(mode,index=0,paging=false){
 const name=mode==='usual'?'usual':'regularity',task=loading(name,[name+'Results',name+'Coverage'],paging);
 try{if(!scope||!selected())throw Error('Отметьте услуги или группы в списке работ.');const normal=referencePanel(mode==='usual'?'usualPanel':'regularityPanel'),range=mode==='usual'&&!normal?fieldsRange('usualPeriod'):scope,s=scopeFor(range),windowSize=Number(value(mode==='usual'?'usualWindow':'regularityGap'));
  const opts={search:mode==='usual'?'':value('regularitySearch'),status:mode==='usual'?'all':value('regularityStatus')};
  if(normal){
   if(mode==='regularity')opts.baseline={from:scope.baseFrom,to:scope.baseTo};
   else opts.range_window=true;
  }
  const req=request(s,{method:'range',percent:mode==='usual'?number('usualPercent'):0,min_count:0,min_amount:0});
  const page=await api.unifiedPatternPageAtPage(req,mode,windowSize,opts,index);if(!task.valid())return;clear(name+'Results','',false);clear(name+'Coverage','',false);
  $(name+'Coverage').append(UI.el('p',mode==='usual'?'Исторических окон с отчётами: '+page.used+' из '+page.expected+'. '+(page.current_complete?'Анализируемый интервал полный.':'Анализируемый интервал неполон.'):'Периодов с отчётами: '+page.used+' из '+page.expected+'. В базовом интервале: '+page.previous_used+'.'+(page.current_complete?'':' Анализируемый интервал неполон.'),'muted small'));
  if(mode==='usual'){
   const warnings=[];
   if(!page.current_complete)warnings.push('Показатели рассчитаны по доступным отчётам. Пропуски могут влиять на отклонение.');
   if(page.current_amount_incomplete&&(usualMetrics.has('amount')||usualMetrics.has('average')))warnings.push('В некоторых строках сумма не указана. Показана сумма заполненных ячеек.');
   if(page.used<2)warnings.push('Для оценки отклонения нужны минимум два исторических окна с отчётами.');
   if(warnings.length)$(name+'Coverage').append(UI.el('p',warnings.join(' '),'warn small'));
   if(usualMetrics.has('amount')||usualMetrics.has('average'))$(name+'Coverage').append(UI.el('p','Среднее рассчитано по строкам с суммой и сопоставимым количеством. При нулевой базе процент недоступен.','muted small'));
  }else if(normal){
   $(name+'Coverage').append(UI.el('p','Для частоты учитываются календарные периоды, в которых есть отчёты выбранных подразделений в пределах соответствующего диапазона.','muted small'));
  }
  const root=$(name+'Results');if(!page.rows.length)root.append(UI.el('p','Нет услуг по выбранным фильтрам.','muted small'));
  if(mode==='usual')for(const metric of usualMetrics){const rows=page.rows.map(row=>{const m=row[metric];return [row.key?itemButton(row,{...s,from:page.baseline_from,to:s.to}):row.name,fmt(m.base,metric),fmt(m.current,metric),delta(m.delta,metric),percent(m.percent),UI.el('span',usualLabels[m.status]||m.status,m.status==='high'?'numeric-rise':m.status==='low'?'numeric-fall':'')];});UI.table(root,metric,'Среднее: '+sampleCaption(page.baseline_from,page.baseline_to,s.displayType)+'. Анализ: '+sampleCaption(page.current_from,page.current_to,s.displayType),['Услуга / группа','Среднее раньше','Сейчас','Отклонение','Отклонение, %','Оценка'],rows,{
   headerHelp:[
    null,
    'Для суммы и количества / объёма берётся среднее по доступным отчётам в предыдущих окнах. Для суммы и средней стоимости используются окна, в которых все суммы работы известны. Средняя стоимость — суммарная стоимость, делённая на суммарное количество за эти окна.',
    null, null,
    null,
    'Для оценки нужны хотя бы два предыдущих окна с данными по показателю. Отклонение отмечается при достижении порога, а при нулевой базе — при любом ненулевом изменении.'
   ],
   csvContext:fullExport(task,req,'usual',metric,{options:opts,window:windowSize}),csvRows:page.rows.map(row=>{const m=row[metric];return [entryLabel(row),m.base,m.current,m.delta,m.percent,usualLabels[m.status]||m.status];})});}
  else {const raw=page.rows.map(row=>[entryLabel(row),page.used?row.hits:null,row.frequency,page.previous_used?row.previousHits:null,row.previousFrequency,Number.isFinite(row.frequency)&&Number.isFinite(row.previousFrequency)?row.frequency-row.previousFrequency:null,row.first,row.last,row.trailing,(row.statuses||[]).map(s=>regularLabels[s]||s).join('; ')]);const rows=raw.map((row,i)=>[itemButton(page.rows[i],s),row[1]??'—',percent(row[2]),row[3]??'—',percent(row[4]),Number.isFinite(row[5])?fmt(row[5],'count')+' п. п.':'—',row[6]||'—',row[7]||'—',row[8]??'—',row[9]]);UI.table(root,'count',caption(s),['Услуга / группа','Была в периодах','Частота сейчас, %','Была раньше','Частота раньше, %','Разница частоты, п. п.','Впервые в истории','Последний раз','Подряд без активности','Что это значит'],rows,{title:'Как часто выполнялись услуги',totalRow:false,csvRows:raw,
   headerHelp:[
    'Название выбранной работы или группы. Для группы учитывается активность входящих в неё работ.',
    'Количество периодов анализируемого диапазона, в которых загруженные отчёты показывают активность.',
    'Число периодов с активностью / число периодов с доступными отчётами в анализируемом диапазоне × 100%.',
    'Число периодов с активностью в загруженных отчётах базового диапазона.',
    'Число периодов с активностью / число периодов с доступными отчётами в базовом диапазоне × 100%.',
    'Частота сейчас минус частота раньше, в процентных пунктах.',
    'Первый период с активностью в загруженной истории выбранных подразделений по конец анализируемого диапазона.',
    'Последний период с активностью в загруженной истории выбранных подразделений по конец анализируемого диапазона.',
    'Число периодов подряд без активности в доступных отчётах к концу диапазона. Период совсем без отчётов прерывает подсчёт.',
    'Признаки описывают частоту в загруженных отчётах. У одной работы может быть несколько признаков.'
   ],
   csvContext:fullExport(task,req,'regularity','count',{options:opts,window:windowSize})});}
  pager(name,page,index);
 }catch(error){task.error(error);}finally{task.done();}
}
function clearExtended(){for(const name of channels.keys())invalidate(name);for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis','contributionResults','contributionCheck','structureResults','usualResults','usualCoverage','regularityResults','regularityCoverage','changesResults','changesWarnings','changesSummary'])clear(id);$('analyticsDetails').hidden=true;$('changeDetails').hidden=true;}

const localPeriods={
 comparison:['basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd'],
 structure:['structureBase','structureCurrent'],
 changes:['changesFrom','changesTo']
};
function periodExpected(id){
 if(!scope)return null;
 if(['basePeriod','baseRangeStart','structureBase'].includes(id))return scope.baseFrom||previous(scope.from,scope.displayType);
 if(id==='baseRangeEnd')return scope.baseTo||scope.baseFrom||previous(scope.from,scope.displayType);
 if(['currentRangeStart','comparisonRangeStart','changesFrom'].includes(id))return scope.from;
 return scope.to;
}
function updatePeriodOverrides(){
 for(const [name,ids] of Object.entries(localPeriods)){
  const indicator=$(name+'PeriodOverride');if(!indicator)continue;
  const organizations=name==='comparison'&&value('comparisonMode')==='organizations';
  const mode=organizations?'comparisonScope':'periodComparisonScope';
  const activeIds=name!=='comparison'?ids:organizations?
   value(mode)==='range'?['comparisonRangeStart','comparisonRangeEnd']:['comparisonPeriod']:
   value(mode)==='range'?['baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd']:['basePeriod','currentPeriod'];
  const different=Boolean(scope)&&activeIds.some(id=>{
   const control=$(id);
   if(!control?.dataset.unifiedTouched)return false;
   const kind=name==='changes'?changesKind:scope.displayType,expected=periodExpected(id);
   const expectedInput=window.nativePeriodSelection?.fromIsoDate?.(expected,kind)||expected;
   return control.value!==expectedInput;
  });
  indicator.hidden=!different&&!(name==='comparison'&&scope&&$(mode)?.dataset.unifiedTouched&&value(mode)!==(scope.overviewMode==='range'?'range':'single'))&&
    !(name==='changes'&&scope&&(changesKind!==scope.displayType||value('changesMethod')==='range'));
 }
}
function resetLocalPeriod(name){
 if(!scope)return;
 for(const id of localPeriods[name]||[])if($(id)?.dataset)delete $(id).dataset.unifiedTouched;
 if(name==='comparison')for(const id of ['periodComparisonScope','comparisonScope'])if($(id)?.dataset)delete $(id).dataset.unifiedTouched;
 if(name==='changes'){changesKind=scope.displayType;$('changesType').value=changesKind;$('changesMethod').value='previous';}
 resetScope(scope);
}
function resetScope(next){if(!next){scope=null;clearExtended();return;}scope=copyScope(next);for(const name of channels.keys())invalidate(name);$('analyticsDetails').hidden=true;$('changeDetails').hidden=true;
 const force=dateKind!==scope.displayType;dateKind=scope.displayType;
 if(force){for(const ids of Object.values(localPeriods))for(const id of ids)if($(id)?.dataset)delete $(id).dataset.unifiedTouched;for(const id of ['periodComparisonScope','comparisonScope'])if($(id)?.dataset)delete $(id).dataset.unifiedTouched;}
 const before=scope.baseFrom||previous(scope.from,scope.displayType),beforeEnd=scope.baseTo||before;
 for(const id of ['periodComparisonScope','comparisonScope'])if(!$(id)?.dataset.unifiedTouched)$(id).value=scope.overviewMode==='range'?'range':'single';
 for(const id of ['basePeriod','baseRangeStart','structureBase'])setDate(id,scope.displayType,before,force);
 setDate('baseRangeEnd',scope.displayType,beforeEnd,force);
 for(const id of ['currentPeriod','currentRangeEnd','comparisonPeriod','comparisonRangeEnd','structureCurrent','usualPeriod'])setDate(id,scope.displayType,scope.to,force);
 for(const id of ['currentRangeStart','comparisonRangeStart'])setDate(id,scope.displayType,scope.from,force);
 if(!changesKind||force){$('changesType').value=scope.displayType;changesKind=scope.displayType;}
 for(const id of ['changesFrom','changesTo'])setDate(id,changesKind,id==='changesFrom'?scope.from:scope.to);
 const yearOption=[...($('changesType').options||[])].find(option=>option.value==='year');if(scope.source==='daily'&&!yearOption){const option=UI.el('option','Годы');option.value='year';$('changesType').append(option);}if(yearOption)yearOption.disabled=scope.source!=='daily';if(scope.source!=='daily'&&changesKind==='year'){changesKind=scope.displayType;$('changesType').value=changesKind;for(const id of ['changesFrom','changesTo'])setDate(id,changesKind,scope.to,true);}
 const orgNames=UI.snapshot?.org_names||scope.organizations;if(!changeOrganizations.size)changeOrganizations=new Set(orgNames);
 changeOrganizations=new Set([...changeOrganizations].filter(org=>orgNames.includes(org)));
 checkboxList('changesOrganizations',orgNames.map(o=>[o,o]),changeOrganizations,()=>changes());
 const oldBase=value('comparisonBaseOrg');$('comparisonBaseOrg').replaceChildren(...scope.organizations.map(org=>{const option=UI.el('option',org);option.value=org;return option;}));if(scope.organizations.includes(oldBase))$('comparisonBaseOrg').value=oldBase;
 if(!customComparisonMetrics)comparisonMetrics=new Set(metrics());checkboxList('comparisonMetrics',Object.entries(titles),comparisonMetrics,()=>{customComparisonMetrics=true;comparison();});
 updatePeriodOverrides();
 if(activeView==='changes')changes();
 if($('advancedAnalytics').open){
  if(activeView==='dashboard'){comparison();if(compositionVisible())structure();}
  if(referencePanel('usualPanel'))patterns('usual');
  if(referencePanel('regularityPanel'))patterns('regularity');
 }
}
function bind(ids,event,handler){for(const id of ids)$(id)?.addEventListener(event,handler);}

function compositionVisible(){return value('workViewMode')==='composition'&&!$('structurePanel').hidden&&(!window.referenceDetail?.active||referencePanel('structurePanel'));}
function workView(){
 const mode=value('workViewMode')==='composition'?'composition':'trend',showComposition=mode==='composition';
 const chart=$('chartPanel'),composition=$('structurePanel');
 if(chart)chart.hidden=showComposition;
 if(composition)composition.hidden=!showComposition;
 const help=$('workViewDescription');
 if(help)help.textContent=showComposition?
  'Рейтинг по доле работ и сравнению с базовым периодом; расчёт выполняется отдельно в SQLite.':
  'Значения по периодам. Выбранные работы не влияют на итог по всем работам без явного фильтра.';
 if(showComposition&&compositionVisible()&&scope&&activeView==='dashboard')void structure();
 else if(!showComposition)invalidate('structure');
}
function init(){if(initialized)return;initialized=true;explainSensitivity();describeActiveFilters();
 $('workViewMode')?.addEventListener('change',workView);
 workView();
 const dateIds=['changesFrom','changesTo','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','structureBase','structureCurrent','usualPeriod'];for(const id of dateIds){for(const event of ['input','change'])$(id)?.addEventListener(event,()=>{$(id).dataset.unifiedTouched='true';updatePeriodOverrides();});}
 for(const id of ['periodComparisonScope','comparisonScope'])$(id)?.addEventListener('change',()=>{$(id).dataset.unifiedTouched='true';updatePeriodOverrides();});
 $('comparisonMode').addEventListener('change',updatePeriodOverrides);
 for(const name of Object.keys(localPeriods))$(name+'PeriodReset')?.addEventListener('click',()=>resetLocalPeriod(name));
 $('changesType')?.addEventListener('change',updatePeriodOverrides);
 $('changesMethod')?.addEventListener('change',updatePeriodOverrides);
 checkboxList('changesMetrics',[['count','Количество / объём'],['amount','Сумма']],changeMetrics,()=>changes());
 const metricOptions=Object.entries(titles);
 const updateMetricCount=()=>{
  if(!window.unifiedOriginal?.renderMultiFilter)return;
  const label=$('usualMetrics').querySelector('.multi-selected-label');
  label.textContent='Выбрано '+usualMetrics.size+' из '+metricOptions.length;
 };
 checkboxList('usualMetrics',metricOptions,usualMetrics,()=>{updateMetricCount();patterns('usual');});
 updateMetricCount();
 bind(['changesMethod','changesFrom','changesTo','changesMode','changesDirection','changesSort'],'change',()=>{invalidate('changes');changes();});
 bind(['changesPercent','changesCountMin','changesAmountMin','changesSearch'],'input',()=>{explainSensitivity();describeActiveFilters();schedule('changes',changes);});
 $('changesType').addEventListener('change',()=>{changesKind=value('changesType');for(const id of ['changesFrom','changesTo'])setDate(id,changesKind,scope?.to||new Date().toISOString().slice(0,10),true);invalidate('changes');changes();});
 for(const name of ['changes','contribution','usual','regularity']){const run=name==='changes'?changes:name==='contribution'?contributions:index=>patterns(name,index);$(name+'Prev').addEventListener('click',()=>{const state=slot(name);if(state.index>0)return run(state.index-1);});$(name+'Next').addEventListener('click',()=>{const state=slot(name);if((state.index+1)*RESULTS_PAGE_SIZE<(state.page?.total_rows||0))return run(state.index+1);});}
 bind(['comparisonMode','periodComparisonScope','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonScope','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','comparisonBaseOrg'],'change',()=>{invalidate('comparison');invalidate('contribution');comparison();});
 bind(['structureBase','structureCurrent','structureMetric','structureLimit','structureScope'],'change',structure);bind(['usualPeriod','usualWindow'],'change',()=>patterns('usual'));bind(['usualPercent'],'input',()=>schedule('usual',()=>patterns('usual')));bind(['regularityGap','regularityStatus'],'change',()=>patterns('regularity'));bind(['regularitySearch'],'input',()=>schedule('regularity',()=>patterns('regularity')));
 $('advancedAnalytics').addEventListener('toggle',()=>{
  if(!$('advancedAnalytics').open||!scope)return;
  comparison();if(!window.referenceDetail?.active&&compositionVisible())structure();
 });
 $('closeAnalyticsDetails').addEventListener('click',()=>closeHistory('analytics'));$('closeChangeDetails').addEventListener('click',()=>closeHistory('change'));
 $('overviewShowComparison').addEventListener('click',()=>{
  $('comparisonMode').value='periods';
  if(referencePanel('overviewPanel'))return window.referenceDesktop.openPanel('comparison',$('overviewShowComparison'));
  $('advancedAnalytics').open=true;comparison();$('comparison').scrollIntoView?.({block:'start'});
 });
 UI.onScope(resetScope);UI.onSnapshot(snapshot=>{if(!snapshot){scope=null;clearExtended();}});
 UI.onView('changes',()=>{activeView='changes';if(scope)changes();});UI.onView('dashboard',()=>{activeView='dashboard';if(scope?.revision===UI.revision&&$('advancedAnalytics').open){comparison();if(compositionVisible())structure();}});for(const view of ['upload','groups','storage'])UI.onView(view,()=>{activeView=view;});
}
function leaveDetail(){
 for(const key of ['analyticsHistory','comparison','contribution','structure','usual','regularity'])if(channels.has(key))invalidate(key);
 historyTriggers.delete('analytics');$('analyticsDetails').hidden=true;
}
window.unifiedExtended=Object.freeze({init,openComparison:comparison,openPatterns:mode=>scope&&patterns(mode),leaveDetail});
})();
