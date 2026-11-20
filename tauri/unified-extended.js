// Original analytics controls, backed exclusively by bounded SQLite requests.
(function(){
'use strict';
const UI=window.unifiedUI,$=UI.$,api=UI.api;
const titles={count:'Количество / объём',amount:'Сумма',average:'Сумма / количество (объём)'};
const usualLabels={normal:'Обычный уровень',high:'Необычно выше',low:'Необычно ниже',insufficient:'Недостаточно данных'};
const regularLabels={frequent:'Часто',rare:'Редко',rarer:'Заметно реже',disappeared:'Перестала встречаться',new:'Впервые в загруженной истории',dormant:'Долго без выполнений',occasional:'Встречается',absent:'Нет выполнений',uncertain:'Есть пропуски',unknown:'Нет сопоставимых отчётов'};
const channels=new Map();
let initialized=false,scope=null,activeView='dashboard',dateKind=null,changesKind=null;
let changeOrganizations=new Set(),changeMetrics=new Set(['count','amount']),usualMetrics=new Set(['count','amount','average']),comparisonMetrics=new Set(['amount']);
let customComparisonMetrics=false;
function slot(name){if(!channels.has(name))channels.set(name,{ticket:0,positions:[null],page:null,captured:null,timer:null});return channels.get(name);}
function clear(id,text=''){const node=$(id);if(node){node.replaceChildren();if(text)node.append(UI.el('p',text,'muted small'));}}
function value(id){return $(id)?.value||'';}
 function orientation(){return value('chartOrientation')==='horizontal'?'horizontal':'vertical';}
function selected(){return Boolean(scope&&(scope.allNames===true||scope.selectedNames?.length));}
function metrics(){return [...(UI.state?.selectedMetrics||['count','amount'])].filter(m=>titles[m]);}
function fmt(n,m='count'){return UI.fmt(n,m);}
function percent(n){return Number.isFinite(n)?n.toLocaleString('ru-RU',{maximumFractionDigits:1})+'%':'—';}
function delta(n,m){return Number.isFinite(n)?(n>0?'+':'')+fmt(n,m):'—';}
function rawDifference(now,base){return [Number.isFinite(now)&&Number.isFinite(base)?now-base:null,Number.isFinite(now)&&Number.isFinite(base)&&base!==0?(now-base)/Math.abs(base)*100:null];}
function difference(now,base,m){return [Number.isFinite(now)&&Number.isFinite(base)?delta(now-base,m):'—',Number.isFinite(now)&&Number.isFinite(base)&&base!==0?percent((now-base)/Math.abs(base)*100):'—'];}
function number(id){const raw=value(id).trim(),n=Number(raw);if(!raw||!Number.isFinite(n)||n<0)throw Error('Укажите неотрицательные числовые пороги.');return n;}
function textError(e){return window.analizUserError?window.analizUserError(e):(e?.message||String(e));}
function current(s,t,revision){return s.ticket===t&&UI.revision===revision;}
function invalidate(name){const s=slot(name);for(const id of s.ids||[])$(id)?.setAttribute('aria-busy','false');s.ticket++;s.page=null;s.captured=null;s.positions=[null];if(s.timer){clearTimeout(s.timer);s.timer=null;}return s;}
function pager(name,result,positions,next){
 const s=slot(name);s.page=result;s.positions=positions;
 const previous=$(name+'Prev'),forward=$(name+'Next'),label=$(name+'Page');
 if(previous)previous.disabled=positions.length<=1;
 if(forward)forward.disabled=!next;
 if(label)label.textContent='Страница '+positions.length+' · '+(result.total_rows??result.summary?.rows??result.rows?.length??0)+' строк';
}
function loading(name,ids){const s=slot(name),ticket=++s.ticket,revision=UI.revision;s.page=null;s.captured=null;s.ids=ids;if($(name+'Page'))$(name+'Page').textContent='Загрузка…';
 for(const id of ids){clear(id,'Загрузка…');$(id)?.setAttribute('aria-busy','true');}
 for(const suffix of ['Prev','Next'])if($(name+suffix))$(name+suffix).disabled=true;
 return {s,ticket,revision,valid:()=>current(s,ticket,revision),done(){if(current(s,ticket,revision))for(const id of ids)$(id)?.setAttribute('aria-busy','false');},error(error){if(current(s,ticket,revision)){for(const id of ids)clear(id,'Не удалось загрузить данные. '+textError(error));}}};
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
function dateLabel(iso){return /^\d{4}-\d{2}-\d{2}$/.test(iso)?iso.slice(8,10)+'.'+iso.slice(5,7)+'.'+iso.slice(0,4):iso;}
function visibleEnd(range){return range.source==='daily'?range.to:periodEnd(range.to,range.displayType);}
function caption(range){const end=visibleEnd(range);return dateLabel(range.from)+(end!==range.from?' — '+dateLabel(end):'');}
function periodLabel(start,kind){return dateLabel(start)+(kind==='week'?' — '+dateLabel(periodEnd(start,'week')):'');}
function workLabel(name){return String(name).trim().replace(/,\s*$/,'');}
function itemButton(entry,s,organization=null,target='analytics'){const button=UI.el('button',workLabel(entry.name)+(entry.kind==='group'?' (группа)':''),'service-link text-button');button.type='button';button.addEventListener('click',()=>history(entry,s,organization,target));return button;}
function plotTimeline(result,entry,s,root,chosenMetrics,seriesType){
 const points=result.points||[],kind=result.granularity==='period'?s.displayType:result.granularity;
 const periods=points.map(p=>{const start=p.start.length===7?p.start+'-01':p.start;return {type:kind,start,end:periodEnd(start,kind)};});
 const data=points.map(p=>p.missing===0?new Map([[entry.key,p.total]]):null);
 for(const metric of chosenMetrics){const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');section.append(UI.el('h3',titles[metric]),plot);root.append(section);UI.drawChart(periods,new Set(s.organizations),metric,plot,{chart:'bar',orientation:orientation(),categories:[entry.key],data,label:()=>entry.name,seriesType});}
}
async function history(entry,s,organization,target){
 if(!entry.key||entry.other)return;
 const prefix=target==='change'?'change':'analytics',panel=$(prefix==='change'?'changeDetails':'analyticsDetails'),title=$(prefix==='change'?'changeDetailTitle':'analyticsDetailTitle'),content=$(prefix==='change'?'changeDetailContent':'analyticsDetailContent');
 if(!panel||!content)return;panel.hidden=false;title.textContent=entry.name+(organization?' · '+organization:'');
 const task=loading(prefix+'History',[content.id]);
 try{const result=await api.nativeWorkTimeline(s.revision,s.type,s.from,s.to,s.organizations,organization,entry.key,null,s.source==='daily'?s.displayType:null,s.selectedNames||[]);if(!task.valid())return;content.replaceChildren();content.append(UI.el('p','Динамика за '+caption(s)+'. Пропуски не считаются нулём. Данные представлены по периодам.','muted small'));plotTimeline(result,entry,s,content,target==='change'?[...changeMetrics]:metrics());title.focus?.({preventScroll:true});panel.scrollIntoView?.({block:'nearest'});}
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
 const labels={up:'рост',down:'снижение',new:'новые',gone:'нет выполнений',small:'малая база',negative:'отрицательные',unchanged:'без изменений'};
 const tags=[],direction=value('changesDirection'),search=value('changesSearch').trim();
 if(labels[direction])tags.push('фильтр: '+labels[direction]);
 if(value('changesMode')==='strong')tags.push('только заметные');
 if(search)tags.push('поиск: '+search.slice(0,30));
 node.textContent=tags.length?' · '+tags.join(' · '):'';
}
function changeLayout(){explainSensitivity();describeActiveFilters();const single=value('changesMethod')==='previous';$('changesFromLabel').hidden=single;$('changeDatePair').classList.toggle('single',single);$('changesToText').textContent=single?'Текущий период':'Конец диапазона';
 const search=Boolean(value('changesSearch').trim());$('changesSort').disabled=search;$('changesSort').title=search?'Сначала начала названий, затем совпадения внутри, по алфавиту.':'';
 for(const option of $('changesSort').options||[])option.disabled=option.value.includes('-')&&!changeMetrics.has(option.value.split('-')[1]);
 if($('changesSort').selectedOptions?.[0]?.disabled&&changeMetrics.size)$('changesSort').value='significance-'+[...changeMetrics][0];
}
function changeContext(){
 if(!scope||!changeOrganizations.size||!changeMetrics.size)throw Error('Выберите подразделения и хотя бы один показатель.');
 const kind=value('changesType'),range=fieldsRange(value('changesMethod')==='previous'?'changesTo':'changesFrom','changesTo',kind,scope.source);
 const s=scopeFor(range,{organizations:[...changeOrganizations],selectedNames:[],selectedKeys:[],allNames:true});
 return {scope:s,request:request(s,{method:value('changesMethod'),percent:number('changesPercent'),min_count:number('changesCountMin'),min_amount:number('changesAmountMin')}),options:{metrics:[...changeMetrics],mode:value('changesMode'),search:value('changesSearch'),direction:value('changesDirection'),sort:value('changesSort')}};
}
function changeMeasure(row,metric){const m=row[metric]||{};return {...m,smallBase:m.smallBase??m.small_base??false};}
function changeLabel(row,m){if(m.negative)return 'Изменение с отрицательными значениями';if(row.status==='new')return 'Новая';if(row.status==='gone')return 'Нет выполнений';return m.delta>0?'Рост':m.delta<0?'Снижение':'Без изменений';}
function renderChangeSummary(summary,chosen){clear('changesSummary');const root=$('changesSummary');
 for(const [label,n,cls,direction] of [['Значительно выросло',summary.grown,'numeric-rise','up'],['Значительно снизилось',summary.fallen,'numeric-fall','down'],['Новых услуг',summary.new,'','new'],['Нет выполнений',summary.gone,'','gone']]){
  const card=UI.el('button',null,'kpi kpi-action');card.type='button';
  const explanation=direction==='up'||direction==='down'
   ?'Изменение не меньше '+value('changesPercent')+'% и минимальной разницы: '+value('changesCountMin')+' по количеству / объёму или '+value('changesAmountMin')+' по сумме. Без отрицательных значений и малой базы.'
   :direction==='new'?'Работа появилась в загруженном периоде и отсутствовала в базовом.':
    'В базовом периоде работа выполнялась, в текущем выполнений нет. Пропуски не считаются нулём.';
  card.title=explanation;card.setAttribute('aria-label',label+': '+n+'. '+explanation+' Показать строки этой категории.');
  card.append(UI.el('span',label),UI.el('strong',String(n),n?cls:''));
  card.addEventListener('click',()=>{$('changesDirection').value=direction;invalidate('changes');void changes();$('changesResults').scrollIntoView?.({block:'start'});});
  root.append(card);
 }
 for(const metric of chosen){const top=summary['top_'+metric],m=top&&changeMeasure(top,metric),card=UI.el('div',null,'kpi');card.append(UI.el('span','Наибольшее абсолютное изменение · '+titles[metric]),UI.el('strong',m?delta(m.delta,metric):'—',m?.delta>0?'numeric-rise':m?.delta<0?'numeric-fall':''));if(top)card.append(UI.el('small',workLabel(top.name)+' · '+top.org+' · '+periodLabel(top.baseStart,value('changesType'))+' / '+periodLabel(top.currentStart,value('changesType'))));root.append(card);}
}
async function changes(positions=[null]){
 changeLayout();const task=loading('changes',['changesResults','changesSummary','changesWarnings']);$('changeDetails').hidden=true;
 try{const captured=changeContext(),result=await api.unifiedChangesPage(captured.request,captured.options,positions.at(-1));if(!task.valid())return;
  clear('changesResults');clear('changesWarnings');renderChangeSummary(result.summary,captured.options.metrics);
  const diagnostic=result.summary.totals;
  if(diagnostic?.missing_pairs||diagnostic?.invalid_reports||diagnostic?.duplicate_issues||diagnostic?.overlap_issues||diagnostic?.import_warning_pairs){
   const problems=[];
   if(diagnostic.missing_pairs>0)problems.push('Нет полной пары отчётов: '+diagnostic.missing_pairs+' (не включены в сопоставление).');
   if(diagnostic.import_warning_pairs>0)problems.push('Пар с замечаниями импорта: '+diagnostic.import_warning_pairs+' (проверьте источник; замечание не означает автоматического исключения).');
   if(diagnostic.invalid_reports>0)problems.push('Некорректных отчётов: '+diagnostic.invalid_reports+'.');
   if(diagnostic.duplicate_issues>0)problems.push('Повторных отчётов: '+diagnostic.duplicate_issues+'.');
   if(diagnostic.overlap_issues>0)problems.push('Пересекающихся отчётов: '+diagnostic.overlap_issues+'.');
   const total=Number.isSafeInteger(result.total_rows)?result.total_rows:result.summary.rows;
   $('changesWarnings').append(UI.el('p','Сопоставимых строк по текущим фильтрам: '+total+'. '+problems.join(' ')+' Пропуски не считаются нулём.','warn small'));
  }
  for(const pair of result.missing_examples||[])$('changesWarnings').append(UI.el('p',pair.org+': нет полного отчёта за '+[pair.missing_base?pair.base_start:null,pair.missing_current?pair.current_start:null].filter(Boolean).map(date=>periodLabel(date,captured.scope.displayType)).join(', '),'warn small'));
  if(!result.rows.length)clear('changesResults','Нет сопоставимых записей по выбранным условиям. Измените фильтры или проверьте наличие соседних отчётов.');
  const rows=[],raw=[];
  for(const row of result.rows)for(const metric of captured.options.metrics){const m=changeMeasure(row,metric),name=itemButton(row,{...captured.scope,from:row.baseStart,to:captured.scope.source==='daily'?periodEnd(row.currentStart,captured.scope.displayType):row.currentStart},row.org,'change');const direction=changeLabel(row,m),kind=m.smallBase?' · малая база':'';
   rows.push([name,row.org,periodLabel(row.baseStart,captured.scope.displayType),periodLabel(row.currentStart,captured.scope.displayType),titles[metric],fmt(m.base,metric),fmt(m.current,metric),delta(m.delta,metric),percent(m.percent),UI.el('span',direction+kind,m.delta>0?'numeric-rise':m.delta<0?'numeric-fall':'')]);raw.push([row.name,row.org,row.baseStart,row.currentStart,titles[metric],m.base,m.current,m.delta,m.percent,direction+kind]);}
  if(rows.length)UI.table($('changesResults'),'count',caption(captured.scope),['Работа / группа','Подразделение','Базовый период','Текущий период','Показатель','Было','Стало','Изменение','Изменение, %','Направление'],rows,{title:'Анализ изменений',totalRow:false,csvRows:raw,csvContext:{text:'Все строки по текущим фильтрам',export:async()=>{
   if(!task.valid())throw Error('Параметры анализа изменились. Рассчитайте заново.');const path=await api.chooseCsvPath('izmeneniya-'+captured.scope.from+'-'+captured.scope.to+'.csv');if(!path)return;if(!task.valid())throw Error('Данные изменились. Рассчитайте заново.');await api.unifiedChangesCsv(captured.request,captured.options,path);
  }}});
  task.s.captured=captured;pager('changes',result,positions,result.next);
 }catch(error){task.error(error);}finally{task.done();}
}
function comparisonRanges(){const ranges=value('periodComparisonScope')==='range';return {before:fieldsRange(ranges?'baseRangeStart':'basePeriod',ranges?'baseRangeEnd':'basePeriod'),after:fieldsRange(ranges?'currentRangeStart':'currentPeriod',ranges?'currentRangeEnd':'currentPeriod')};}
function comparisonLayout(){const organizations=value('comparisonMode')==='organizations',ranges=value('periodComparisonScope')==='range',orgRange=value('comparisonScope')==='range';
 $('periodComparisonFilters').hidden=organizations;$('orgComparisonFilters').hidden=!organizations;$('kpis').hidden=organizations;$('comparisonCharts').hidden=!organizations;$('comparisonDynamics').hidden=!organizations;$('contributions').hidden=organizations;
 $('periodComparisonSingle').hidden=ranges;$('periodComparisonRanges').hidden=!ranges;$('comparisonPeriodLabel').hidden=orgRange;$('comparisonRangeGroup').hidden=!orgRange;
}
function addBoundedPager(root,s,rows,run){const bar=UI.el('div',null,'change-pagination'),prev=UI.el('button','← Предыдущая','button secondary'),next=UI.el('button','Следующая →','button secondary');prev.type=next.type='button';prev.disabled=s.positions.length<=1;next.disabled=rows.length<50;prev.addEventListener('click',()=>run(s.positions.slice(0,-1)));next.addEventListener('click',()=>run([...s.positions,rows.at(-1).key]));bar.append(prev,UI.el('span','Страница '+s.positions.length,'muted small'),next);root.append(bar);}
async function comparison(positions=[null]){
 comparisonLayout();if(!scope)return;
 const task=loading('comparison',['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis']);$('analyticsDetails').hidden=true;
 try{if(!selected())throw Error('Отметьте работы или группы для сравнения.');
  if(value('comparisonMode')==='organizations'){await departmentComparison(task,positions);return;}
  const {before,after}=comparisonRanges(),s=scopeFor(after),req=request(s,{percent:0,min_count:0,min_amount:0});
  const [dashboard,page]=await Promise.all([api.nativeAnalysisCompare(s.revision,s.type,before.from,before.to,after.from,after.to,s.organizations,null,s.selectedNames),api.unifiedComparisonPage(req,{from:before.from,to:before.to},null,positions.at(-1),50)]);
  if(!task.valid())return;for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis'])clear(id);
  const summary=dashboard.summary;
  $('comparisonHelp').textContent='Второй интервал сравнивается с первым по отмеченным работам. Прочерк — неполные данные; при нулевой базе процент не рассчитывается.';
  for(const [label,total] of [['Первый интервал',summary.before],['Второй интервал',summary.after]])$('comparisonCoverage').append(UI.el('p',label+': '+(total.complete?'Полные данные':'Неполные данные')+' · периодов '+total.periods+', пропусков: '+total.missing+' (подразделение × период)',total.complete?'muted small':'warn small'));
  const beforeComplete=summary.before.complete,afterComplete=summary.after.complete;
  if(!beforeComplete||!afterComplete){
   $('comparisonCoverage').append(UI.el('p',
    'Сравнение недоступно: '+(!beforeComplete?'не хватает отчётов базового периода':'не хватает отчётов анализируемого периода')+
    '. Показаны только доступные показатели; прочерк не означает ноль.','warn small'));
  }
  for(const metric of metrics()){
   const valueNow=summary.after.total[metric],valueBefore=summary.before.total[metric],card=UI.el('div',null,'kpi');
   card.append(UI.el('span',titles[metric]+' · анализируемый период'),UI.el('strong',fmt(valueNow,metric)),
    UI.el('small',beforeComplete&&afterComplete?difference(valueNow,valueBefore,metric).join(' · '):'Неполные данные'));
   $('kpis').append(card);
   if(!beforeComplete||!afterComplete){
    const showAfter=afterComplete,label=showAfter?'Значение сейчас':'Значение раньше';
    const total=showAfter?valueNow:valueBefore;
    const visible=page.filter(entry=>Number.isFinite((entry[metric]||{})[showAfter?'current':'base']));
    if(!Number.isFinite(total)&&!visible.length)continue;
    const raw=[['Итого по выбранным работам',total]],rows=[['Итого по выбранным работам',fmt(total,metric)]];
    for(const entry of visible){const value=(entry[metric]||{})[showAfter?'current':'base'];
     rows.push([itemButton(entry,s),fmt(value,metric)]);raw.push([entry.name,value]);}
    UI.table($('comparisonTable'),metric,caption(showAfter?after:before),
     ['Работа / группа',label],rows,{csvRows:raw});
    continue;
   }
   const raw=[['Итого по выбранным работам',valueBefore,valueNow,...rawDifference(valueNow,valueBefore)]];
   const rows=[['Итого по выбранным работам',fmt(valueBefore,metric),fmt(valueNow,metric),...difference(valueNow,valueBefore,metric)]];
   for(const entry of page){
    const m=entry[metric]||{};
    if(!Number.isFinite(m.base)&&!Number.isFinite(m.current))continue;
    rows.push([itemButton(entry,{...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to}),
     fmt(m.base,metric),fmt(m.current,metric),delta(m.delta,metric),percent(m.percent)]);
    raw.push([entry.name,m.base,m.current,m.delta,m.percent]);
   }
   UI.table($('comparisonTable'),metric,caption(before)+' / '+caption(after),
    ['Работа / группа','Значение раньше','Значение сейчас','Разница','Изменение, %'],rows,{csvRows:raw});
  }
  task.s.positions=positions;addBoundedPager($('comparisonTable'),task.s,page,comparison);
  if($('contributions').open)contributions();
 }catch(error){task.error(error);}finally{task.done();}
}
async function departmentComparison(task,positions){
 const range=value('comparisonScope')==='range'?fieldsRange('comparisonRangeStart','comparisonRangeEnd'):fieldsRange('comparisonPeriod'),s=scopeFor(range),chosen=[...comparisonMetrics],baseOrg=value('comparisonBaseOrg');
 const organizations=await api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,s.selectedNames);
 if(!task.valid())return;for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis'])clear(id);
 $('comparisonHelp').textContent='Общий интервал для подразделений; разница относительно '+baseOrg+'. При нулевой базе процент не рассчитывается.';
 for(const org of organizations)$('comparisonCoverage').append(UI.el('p',org.org+': '+(org.complete?'Полные данные':'Неполные данные')+' · пропусков '+org.missing,org.complete?'muted small':'warn small'));
 // Bounded selected-name set: each SQL query aggregates one actual service;
 // no full-report loading and no inference from an absent page row.
 const namePage=s.allNames===true?await api.unifiedNameOptions(s.revision,'',positions.at(-1),50):null;
 const names=namePage?namePage.rows:(s.nameRows||[]).filter(row=>s.selectedKeys.includes(row.key)).slice(0,50);
 if(!task.valid())return;
 const perName=[];for(let i=0;i<names.length;i+=4){const batch=await Promise.all(names.slice(i,i+4).map(async entry=>({entry,rows:await api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,[entry.name])})));if(!task.valid())return;perName.push(...batch);}
 if(!task.valid())return;
 for(const metric of chosen){const raw=[],rows=[];
  const make=(label,values)=>[label,...s.organizations.flatMap(org=>{const n=values.find(o=>o.org===org)?.total?.[metric];return org===baseOrg?[fmt(n,metric)]:[fmt(n,metric),...difference(n,values.find(o=>o.org===baseOrg)?.total?.[metric],metric)];})];
  const makeRaw=(label,values)=>{const base=values.find(o=>o.org===baseOrg)?.total?.[metric];return [label,...s.organizations.flatMap(org=>{const n=values.find(o=>o.org===org)?.total?.[metric]??null;return org===baseOrg?[n]:[n,...rawDifference(n,base)];})];};
  rows.push(make('Итого по выбранным работам',organizations));raw.push(makeRaw('Итого по выбранным работам',organizations));
  for(const item of perName){rows.push(make(itemButton(item.entry,s),item.rows));raw.push(makeRaw(item.entry.name,item.rows));}
  UI.table($('comparisonTable'),metric,caption(range),['Работа / группа',...s.organizations.flatMap(org=>org===baseOrg?[org]:[org,org+' · разница',org+' · %'])],rows,{csvRows:raw});
  const plot=UI.el('div',null,'metric-plot');$('comparisonCharts').append(plot);
  UI.drawChart([{type:s.displayType,start:s.from,end:visibleEnd(s)}],new Set(s.organizations),metric,plot,{chart:'bar',seriesType:'organizations',categories:s.organizations,data:[new Map(organizations.map(o=>[o.org,o.total]))]});
 }
 if(namePage){const state=task.s;state.positions=positions;const bar=UI.el('div',null,'change-pagination'),prev=UI.el('button','← Предыдущая','button secondary'),next=UI.el('button','Следующая →','button secondary');prev.type=next.type='button';prev.disabled=positions.length<=1;next.disabled=!namePage.next;prev.addEventListener('click',()=>comparison(positions.slice(0,-1)));next.addEventListener('click',()=>comparison([...positions,namePage.next]));bar.append(prev,UI.el('span','Страница '+positions.length+' · '+namePage.total_rows+' работ','muted small'),next);$('comparisonTable').append(bar);}
 const trends=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations,null,s.source==='daily'?s.displayType:null,s.selectedNames);
 if(!task.valid())return;
 const first=trends.series?.[0]?.points||[],kind=trends.granularity==='period'?s.displayType:trends.granularity,periods=first.map(p=>({type:kind,start:p.start.length===7?p.start+'-01':p.start,end:periodEnd(p.start.length===7?p.start+'-01':p.start,kind)}));
 const data=first.map((p,i)=>new Map(trends.series.map(series=>[series.organization,series.points[i]?.missing===0?series.points[i].total:{count:null,amount:null,average:null}])));
 for(const metric of chosen){const plot=UI.el('div',null,'metric-plot');$('comparisonDynamicsPlots').append(plot);UI.drawChart(periods,new Set(s.organizations),metric,plot,{chart:'bar',orientation:orientation(),seriesType:'organizations',categories:s.organizations,data});}
}
async function contributions(positions=[null]){
 const task=loading('contribution',['contributionResults','contributionCheck']);
 try{if(!scope)throw Error('Выберите периоды.');const {before,after}=comparisonRanges(),s=scopeFor(after,{selectedNames:[],allNames:true}),chosen=metrics().filter(m=>m!=='average');
  const result=await Promise.all(chosen.map(metric=>positions.at(-1)?.[metric]===false?null:api.nativeCompositionPage(request(s,{percent:0,min_count:0,min_amount:0}),{from:before.from,to:before.to},'contributions',metric,positions.at(-1)?.[metric]||null)));
  if(!task.valid())return;clear('contributionResults');clear('contributionCheck');const next={};
  result.forEach((page,i)=>{const metric=chosen[i];if(!page){next[metric]=false;return;}if(!page.baseComplete||!page.currentComplete){$('contributionCheck').append(UI.el('p','Нет полного набора отчётов для обоих интервалов. Вклад не рассчитывается.','warn small'));return;}
   const check=page.check;$('contributionCheck').append(UI.el('p',titles[metric]+': сумма вкладов '+fmt(check.sum,metric)+'; общее изменение '+fmt(check.delta,metric)+' — '+(check.reconciled?'совпадает':'расхождение '+fmt(check.residual,metric)),check.reconciled?'positive small':'negative small'));
   const rows=page.rows.map(row=>[itemButton(row,{...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to}),fmt(row.base,metric),fmt(row.current,metric),delta(row.delta,metric),percent(row.percent),percent(row.contribution)]);
   UI.table($('contributionResults'),metric,caption(before)+' / '+caption(after),['Услуга / группа','Значение раньше','Значение сейчас','Изменение значения','Изменение, %','Вклад в общее изменение, %'],rows,{totalRow:false,csvRows:page.rows.map(r=>[r.name,r.base,r.current,r.delta,r.percent,r.contribution])});next[metric]=page.next||false;
  });
  const total=Math.max(0,...result.filter(Boolean).map(p=>p.total_rows)),cursor=Object.values(next).some(Boolean)?next:null;pager('contribution',{total_rows:total,next:cursor},positions,cursor);
 }catch(error){task.error(error);}finally{task.done();}
}
async function structure(){
 const task=loading('structure',['structureResults']);
 try{if(!scope)throw Error('Выберите период.');if(value('structureScope')!=='all'&&!selected())throw Error('Отметьте работы или выберите все выполненные работы.');
  const before=fieldsRange('structureBase'),after=fieldsRange('structureCurrent'),metric=value('structureMetric'),s=scopeFor(after,value('structureScope')==='all'?{selectedNames:[],allNames:true}:{});
  const page=await api.nativeCompositionPage(request(s,{percent:0,min_count:0,min_amount:0}),{from:before.from,to:before.to},'structure',metric,null,Number(value('structureLimit')));
  if(!task.valid())return;clear('structureResults');const root=$('structureResults');
  if(!page.currentComplete){clear('structureResults','Нет полного отчёта анализируемого периода. Доли не рассчитываются.');return;}
  root.append(UI.el('p','Итог: '+fmt(page.total,metric),'small'));if(!page.sharesValid)root.append(UI.el('p','Есть отрицательные значения или неположительный итог: доли не рассчитываются.','warn small'));if(!page.baseComplete)root.append(UI.el('p','Базовый период неполон: изменение долей недоступно.','warn small'));
  const visible=page.rows.filter(row=>Number.isFinite(row.current)&&row.current!==0);
  const selectedLimit=Number(value('structureLimit'));
  if(visible.length<selectedLimit)root.append(UI.el('p','Показано '+visible.length+' из '+selectedLimit+': у остальных нет ненулевых значений за этот период.','muted small'));
  if(!visible.length){root.append(UI.el('p','Нет ненулевых значений за выбранный период. Нулевые строки рейтинга скрыты.','muted small'));return;}
  const baselineValid=page.baseComplete;
  const label=row=>row.other?row.name:itemButton(row,{...s,from:before.from<after.from?before.from:after.from,to:before.to>after.to?before.to:after.to});
  const raw=visible.map(row=>baselineValid?
   [row.name,row.base,row.current,row.baseShare,row.share,row.cumulative,row.shareChange]:
   [row.name,row.current,row.share,row.cumulative]);
  const rows=visible.map(row=>baselineValid?
   [label(row),fmt(row.base,metric),fmt(row.current,metric),percent(row.baseShare),percent(row.share),percent(row.cumulative),Number.isFinite(row.shareChange)?fmt(row.shareChange,'count')+' п. п.':'—']:
   [label(row),fmt(row.current,metric),percent(row.share),percent(row.cumulative)]);
  const headers=baselineValid?['Услуга / группа','Значение раньше','Значение сейчас','Доля раньше','Доля сейчас','Накопленная доля','Изменение доли']:
   ['Услуга / группа','Значение сейчас','Доля сейчас','Накопленная доля'];
  UI.table(root,metric,caption(before)+' / '+caption(after),headers,rows,{totalRow:false,csvRows:raw});
  const plot=UI.el('div',null,'metric-plot');root.append(plot);
  const key=row=>row.other?'others':row.key;
  UI.drawChart([{type:s.displayType,start:after.from,end:visibleEnd(after)}],new Set(s.organizations),metric,plot,
   {chart:'bar',orientation:orientation(),categories:visible.map(key),limit:visible.length,
    data:[new Map(visible.map(r=>[key(r),{[metric]:r.current}]))],label:k=>workLabel(visible.find(r=>key(r)===k).name)});
 }catch(error){task.error(error);}finally{task.done();}
}
async function patterns(mode,positions=[null]){
 const name=mode==='usual'?'usual':'regularity',task=loading(name,[name+'Results',name+'Coverage']);
 try{if(!scope||!selected())throw Error('Отметьте услуги или группы в списке работ.');const range=mode==='usual'?fieldsRange('usualPeriod'):{type:scope.type,displayType:scope.displayType,source:scope.source,from:scope.from,to:scope.to},s=scopeFor(range),windowSize=Number(value(mode==='usual'?'usualWindow':'regularityGap'));
  const opts={search:mode==='usual'?'':value('regularitySearch'),status:mode==='usual'?'all':value('regularityStatus')},req=request(s,{method:'range',percent:mode==='usual'?number('usualPercent'):0,min_count:0,min_amount:0});
  const page=await api.unifiedPatternPage(req,mode,windowSize,opts,positions.at(-1));if(!task.valid())return;clear(name+'Results');clear(name+'Coverage');
  $(name+'Coverage').append(UI.el('p','Полных периодов: '+page.used+' из '+page.expected+'. В предыдущем диапазоне: '+page.previous_used+'.'+(page.current_complete?'':' Анализируемый диапазон неполон.'),'muted small'));
  const root=$(name+'Results');if(!page.rows.length)root.append(UI.el('p','Нет услуг по выбранным фильтрам.','muted small'));
  if(mode==='usual')for(const metric of usualMetrics){const rows=page.rows.map(row=>{const m=row[metric];return [row.key?itemButton(row,{...s,from:page.baseline_from,to:s.to}):row.name,fmt(m.base,metric),fmt(m.current,metric),delta(m.delta,metric),percent(m.percent),UI.el('span',usualLabels[m.status]||m.status,m.status==='high'?'numeric-rise':m.status==='low'?'numeric-fall':'')];});UI.table(root,metric,'Среднее: '+page.baseline_from+' — '+page.baseline_to+'; анализ: '+page.current_from+' — '+page.current_to,['Услуга / группа','Среднее раньше','Сейчас','Отклонение','Отклонение, %','Оценка'],rows,{csvRows:page.rows.map(row=>{const m=row[metric];return [row.name,m.base,m.current,m.delta,m.percent,usualLabels[m.status]||m.status];})});}
  else {const raw=page.rows.map(row=>[row.name,page.used?row.hits:null,row.frequency,page.previous_used?row.previousHits:null,row.previousFrequency,Number.isFinite(row.frequency)&&Number.isFinite(row.previousFrequency)?row.frequency-row.previousFrequency:null,row.first,row.last,row.trailing,(row.statuses||[]).map(s=>regularLabels[s]||s).join('; ')]);const rows=raw.map((row,i)=>[itemButton(page.rows[i],s),row[1]??'—',percent(row[2]),row[3]??'—',percent(row[4]),Number.isFinite(row[5])?fmt(row[5],'count')+' п. п.':'—',row[6]||'—',row[7]||'—',row[8]??'—',row[9]]);UI.table(root,'count',caption(s),['Услуга / группа','Была в периодах','Частота сейчас, %','Была раньше','Частота раньше, %','Разница частоты, п. п.','Впервые в истории','Последний раз','Подряд без активности','Что это значит'],rows,{title:'Как часто выполнялись услуги',totalRow:false,csvRows:raw});}
  pager(name,page,positions,page.next);
 }catch(error){task.error(error);}finally{task.done();}
}
function clearExtended(){for(const name of channels.keys())invalidate(name);for(const id of ['comparisonTable','comparisonCoverage','comparisonCharts','comparisonDynamicsPlots','kpis','contributionResults','contributionCheck','structureResults','usualResults','usualCoverage','regularityResults','regularityCoverage','changesResults','changesWarnings','changesSummary'])clear(id);$('analyticsDetails').hidden=true;$('changeDetails').hidden=true;}

const localPeriods={
 comparison:['basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd'],
 structure:['structureBase','structureCurrent'],
 changes:['changesFrom','changesTo']
};
function periodExpected(id){
 if(!scope)return null;
 if(['basePeriod','baseRangeStart','structureBase'].includes(id))return scope.baseFrom||previous(scope.from,scope.displayType);
 if(id==='baseRangeEnd')return scope.baseTo||scope.baseFrom||previous(scope.from,scope.displayType);
 if(['currentRangeStart','changesFrom'].includes(id))return scope.from;
 return scope.to;
}
function updatePeriodOverrides(){
 for(const [name,ids] of Object.entries(localPeriods)){
  const indicator=$(name+'PeriodOverride');if(!indicator)continue;
  const different=Boolean(scope)&&ids.some(id=>{
   const control=$(id);
   if(!control?.dataset.unifiedTouched)return false;
   const kind=name==='changes'?changesKind:scope.displayType,expected=periodExpected(id);
   const expectedInput=window.nativePeriodSelection?.fromIsoDate?.(expected,kind)||expected;
   return control.value!==expectedInput;
  });
  indicator.hidden=!different&&!(name==='comparison'&&scope&&$('periodComparisonScope')?.dataset.unifiedTouched&&value('periodComparisonScope')!==(scope.overviewMode==='range'?'range':'single'))&&
    !(name==='changes'&&scope&&(changesKind!==scope.displayType||value('changesMethod')==='range'));
 }
}
function resetLocalPeriod(name){
 if(!scope)return;
 for(const id of localPeriods[name]||[])if($(id)?.dataset)delete $(id).dataset.unifiedTouched;
 if(name==='comparison'&&$('periodComparisonScope')?.dataset)delete $('periodComparisonScope').dataset.unifiedTouched;
 if(name==='changes'){changesKind=scope.displayType;$('changesType').value=changesKind;$('changesMethod').value='previous';}
 resetScope(scope);
}
function resetScope(next){if(!next){scope=null;clearExtended();return;}scope=copyScope(next);for(const name of channels.keys())invalidate(name);$('analyticsDetails').hidden=true;$('changeDetails').hidden=true;
 const force=dateKind!==scope.displayType;dateKind=scope.displayType;
 if(force){for(const ids of Object.values(localPeriods))for(const id of ids)if($(id)?.dataset)delete $(id).dataset.unifiedTouched;if($('periodComparisonScope')?.dataset)delete $('periodComparisonScope').dataset.unifiedTouched;}
 const before=scope.baseFrom||previous(scope.from,scope.displayType),beforeEnd=scope.baseTo||before;
 if(!$('periodComparisonScope')?.dataset.unifiedTouched)$('periodComparisonScope').value=scope.overviewMode==='range'?'range':'single';
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
 updatePeriodOverrides();if(activeView==='changes')changes();if(activeView==='dashboard'&&$('advancedAnalytics').open){comparison();if($('structurePanel').open)structure();if($('usualPanel').open)patterns('usual');if($('regularityPanel').open)patterns('regularity');}
}
function bind(ids,event,handler){for(const id of ids)$(id)?.addEventListener(event,handler);}
function init(){if(initialized)return;initialized=true;explainSensitivity();describeActiveFilters();
 const dateIds=['changesFrom','changesTo','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','structureBase','structureCurrent','usualPeriod'];for(const id of dateIds){for(const event of ['input','change'])$(id)?.addEventListener(event,()=>{$(id).dataset.unifiedTouched='true';updatePeriodOverrides();});}
 $('periodComparisonScope')?.addEventListener('change',()=>{$('periodComparisonScope').dataset.unifiedTouched='true';updatePeriodOverrides();});
 for(const name of Object.keys(localPeriods))$(name+'PeriodReset')?.addEventListener('click',()=>resetLocalPeriod(name));
 $('changesType')?.addEventListener('change',updatePeriodOverrides);
 $('changesMethod')?.addEventListener('change',updatePeriodOverrides);
 checkboxList('changesMetrics',[['count','Количество / объём'],['amount','Сумма']],changeMetrics,()=>changes());checkboxList('usualMetrics',Object.entries(titles),usualMetrics,()=>patterns('usual'));
 bind(['changesMethod','changesFrom','changesTo','changesMode','changesDirection','changesSort'],'change',()=>{invalidate('changes');changes();});
 bind(['changesPercent','changesCountMin','changesAmountMin','changesSearch'],'input',()=>{explainSensitivity();describeActiveFilters();schedule('changes',changes);});
 $('changesType').addEventListener('change',()=>{changesKind=value('changesType');for(const id of ['changesFrom','changesTo'])setDate(id,changesKind,scope?.to||new Date().toISOString().slice(0,10),true);invalidate('changes');changes();});
 for(const name of ['changes','contribution','usual','regularity']){const run=name==='changes'?changes:name==='contribution'?contributions:positions=>patterns(name,positions);$(name+'Prev').addEventListener('click',()=>{const s=slot(name);if(s.positions.length>1)run(s.positions.slice(0,-1));});$(name+'Next').addEventListener('click',()=>{const s=slot(name);if(s.page?.next)run([...s.positions,s.page.next]);});}
 bind(['comparisonMode','periodComparisonScope','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonScope','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','comparisonBaseOrg'],'change',()=>{invalidate('comparison');invalidate('contribution');comparison();});
 bind(['structureBase','structureCurrent','structureMetric','structureLimit','structureScope'],'change',structure);bind(['usualPeriod','usualWindow'],'change',()=>patterns('usual'));bind(['usualPercent'],'input',()=>schedule('usual',()=>patterns('usual')));bind(['regularityGap','regularityStatus'],'change',()=>patterns('regularity'));bind(['regularitySearch'],'input',()=>schedule('regularity',()=>patterns('regularity')));
 for(const [id,run] of [['advancedAnalytics',comparison],['contributions',contributions],['structurePanel',structure],['usualPanel',()=>patterns('usual')],['regularityPanel',()=>patterns('regularity')]])$(id).addEventListener('toggle',()=>{if($(id).open&&scope)run();});
 $('closeAnalyticsDetails').addEventListener('click',()=>{invalidate('analyticsHistory');$('analyticsDetails').hidden=true;});$('closeChangeDetails').addEventListener('click',()=>{invalidate('changeHistory');$('changeDetails').hidden=true;});
 $('overviewShowComparison').addEventListener('click',()=>{$('advancedAnalytics').open=true;$('comparisonMode').value='periods';comparison();$('comparison').scrollIntoView?.({block:'start'});});
 UI.onScope(resetScope);UI.onSnapshot(snapshot=>{if(!snapshot){scope=null;clearExtended();}});
 UI.onView('changes',()=>{activeView='changes';if(scope)changes();});UI.onView('dashboard',()=>{activeView='dashboard';if(scope?.revision===UI.revision&&$('advancedAnalytics').open)comparison();});for(const view of ['upload','groups','storage'])UI.onView(view,()=>{activeView=view;});
}
window.unifiedExtended=Object.freeze({init});
})();
