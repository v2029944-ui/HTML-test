// Dashboard presentation uses the original DOM and pure drawing helpers.
// SQLite returns metadata pages and bounded aggregates, never report JSON.
(function(){
'use strict';
const {observed,incomplete,topLevelWorkRows,workSelectionText}=window.analysisSummary;
const partialChartNote=' Частичные значения рассчитаны по доступным отчётам и заполненным суммам. Пропуски не считаются нулём.';
const UI=window.unifiedUI,O=window.unifiedOriginal,api=UI.api,$=UI.$;
const titles=window.analysisSummary.titles;
let initialized=false,scope=null,ticket=0,nameTicket=0,selectionTicket=0,chartTicket=0,selectionReady=false,allNames=false,totalNames=0;
let displayedOverviewScope=null,displayedChart=null,lastReferenceMetric=null,workTotal=null,workCountTicket=0;
const overviewResults=['overviewKpis','overviewInsights','overviewOrganizations','overviewAlerts'];
const chartResults=['chartsContainer','dataTable','chartOrgTotals'];
let selectedRows=new Map(),namePage=null,nameIndex=0,seriesIndex=0,printCapture=null,lastRevision=null,searchTimer=null;
const state=UI.state;
function clear(id,text='',reset=true){const node=$(id);if(!node)return;if(reset)window.sqlRowScroll?.reset(node);node.replaceChildren();if(text)node.append(UI.el('p',text,'muted small'));}
function value(id){return $(id)?.value||'';}
 function orientation(){return value('chartOrientation')==='horizontal'?'horizontal':'vertical';}
function chartBusy(busy){
 for(const id of chartResults){
  const node=$(id);
  node.inert=busy;node.setAttribute('aria-busy',String(busy));
  const pager=node.nextElementSibling;
  if(pager?.matches('.sql-pager'))pager.inert=busy;
 }
}
function clearChart(){
 displayedChart=null;
 for(const id of chartResults)clear(id);
 clear('chartHelp');
 chartBusy(false);
}
function invalidateScope(){
 scope=null;printCapture=null;displayedOverviewScope=null;chartTicket++;clearChart();
 for(const id of overviewResults){clear(id);$(id).inert=false;$(id).setAttribute('aria-busy','false');}
 clear('overviewPeriodText');$('printSummary').disabled=true;UI.publishScope(null);
}
function valid(mine,revision){return ticket===mine&&UI.revision===revision;}
function fmt(n,m='count'){return UI.fmt(n,m);}
function errorText(e){return window.analizUserError?window.analizUserError(e):(e?.message||String(e));}
function chosenMetrics(){return [...state.selectedMetrics].filter(m=>titles[m]);}
function orgs(){return [...state.selectedOrganizations];}
function serialize(d){return d.toISOString().slice(0,10);}
function shift(date,kind,n){const d=new Date(date+'T00:00:00Z');if(kind==='month')d.setUTCMonth(d.getUTCMonth()+n);else if(kind==='year')d.setUTCFullYear(d.getUTCFullYear()+n);else d.setUTCDate(d.getUTCDate()+n*(kind==='week'?7:1));return serialize(d);}
function rangeFromIso(kind,from,to,source){return UI.range(kind,window.nativePeriodSelection.fromIsoDate(from,kind),window.nativePeriodSelection.fromIsoDate(to,kind),source);}
function previousRange(current){
 const kind=current.displayType,from=current.from,to=current.to;
 const periods=kind==='month'?(Number(to.slice(0,4))-Number(from.slice(0,4)))*12+Number(to.slice(5,7))-Number(from.slice(5,7))+1:kind==='year'?Number(to.slice(0,4))-Number(from.slice(0,4))+1:Math.floor((Date.parse(to)-Date.parse(from))/(86400000*(kind==='week'?7:1)))+1;
 return rangeFromIso(kind,shift(from,kind,-periods),shift(from,kind,-1),current.source);
}

function chosenBaseRange(current){
 const suggested=previousRange(current);
 if(!suggested)return null;
 const kind=current.displayType,node=$('analysisBasePeriod');
 if(node.dataset.periodType!==kind){delete node.dataset.userSelected;UI.setPeriodInput('analysisBasePeriod',kind,suggested.from);}
 else if(!node.dataset.userSelected)UI.setPeriodInput('analysisBasePeriod',kind,suggested.from);
 if(!node.dataset.userSelected)return suggested;
 const from=window.nativePeriodSelection.normalize(kind,node.value);
 if(!from)throw Error('Выберите корректный базовый период.');
 const count=kind==='month'?(Number(current.to.slice(0,4))-Number(current.from.slice(0,4)))*12+Number(current.to.slice(5,7))-Number(current.from.slice(5,7))+1:
 kind==='year'?Number(current.to.slice(0,4))-Number(current.from.slice(0,4))+1:
 Math.floor((Date.parse(current.to)-Date.parse(current.from))/(86400000*(kind==='week'?7:1)))+1;
 return rangeFromIso(kind,from,shift(from,kind,count-1),current.source);
}
function layout(){const single=value('overviewMode')==='single';$('overviewSingle').hidden=!single;$('overviewRange').hidden=single;if(single){$('rangeStart').value=value('overviewPeriod');$('rangeEnd').value=value('overviewPeriod');}
 const departments=value('chartBreakdown')==='organizations',chosen=departments?[...state.chartMetrics]:chosenMetrics();
 $('chartOrgScopeLabel').hidden=!departments;$('chartMetricsWrap').hidden=!departments;$('chartSeriesLayoutLabel').hidden=!departments||chosen.length<2;$('chartNotes').hidden=!departments;$('chartComparisonHint').hidden=!departments;$('chartReadGuide').hidden=!departments||state.chart==='table';
 $('chartDescription').textContent=departments?'Показатели подразделений за общий период. Настройки этого графика — ниже.':'Отмеченные работы и группы из общих фильтров.';
 $('chartComparisonHint').textContent='Подразделения выбираются вверху страницы. Показатели этого графика задаются независимо от показателей обзора и работ.';
 $('chartReadGuide').textContent=chosen.length>1&&value('chartSeriesLayout')==='combined'?'Цвет обозначает подразделение. В совместной диаграмме значения показаны в % от максимума каждого показателя, а не как процент изменения.':'У каждого показателя своя шкала в исходных единицах. Цвет обозначает подразделение.';
 const displayed=displayedChart&&displayedChart.scope.revision===UI.revision?displayedChart.mode:state.chart;
 $('chartsContainer').hidden=displayed==='table';$('dataTable').hidden=displayed!=='table';
 document.querySelectorAll('[data-chart]').forEach(button=>button.classList.toggle('active',button.dataset.chart===state.chart));
}
function filters(){O.renderMultiFilter('orgFilter',(UI.snapshot?.org_names||[]).map(n=>[n,n]),state.selectedOrganizations,()=>window.referenceDetail?.active==='overviewPanel'
 ?window.referenceFilters.setOrganizations(orgs()):calculate());O.renderMultiFilter('metric',[['amount','Сумма'],['count','Количество / объём']],state.selectedMetrics,()=>{if(printCapture?.revision===UI.revision)drawOverview(printCapture.result,printCapture.organizations,printCapture.scope,printCapture.before);publish();renderCharts();});O.renderMultiFilter('chartMetrics',[['amount','Сумма'],['count','Количество / объём'],['average','Сумма / количество (объём)']],state.chartMetrics,()=>renderCharts());}
function publish(){if(!scope)return;scope={...scope,organizations:orgs(),allNames,selectedNames:allNames?[]:[...selectedRows.values()].map(r=>r.name),selectedKeys:allNames?[]:[...selectedRows.keys()],nameRows:[...selectedRows.values()]};state.selectedCategories=new Set(scope.selectedNames);UI.publishScope(scope);}
function selectionCount(){
 state.selectedCategories=new Set([...selectedRows.values()].map(r=>r.name));
 const label=$('categoryCount'),mine=++workCountTicket,revision=UI.revision;
 if(workTotal===null){label.textContent=allNames?'Все работы и группы':'Выбрано '+selectedRows.size+' из '+totalNames;return;}
 const format=selected=>workSelectionText({selected,total:workTotal});
 if(allNames||!selectedRows.size){label.textContent=format(allNames?workTotal:0);return;}
 label.textContent='Подсчёт работ…';
 api.unifiedSelectedNameOptions(revision,[...selectedRows.keys()]).then(page=>{
  if(mine===workCountTicket&&revision===UI.revision)label.textContent=workSelectionText(page.work_counts);
 }).catch(()=>{if(mine===workCountTicket&&revision===UI.revision)label.textContent='Не удалось подсчитать работы';});
}
function changeWorks(refreshNames=false){
 ++selectionTicket;selectionReady=true;seriesIndex=0;selectionCount();
 publish();
 if(window.referenceDetail?.active==='overviewPanel')window.referenceFilters?.setWorks(allNames,selectedRows,true);
 if(refreshNames)void names(nameIndex);
 return renderCharts();
}
function pager(root,index,run,options){
 const pages=Math.max(1,Math.ceil(options.total/options.size));
 const go=i=>{if(options.current()&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return run(i);return false;};
 if(window.sqlRowScroll?.attach(root,{index,size:options.size,total:options.total,count:options.count,hasNext:index+1<pages,label:options.label,controlsOnly:root.id==='chartsContainer',direct:true,go}))return;
 const bar=UI.el('div',null,'change-pagination'),previous=UI.el('button','← Предыдущая','button secondary'),forward=UI.el('button','Следующая →','button secondary');previous.type=forward.type='button';previous.disabled=index===0;forward.disabled=index+1>=pages;previous.addEventListener('click',()=>go(index-1));forward.addEventListener('click',()=>go(index+1));bar.append(previous,UI.el('span','Показано '+options.count+' из '+options.total,'muted small'),forward);root.append(bar);
}
async function names(index=0,paging=false){
 if(!Number.isSafeInteger(UI.revision))return;const mine=++nameTicket,revision=UI.revision;clear('categories','Загрузка названий…',!paging);
 try{const page=await api.unifiedNameOptionsAtPage(revision,value('categorySearch'),index,50);if(mine!==nameTicket||revision!==UI.revision)return;namePage=page;nameIndex=index;workTotal=page.work_counts?.total??null;if(!value('categorySearch'))totalNames=page.total_rows;
  clear('categories','',false);const root=$('categories');
  for(const row of page.rows){const label=UI.el('label',null,'category-row'),box=UI.el('input');box.type='checkbox';box.checked=allNames||selectedRows.has(row.key);box.disabled=allNames;box.addEventListener('change',()=>{if(box.checked){if(selectedRows.size>=50){box.checked=false;UI.notice('Можно отметить до 50 отдельных работ. «Выбрать все» включает все работы без ограничения.',true);return;}selectedRows.set(row.key,row);}else selectedRows.delete(row.key);void changeWorks();});
   const caption=UI.el('span',window.analysisSummary.itemLabel(row),row.kind==='group'?'work-group-option':null);
   label.append(box,caption);root.append(label);
  }
  if(!page.rows.length)root.append(UI.el('p','Совпадений нет.','muted small'));
  if(allNames)root.append(UI.el('p','Выбраны все работы, включая строки, которые не отображаются сейчас. Чтобы отметить отдельные работы, нажмите «Снять всё».','muted small'));
  pager(root,index,i=>names(i,true),{size:50,total:page.total_rows,count:page.rows.length,label:'Названия работ',current:()=>mine===nameTicket&&revision===UI.revision});selectionCount();
 }catch(error){if(mine===nameTicket&&revision===UI.revision)clear('categories','Не удалось загрузить названия. '+errorText(error));}
}
async function top(eager=false){
 if(!scope)return false;const mine=ticket,request=++selectionTicket,s={...scope,organizations:orgs()};
 const current=()=>valid(mine,s.revision)&&request===selectionTicket;
 try{const result=await api.nativeAnalysisRanking(s.revision,s.type,s.from,s.to,s.organizations,null,chosenMetrics()[0]||'amount','selectable',null,null);if(!current())return null;
  if(!result.complete&&!result.rows.length){if(!eager)UI.notice('Нет данных для рейтинга за выбранный диапазон.',true);return false;}
  if(!eager&&!result.complete)UI.notice('Рейтинг рассчитан по доступным отчётам. В выбранном диапазоне есть пропуски.',true);
  let rows=result.rows.slice(0,8);
  if(workTotal!==null&&rows.length){
   const options=await api.unifiedSelectedNameOptions(s.revision,rows.map(row=>row.key));
   if(!current())return null;
   const metadata=new Map(options.rows.map(row=>[row.key,row]));
   rows=rows.flatMap(row=>metadata.has(row.key)?[metadata.get(row.key)]:[]);
  }
  selectedRows=new Map(rows.map(row=>[row.key,row]));allNames=false;selectionReady=true;selectionCount();return true;
 }catch(error){if(!current())return null;if(!eager)UI.notice(errorText(error),true);return false;}
}
function changeValue(delta,metric){return window.analysisSummary.displayedChange(delta,metric,fmt);}
function changeClass(direction){return direction==='up'?'numeric-rise':direction==='down'?'numeric-fall':'';}
function overviewDifference(now,before,metric){
 if(!Number.isFinite(now)||!Number.isFinite(before))return 'Недостаточно данных для сравнения';
 const delta=now-before,change=changeValue(delta,metric);
 if(change.direction==='neutral')return change.label;
 return change.value+' ('+(before===0?'процент не рассчитан':(delta/Math.abs(before)*100).toLocaleString('ru-RU',{maximumFractionDigits:1})+'%')+')';
}
const overviewValue=(period,metric)=>observed(period)?.[metric];
function overviewBase(summary,metric){
 if(metric==='average'&&summary.average&&summary.average.delta==null)return null;
 return overviewValue(summary.before,metric);
}
function dateLabel(iso){return /^\d{4}-\d{2}-\d{2}$/.test(iso)?iso.slice(8,10)+'.'+iso.slice(5,7)+'.'+iso.slice(0,4):iso;}
function rangeLabel(from,to){return dateLabel(from)+(from===to?'':' — '+dateLabel(to));}
function sourceRangeLabel(s){return rangeLabel(s.from,s.source==='daily'?s.to:O.periodEnd(s.to,s.displayType));}
async function renderMissingMatrix(root,s){
 await window.unifiedCoverage.render(root,s,()=>UI.revision===s.revision&&printCapture?.scope===s);
}
async function suggestWeeklyReports(s){
 if(s.source!=='daily'||s.displayType!=='week')return;
 try{
  const options=await api.unifiedPeriodOptions(s.revision,'week',null,null,500);
  if(UI.revision!==s.revision||printCapture?.scope!==s||!options.rows?.some(p=>p.start>=s.from&&p.start<=s.to))return;
  const message=UI.el('p','В выбранном диапазоне есть недельные отчёты. Сейчас выбраны дневные, поэтому полнота проверяется по каждому дню.','muted small');
  const button=UI.el('button','Переключить на недельные отчёты','button secondary');
  button.type='button';button.addEventListener('click',()=>{if(UI.revision!==s.revision)return;
   if(window.referenceFilters)return window.referenceFilters.openImportedPeriod({kind:'week',from:s.from,to:s.to});
   $('unifiedSource').value='imported';$('periodType').value='week';void configure(UI.snapshot);});
  message.append(button);$('overviewAlerts').append(message);
 }catch{/* Подсказка о другом источнике не влияет на расчёты. */}
}
async function suggestWeeklyWhenDailyEmpty(revision,mine){
 try{
  const weekly=await api.unifiedPeriodOptions(revision,'week',null,null,1);
  if(!valid(mine,revision)||!weekly.total_rows)return;
  const message=UI.el('p','Дневных отчётов пока нет, но в базе загружены недельные. Для их просмотра выберите недельный источник.','warn small');
  const button=UI.el('button','Открыть недельные отчёты','button secondary');button.type='button';
  button.addEventListener('click',()=>{if(UI.revision!==revision)return;
   if(window.referenceFilters&&weekly.max)return window.referenceFilters.openImportedPeriod({kind:'week',from:weekly.max,to:O.periodEnd(weekly.max,'week')});
   $('unifiedSource').value='imported';$('periodType').value='week';
   if(weekly.max){for(const id of ['overviewPeriod','rangeStart','rangeEnd'])UI.setPeriodInput(id,'week',weekly.max);}
   void configure(UI.snapshot);
  });
  message.append(button);$('overviewAlerts').append(message);
 }catch{/* Подсказка не заменяет обязательную проверку источников. */}
}
function coverageDetails(label,s){
 const details=UI.el('details',null,'coverage-details'),matrix=UI.el('div');
 details.append(UI.el('summary',label),matrix);let loaded=false;
 details.addEventListener('toggle',()=>{if(details.open&&!loaded){loaded=true;void renderMissingMatrix(matrix,s);}});
 return details;
}
function drawOverview(result,organizations,s,before){
 for(const id of overviewResults)clear(id);
 const summary=result.summary,current=summary.after,previous=summary.before,root=$('overviewKpis'),insights=$('overviewInsights');
 $('overviewPeriodText').textContent=sourceRangeLabel(s)+' · '+s.organizations.join(', ');$('printSummary').disabled=!current.periods;
 for(const [metric,label] of [['amount','Сумма выполненных работ'],['count','Количество / объём'],['average','Сумма / количество (объём)']]){
  const card=UI.el('div',null,'kpi'),shown=observed(current)?.[metric],now=overviewValue(current,metric),old=overviewBase(summary,metric);
  const comparable=Number.isFinite(now)&&Number.isFinite(old);
  card.append(UI.el('span',label),UI.el('strong',fmt(shown,metric)),UI.el('small',!comparable&&incomplete(current)&&Number.isFinite(shown)?'Частичное значение':overviewDifference(now,old,metric),comparable?changeClass(changeValue(now-old,metric).direction):''));root.append(card);
 }
 const orgCard=UI.el('div',null,'kpi');orgCard.append(UI.el('span','Выбрано подразделений'),UI.el('strong',String(s.organizations.length)),UI.el('small','Из '+UI.snapshot.org_names.length+' подразделений'));root.append(orgCard);
 for(const item of organizations){const line=UI.el('div',null,'overview-unit');line.append(UI.el('span',item.org),UI.el('strong',fmt(observed(item)?.amount,'amount')+' · Количество / объём: '+fmt(observed(item)?.count,'count')+(incomplete(item)?' · Неполные данные':''),incomplete(item)?'quality-bad':''));$('overviewOrganizations').append(line);}
 const averageNotice=window.analysisSummary.averageNote(current);
 if(averageNotice)root.append(UI.el('p',averageNotice,'muted small'));
 const flagged=!current.complete||Boolean(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports);
 if(!current.complete){
  const alert=UI.el('div',null,'overview-alert warn');
  alert.setAttribute('role','status');
  alert.append(UI.el('p','Неполные данные: '+current.missing+' пропусков (подразделение × '+({day:'день',week:'неделя',month:'месяц'}[s.type]||'период')+') за '+sourceRangeLabel(s)+'.'));
  alert.append(coverageDetails('Показать где',s));
  if(current.available_reports>0)alert.append(UI.el('p','Частичный итог по '+current.available_reports+' доступным отчётам: сумма '+fmt(current.available_total.amount,'amount')+' · количество / объём '+fmt(current.available_total.count,'count')+'.','muted small'));
  $('overviewAlerts').append(alert);
 }
 if(current.complete&&incomplete(current))$('overviewAlerts').append(UI.el('p',partialChartNote.trim(),'warn small'));
 if(current.complete)$('overviewAlerts').append(coverageDetails('Качество данных по подразделениям и периодам',s));
 if(flagged){
  const info=UI.el('div',null,'overview-alert');
  if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)
   info.append(UI.el('p','Есть замечания к отчётам: некорректных '+current.invalid_reports+', повторов '+current.duplicate_issues+', пересечений '+current.overlap_issues+', с предупреждениями импорта '+current.import_warning_reports+'.','muted small'));
  const inspect=UI.el('button','Проверить данные','button primary');inspect.type='button';
  inspect.addEventListener('click',()=>window.referenceDesktop.openPanel('upload',inspect));info.append(inspect);$('overviewAlerts').append(info);
 }
 insights.append(UI.el('p','Сравнение с базовым периодом: '+sourceRangeLabel(before)+'.','muted small'));
 if(!current.complete||!previous.complete)insights.append(UI.el('p','Сравнение учитывает доступные отчёты. Пропусков в обоих диапазонах: '+(current.missing+previous.missing)+'.','muted small'));
 for(const metric of ['amount','count']){const line=UI.el('div',null,'overview-line');line.append(UI.el('strong',titles[metric]+': '+overviewDifference(overviewValue(current,metric),overviewValue(previous,metric),metric)));insights.append(line);}const metric=state.selectedMetrics.has('amount')?'amount':'count';for(const row of (result['top_'+metric]||[]).filter(row=>Number.isFinite(row[metric]?.delta)&&row[metric].delta!==0).slice(0,2)){const change=changeValue(row[metric].delta,metric),line=UI.el('div',null,'overview-line');line.append(UI.el('strong',window.analysisSummary.itemLabel(row)+': '+(change.direction==='neutral'?change.label:change.value),changeClass(change.direction)),UI.el('small','Один из крупнейших вкладов в изменение показателя «'+titles[metric]+'»'));insights.append(line);}
 insights.append(UI.el('p','Единицы суммы и количества / объёма необходимо сверять с источником. Дробное количество может означать объём, а не число работ.','muted small'));
 printCapture={revision:s.revision,scope:s,before,summary,organizations,result};
 displayedOverviewScope=s;window.referenceDetail?.refreshOverviewCaption?.();
}
async function calculate(shared=null){
 const mine=++ticket,revision=UI.revision;
 printCapture=null;chartTicket++;$('printSummary').disabled=true;
 if(displayedChart){
  if(displayedChart.scope.revision!==revision)clearChart();
  else chartBusy(true);
 }
 if(displayedOverviewScope?.revision!==revision){
  displayedOverviewScope=null;clear('overviewPeriodText');
  for(const id of overviewResults)clear(id,'Загрузка…');
 }
 for(const id of overviewResults){$(id).inert=true;$(id).setAttribute('aria-busy','true');}
 try{if(!Number.isSafeInteger(revision))throw Error('Дождитесь подготовки данных.');layout();const displayType=value('periodType'),source=value('unifiedSource'),range=shared?{displayType,source,type:shared.type,from:shared.from,to:shared.to}:UI.range(displayType,value('rangeStart'),value('rangeEnd'),source);if(!range||!orgs().length)throw Error('Выберите корректный период и подразделения.');const s={...range,revision,organizations:orgs(),selectedNames:[],selectedKeys:[],allNames:false,nameRows:[]},before=shared?{...range,from:shared.baseFrom,to:shared.baseTo}:chosenBaseRange(s);if(!before)throw Error('Базовый период выходит за допустимые календарные границы.');s.baseFrom=before.from;s.baseTo=before.to;s.overviewMode=value('overviewMode');scope=s;
  // Reference selections already include the work filters and exact period bounds.
  if(shared)publish();
  const [overview,organizations]=await window.analysisSummary.read(api,shared||s);
  if(!valid(mine,revision))return;drawOverview(overview,organizations,s,before);
   if(!overview.summary.after.complete)void suggestWeeklyReports(s);
  if(!selectionReady){
   const ranked=await top(true);if(!valid(mine,revision))return;
   if(ranked===false){
    const selection=selectionTicket,page=await api.unifiedNameOptionsAtPage(revision,'',0,8);
    if(!valid(mine,revision))return;
    if(selection===selectionTicket){selectedRows=new Map(page.rows.map(r=>[r.key,r]));selectionReady=true;}
   }
  }
  if(!shared)publish();
  selectionCount();void names(nameIndex);if($('advancedAnalytics').open)await renderCharts();
 }catch(error){if(valid(mine,revision)){invalidateScope();clear('overviewAlerts','Не удалось загрузить обзор. '+errorText(error));}}
 finally{if(valid(mine,revision))for(const id of overviewResults){$(id).inert=false;$(id).setAttribute('aria-busy','false');}}
}
function descriptors(points,granularity,s){return window.analysisSummary.timelinePeriods(points,granularity,s,O.periodEnd);}
const allPeriodsHidden='Фильтр скрыл все периоды в этой выборке. Отключите скрытие нулевых периодов и периодов без отчётов, чтобы увидеть их.';
function confirmedZero(point){return point&&point.missing===0&&point.total?.count===0&&point.total?.amount===0;}
function reportBuckets(result){return Array.isArray(result.buckets_with_reports)?new Set(result.buckets_with_reports):null;}
function withoutReports(point,buckets){return point.missing>0&&buckets!==null&&!buckets.has(point.start);}
function chartInterval(s){const to=s.source==='daily'?s.to:O.periodEnd(s.to,s.displayType);return O.displayDay(s.from)+(to===s.from?'':' — '+O.displayDay(to));}
function plotSection(root,metric,title=titles[metric]){const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');section.append(UI.el('h3',title),plot);root.append(section);return plot;}
function chartExport(scope,isCurrent,table,metrics){
 if(window.referenceDetail?.active!=='chartPanel')return null;
 const req=UI.request(scope),options={table,metrics:[...metrics],hideEmpty:$('hideEmptyPeriods').checked,matchChart:true};
 const valid=()=>isCurrent()&&window.referenceDetail?.active==='chartPanel';
 return {export:async()=>{
  if(!valid())throw Error('Параметры графика изменились. Повторите расчёт.');
  const path=await api.chooseCsvPath('dinamika-'+table+'-'+scope.from+'-'+scope.to+'.csv');
  if(!path)return;
  if(!valid())throw Error('Данные изменились. Повторите расчёт.');
  await api.referenceTrendCsv(req,options,path);
 }};
}
async function readWorkSeries(s,rows,current){
 const series=[];
 for(let i=0;i<rows.length&&current();i+=4){
  series.push(...await Promise.all(rows.slice(i,i+4).map(row=>api.nativeWorkTimeline(
   s.revision,s.type,s.from,s.to,s.organizations,null,row.key,null,s.source==='daily'?s.displayType:null,s.selectedNames))));
 }
 return series;
}
function validateWorkSeries(series,totals){
 if(series.some(item=>item.granularity!==totals.granularity||item.points.length!==totals.points.length||
  item.points.some((point,i)=>point.start!==totals.points[i].start)))throw Error('Несовпадающие границы SQL-серий. Повторите расчёт.');
}
function treeWorkLabel(row,depth,render){
 if(!row.has_children&&!depth)return row.name;
 const label=UI.el(row.has_children?'button':'span',row.name,row.has_children?'text-button work-group-option chart-group-toggle':'chart-group-label');
 label.style.setProperty('--chart-tree-indent',Math.min(depth,8)*0.85+'rem');
 label.dataset.workName=row.name;
 if(!row.has_children)return label;
 label.type='button';
 label.setAttribute('aria-expanded','false');
 let expanded=false,ticket=0,rows=[];
 function clearRows(){for(const tr of rows){tr.firstElementChild?.firstElementChild?.clearTree?.();tr.remove();}rows=[];}
 function collapse(){
  expanded=false;ticket++;label.setAttribute('aria-expanded','false');clearRows();
 }
 async function open(index,restoreFocus){
  expanded=true;label.setAttribute('aria-expanded','true');clearRows();
  const mine=++ticket,anchor=label.closest('tr');
  const current=()=>mine===ticket&&expanded&&label.isConnected&&render.current();
  function append(tr){
   tr.classList.add('chart-group-child');tr.dataset.treeDepth=String(depth+1);
   (rows.at(-1)||anchor).after(tr);rows.push(tr);
  }
  function status(text){
   const tr=UI.el('tr'),cell=UI.el('td',text,'muted small chart-group-status');
   cell.colSpan=render.columns;tr.append(cell);append(tr);return cell;
  }
  status('Загрузка работ и подгрупп…');
  try{
   const page=await api.unifiedGroupChildrenAtPage(render.scope.revision,row.key,index);
   if(!current())return;
   const series=await readWorkSeries(render.scope,page.rows,current);
   if(!current())return;
   validateWorkSeries(series,render.totals);clearRows();
   for(const [i,entry] of page.rows.entries()){
    const tr=UI.el('tr'),first=UI.el('td');
    const child=treeWorkLabel(entry,depth+1,render);
    if(typeof child==='string')first.textContent=child;else first.append(child);
    tr.append(first,...render.indices.map(p=>UI.el('td',fmt(observed(series[i].points[p])?.[render.metric],render.metric))));
    append(tr);
   }
   if(!page.rows.length)status('В группе нет работ или подгрупп.');
   if(page.total_rows>50){
    const controls=status(''),host=UI.el('span');
    controls.append(host);
    const pager=window.sqlRowScroll.attach(host,{
     index,size:50,total:page.total_rows,count:page.rows.length,controlsOnly:true,footer:controls,
     label:'Страницы группы '+row.name,go:next=>open(next,true)
    });
    if(restoreFocus)(pager?.querySelector('[data-page-action=choose]')||label).focus({preventScroll:true});
   }
  }catch(error){if(current()){clearRows();status('Не удалось открыть группу. '+errorText(error));if(restoreFocus)label.focus({preventScroll:true});}}
 }
 label.addEventListener('click',()=>{if(expanded)collapse();else void open(0);});
 label.clearTree=collapse;
 return label;
}
function printWorkTree(section,interval){
 const button=UI.el('button','Печать текущей страницы','button secondary');
 button.type='button';
 button.addEventListener('click',()=>{
  const root=UI.el('section'),table=section.querySelector('table').cloneNode(true);
  table.querySelectorAll('td.chart-group-status').forEach(cell=>cell.parentElement.remove());
  table.querySelectorAll('button[data-work-name]').forEach(toggle=>{
   const label=UI.el('span',toggle.dataset.workName,'chart-group-label');
   label.style.setProperty('--chart-tree-indent',toggle.style.getPropertyValue('--chart-tree-indent'));
   toggle.replaceWith(label);
  });
  root.id='referencePrintDocument';
  root.append(UI.el('h1','Динамика работ · '+section.querySelector('h3').textContent),
   UI.el('p','Период '+interval+'. Текущая страница и раскрытые группы.'),table);
  document.body.append(root);
  document.body.dataset.referencePrint='document';
  try{window.print();}finally{root.remove();delete document.body.dataset.referencePrint;}
 });
 section.append(button);
}
async function renderCharts(index=seriesIndex,paging=false){
 const mine=++chartTicket;
 layout();
 if(window.referenceDetail?.active&&window.referenceDetail.active!=='chartPanel')return;
 if(value('workViewMode')==='composition'||!scope||scope.revision!==UI.revision)return;const s={...scope,organizations:orgs(),selectedNames:allNames?[]:[...selectedRows.values()].map(r=>r.name),allNames},revision=s.revision;
 // The reference detail charts daily observations even when the shell selects a whole week or month.
 if(window.referenceDetail?.active==='chartPanel'&&s.source==='daily')s.displayType='day';
 const isCurrent=()=>chartTicket===mine&&UI.revision===revision;
 const requestedScope=scope;
 if(displayedChart?.scope.revision!==revision)for(const id of chartResults)clear(id,'Загрузка…',!paging);
 chartBusy(true);
 const commitView=()=>{
  displayedChart={scope:requestedScope,mode:state.chart};layout();
  window.referenceDetail?.refreshChartCaption?.();
 };
 try{if(value('chartBreakdown')==='organizations'){await departmentCharts(s,isCurrent);if(isCurrent())commitView();return;}
  if(!allNames&&!selectedRows.size)throw Error('Выберите хотя бы одну работу или группу.');
  const pageSize=state.chart==='table'?50:8;let rows,seriesTotal;
  if(allNames){const page=await api.unifiedNameOptionsAtPage(revision,'',index,pageSize,true);if(!isCurrent())return;rows=page.rows;seriesTotal=page.total_rows;}
  else {const roots=topLevelWorkRows([...selectedRows.values()]),offset=index*pageSize;seriesTotal=roots.length;rows=roots.slice(offset,offset+pageSize);}
  if(!rows.length){commitView();clear('chartsContainer','Нет работ для графика.');clear('dataTable','Нет работ для таблицы.');clear('chartOrgTotals');return;}
  const [totals,series]=await Promise.all([
   api.nativeAnalysisTimeline(revision,s.type,s.from,s.to,s.organizations,null,s.source==='daily'?s.displayType:null,s.selectedNames),
   readWorkSeries(s,rows,isCurrent)
  ]);
  if(!isCurrent())return;
  validateWorkSeries(series,totals);
  const nonzeroWorkBuckets=new Set(totals.zero_buckets_with_nonzero_works);
  const buckets=reportBuckets(totals),hide=$('hideEmptyPeriods').checked;
  const hiddenZero=p=>confirmedZero(p)&&!nonzeroWorkBuckets.has(p.start);
  const points=hide?totals.points.filter(p=>!hiddenZero(p)&&!withoutReports(p,buckets)):totals.points;
  const periods=descriptors(points,totals.granularity,s);
  const hiddenPeriods=hide?descriptors(totals.points.filter(hiddenZero),totals.granularity,s):[];
  window.referenceDetail?.setChartPeriodCount?.(points.length);
  const indices=points.map(p=>totals.points.indexOf(p));
  const data=points.map((p,i)=>{if(p.missing>0&&!Number.isFinite(observed(p)?.count))return null;return new Map(rows.map((row,j)=>[row.key,observed(series[j].points[indices[i]])]));});
  const root=state.chart==='table'?$('dataTable'):$('chartsContainer');
  for(const id of chartResults)clear(id,'',$(id)!==root);
  const filteredAll=totals.points.length>0&&!points.length;
  if(filteredAll)clear(state.chart==='table'?'dataTable':'chartsContainer',allPeriodsHidden,false);
  for(const metric of chosenMetrics())if(state.chart==='table'){
   const raw=[['Итого по выбранным работам',...points.map(p=>observed(p)?.[metric]??null)],...rows.map(row=>[row.name,...data.map(map=>map?.get(row.key)?.[metric]??null)])];
   const detail={scope:s,metric,totals,indices,columns:periods.length+1,current:isCurrent};
   UI.table($('dataTable'),metric,chartInterval(s)+(metric==='average'?' · прочерк означает неполные данные или неположительное количество.':' · прочерк означает неполные данные.'),['Работа / группа',...periods.map(O.displayPeriod)],raw.map((row,i)=>[i?treeWorkLabel(rows[i-1],0,detail):row[0],...row.slice(1).map(n=>fmt(n,metric))]),{csvRows:raw,csvContext:chartExport(s,isCurrent,'works',[metric])});
   if(rows.some(row=>row.has_children))printWorkTree($('dataTable').lastElementChild,chartInterval(s));
  }else if(!filteredAll)UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric,titles[metric]+' · '+chartInterval(s)),{chart:state.chart,orientation:orientation(),hiddenPeriods,categories:rows.map(r=>r.key),data,label:key=>rows.find(r=>r.key===key).name,pointNotice:i=>incomplete(points[i])?(points[i].missing?'Неполные данные: пропусков '+points[i].missing:'В некоторых строках сумма не указана.'):''});
  commitView();seriesIndex=index;pager(root,index,i=>{seriesIndex=i;return renderCharts(i,true);},{size:pageSize,total:seriesTotal,count:rows.length,label:'Работы на графике',current:isCurrent});
  $('chartHelp').textContent='Выбрано подразделений: '+s.organizations.length+' из '+UI.snapshot.org_names.length+'. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[totals.granularity]||'по периодам')+'.'+(points.some(incomplete)?partialChartNote:' Пропуски не считаются нулём.');
 }catch(error){if(isCurrent()){for(const id of ['chartsContainer','dataTable'])clear(id,'Не удалось построить аналитику. '+errorText(error));clear('chartOrgTotals');clear('chartHelp');commitView();}}
 finally{if(isCurrent())chartBusy(false);}
}
async function departmentCharts(base,isCurrent){
 const all=value('chartOrgScope')==='all',s={...base,selectedNames:all?[]:base.selectedNames},chosen=[...state.chartMetrics];
 if(!all&&!base.allNames&&!selectedRows.size)throw Error('Нет отмеченных работ. Выберите работы или учитывайте все выполненные работы.');
 const [trends,organizations]=await Promise.all([api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(0,8),null,s.source==='daily'?s.displayType:null,s.selectedNames),api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,s.selectedNames)]);
 if(!isCurrent())return;
 let buckets=reportBuckets(trends);
 for(let offset=8;offset<s.organizations.length;offset+=8){
  const page=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(offset,offset+8),null,s.source==='daily'?s.displayType:null,s.selectedNames);
  if(!isCurrent())return;
  trends.series.push(...page.series);
  const extra=reportBuckets(page);
  if(buckets&&extra){for(const start of extra)buckets.add(start);}else buckets=null;
 }
 for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id);
 const rawSeries=trends.series||[],first=rawSeries[0]?.points||[],hide=$('hideEmptyPeriods').checked;
 const zeros=first.map((_,i)=>hide&&rawSeries.every(series=>confirmedZero(series.points[i])));
 const keep=first.map((p,i)=>!hide||(!zeros[i]&&!withoutReports(p,buckets)));
 const points=first.filter((_,i)=>keep[i]),periods=descriptors(points,trends.granularity,s);
 const hiddenPeriods=descriptors(first.filter((_,i)=>zeros[i]),trends.granularity,s);
 window.referenceDetail?.setChartPeriodCount?.(points.length);
 const data=first.map((p,i)=>new Map(rawSeries.map(series=>[series.organization,observed(series.points[i])]))).filter((p,i)=>keep[i]);
 function pointNotice(i,shown=s.organizations){
  const index=first.indexOf(points[i]);
  const missing=rawSeries.filter(series=>shown.includes(series.organization)&&incomplete(series.points[index])).map(series=>series.organization);
  return missing.length?'Неполные данные: '+missing.join(', ')+'.':'';
 }
 const filteredAll=first.length>0&&!points.length;
 if(filteredAll)clear(state.chart==='table'?'dataTable':'chartsContainer',allPeriodsHidden,false);
 if(state.chart==='table'){
  const raw=s.organizations.flatMap(org=>chosen.map(metric=>[org,titles[metric],...data.map(map=>map.get(org)?.[metric]??null)]));
  UI.table($('dataTable'),'amount',chartInterval(s),['Подразделение','Показатель',...periods.map(O.displayPeriod)],raw.map((row,i)=>[...row.slice(0,2),...row.slice(2).map(n=>fmt(n,chosen[i%chosen.length]))]),{title:'Все выбранные показатели по подразделениям',totalRow:false,csvRows:raw,csvContext:chartExport(s,isCurrent,'organizations',chosen)});
 }else if(!filteredAll&&chosen.length>1&&value('chartSeriesLayout')==='combined')UI.drawDepartments(periods,s.organizations,chosen,data,plotSection($('chartsContainer'),'amount',O.departmentChartTitle(s.organizations.length,chosen.length)+' · '+chartInterval(s)),{orientation:orientation()});
 else if(!filteredAll)for(const metric of chosen)UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric,titles[metric]+' · '+chartInterval(s)),{chart:state.chart,orientation:orientation(),hiddenPeriods,seriesType:'organizations',categories:s.organizations,data,pointNotice});
 const raw=organizations.map(org=>[org.org,...chosen.map(metric=>observed(org)?.[metric]??null),incomplete(org)?(org.missing?'Пропусков: '+org.missing:'Есть строки без суммы'):'Полные данные']);
 UI.table($('chartOrgTotals'),'amount',chartInterval(s),['Подразделение',...chosen.map(m=>titles[m]),'Полнота данных'],raw.map(row=>[row[0],...row.slice(1,-1).map((n,i)=>fmt(n,chosen[i])),row.at(-1)]),{title:'Итог по подразделениям за диапазон',totalRow:false,csvRows:raw,
  headerHelp:['Подразделение из выбранного списка.',...chosen.map(m=>
   (m==='average'?'Общая сумма / суммарное количество (объём)':'Итоговый показатель «'+titles[m]+'»')+' за выбранный диапазон. При пропусках итог учитывает доступные данные.'),
   'Полнота отчётов этого подразделения за диапазон. При неполных данных показано число пропущенных периодов.'],
  csvContext:chartExport(s,isCurrent,'organizationTotals',chosen)});
 $('chartHelp').textContent='Подразделений: '+s.organizations.length+'. Учитываются '+(all?'все выполненные работы.':'отмеченные работы и группы.')+' Прочерк означает неполные данные. Отношение суммы к количеству рассчитывается при положительном количестве. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[trends.granularity]||'по периодам')+'.'+(rawSeries.some(series=>series.points.some(incomplete))?partialChartNote:'');
}
async function configure(snapshot){
 if(!snapshot){ticket++;nameTicket++;invalidateScope();clear('overviewAlerts','Подготовка данных…');clear('categories');return;}
 const mine=++ticket,revision=snapshot.revision;workTotal=null;++workCountTicket;invalidateScope();
 if(lastRevision!==null&&lastRevision!==revision){selectionReady=false;allNames=false;selectedRows.clear();}lastRevision=revision;
 filters();const kind=value('periodType')||'week',source=value('unifiedSource')||'daily';
 if(kind==='year'&&source!=='daily'){$('unifiedSource').value='daily';UI.notice('Для календарного года используются дневные отчёты.');}
 try{const actualSource=value('unifiedSource'),reportType=actualSource==='daily'?'day':kind,options=await api.unifiedPeriodOptions(revision,reportType,actualSource==='daily'?kind:null,null,1);if(!valid(mine,revision))return;
  for(const id of ['overviewPeriod','rangeStart','rangeEnd']){const old=$(id).dataset.periodType,reset=old!==kind||!$(id).value;UI.setPeriodInput(id,kind,reset?(options.max||new Date().toISOString().slice(0,10)):undefined);}
  if(!options.total_rows){scope=null;for(const id of ['overviewKpis','overviewInsights','overviewOrganizations'])clear(id);clear('overviewAlerts','Для выбранного источника нет отчётов. Загрузите данные или выберите другой источник.');$('printSummary').disabled=true;void names();if(actualSource==='daily'&&kind==='week')void suggestWeeklyWhenDailyEmpty(revision,mine);return;}
  nameIndex=0;seriesIndex=0;await calculate();
 }catch(error){if(valid(mine,revision)){scope=null;clear('overviewAlerts','Не удалось открыть периоды. '+errorText(error));}}
}
function print(){const captured=printCapture;if(!captured||captured.revision!==UI.revision){UI.notice('Дождитесь загрузки сводки.',true);return;}const root=$('printReport');root.replaceChildren();root.append(UI.el('h1','Сводка выполненных работ'),UI.el('p','Период: '+sourceRangeLabel(captured.scope)),UI.el('p','Подразделения: '+captured.scope.organizations.join(', ')),UI.el('p','Дата подготовки: '+new Date().toLocaleDateString('ru-RU')));
 const add=(title,headers,rows)=>{root.append(UI.el('h2',title));const table=UI.el('table'),head=UI.el('thead'),tr=UI.el('tr');for(const label of headers)tr.append(UI.el('th',label));head.append(tr);table.append(head);const body=UI.el('tbody');for(const values of rows){const line=UI.el('tr');for(const v of values)line.append(UI.el('td',v));body.append(line);}table.append(body);root.append(table);};
 add('Основные показатели',['Показатель','Текущий результат','Относительно базового периода'],Object.keys(titles).map(metric=>[titles[metric],fmt(observed(captured.summary.after)?.[metric],metric),overviewDifference(overviewValue(captured.summary.after,metric),overviewBase(captured.summary,metric),metric)]));
 if(!captured.summary.after.complete||!captured.summary.before.complete)root.append(UI.el('p','Сравнение учитывает доступные отчёты. Пропуски не считаются нулём.'));
 const current=captured.summary.after;
 const averageNotice=window.analysisSummary.averageNote(current);
 if(averageNotice)root.append(UI.el('p',averageNotice));
 if(!current.complete&&current.available_reports>0)add('Частичный итог по доступным отчётам',['Исходных отчётов','Сумма','Количество / объём','Сумма / количество (объём)'],[[String(current.available_reports),fmt(current.available_total.amount,'amount'),fmt(current.available_total.count,'count'),fmt(current.available_total.average,'average')]]);
 if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)root.append(UI.el('p','В исходных отчётах есть замечания. Некорректных: '+current.invalid_reports+', дубликатов: '+current.duplicate_issues+', перекрытий: '+current.overlap_issues+', с замечаниями импорта: '+current.import_warning_reports+'. Суммы относятся к распознанной детализации; перед использованием проверьте контроль качества данных.'));
 add('По подразделениям',['Подразделение','Сумма','Количество / объём'],captured.organizations.map(org=>[org.org,fmt(observed(org)?.amount,'amount'),fmt(observed(org)?.count,'count')]));
 for(const metric of ['amount','count']){const entries=(captured.result['top_'+metric]||[]).filter(row=>Number.isFinite(row[metric]?.delta)&&row[metric].delta!==0).slice(0,5);if(entries.length)add('Крупнейшие изменения: '+titles[metric],['Услуга или группа','Было','Стало','Изменение'],entries.map(row=>[window.analysisSummary.itemLabel(row),fmt(row[metric].base,metric),fmt(row[metric].current,metric),changeValue(row[metric].delta,metric).value]));}
 root.append(UI.el('p','Единицы суммы и количества / объёма необходимо сверять с исходными Excel. Дробное количество может означать объём, а не число работ.'),UI.el('p','Сводка сформирована локально. Группы учитываются без двойного подсчёта. Отсутствие отчёта не считается нулём. Рост показателя сам по себе не означает улучшение эффективности.'));window.print();
}
function init(){if(initialized)return;initialized=true;state.selectedMetrics=state.selectedMetrics||new Set(['amount','count']);state.chartMetrics=state.chartMetrics||new Set(['amount','count']);state.chart='bar';
 for(const id of ['periodType','unifiedSource'])$(id).addEventListener('change',()=>{invalidateScope();void configure(UI.snapshot);});
 for(const id of ['overviewMode','overviewPeriod','rangeStart','rangeEnd'])$(id).addEventListener('change',()=>{seriesIndex=0;void calculate();});
 $('analysisBasePeriod').addEventListener('change',()=>{$('analysisBasePeriod').dataset.userSelected='true';seriesIndex=0;void calculate();});
 for(const id of ['chartBreakdown','chartOrgScope','chartSeriesLayout','hideEmptyPeriods','chartOrientation'])$(id).addEventListener('change',()=>{seriesIndex=0;void renderCharts();});
 $('workViewMode').addEventListener('change',()=>{void renderCharts();});
 document.querySelectorAll('[data-chart]').forEach(button=>button.addEventListener('click',()=>{state.chart=button.dataset.chart;seriesIndex=0;void renderCharts();}));
 $('categorySearch').addEventListener('input',()=>{nameTicket++;window.sqlRowScroll?.reset($('categories'));if(searchTimer)clearTimeout(searchTimer);searchTimer=setTimeout(()=>{searchTimer=null;void names();},160);});
 $('selectAll').addEventListener('click',()=>{allNames=true;selectedRows.clear();void changeWorks(true);});
 $('clearAll').addEventListener('click',()=>{allNames=false;selectedRows.clear();void changeWorks(true);});
 $('selectTop').addEventListener('click',async()=>{if(await top())await changeWorks(true);});
 $('advancedAnalytics').addEventListener('toggle',()=>{if($('advancedAnalytics').open)void renderCharts();});$('printSummary').addEventListener('click',print);
 UI.onSnapshot(configure);UI.onView('dashboard',()=>{if(!scope&&UI.snapshot)void configure(UI.snapshot);});
}
// Apply the shared shell filters in one revision. Dispatching several native change
// events here would publish transient mixtures of old and new date boundaries.
function applyReferenceScope(shared){
 if(!shared||shared.revision!==UI.revision){lastReferenceMetric=null;++ticket;invalidateScope();return;}
 const kind=shared.displayType;
 $('periodType').value=kind;$('unifiedSource').value=shared.source;$('overviewMode').value=shared.overviewMode;
 for(const [id,date] of [['rangeStart',shared.from],['rangeEnd',shared.to],['overviewPeriod',shared.from],['analysisBasePeriod',shared.baseFrom]])UI.setPeriodInput(id,kind,date);
 $('analysisBasePeriod').dataset.userSelected='true';
 state.selectedOrganizations=new Set(shared.organizations);
 if(window.referenceDetail?.active!=='overviewPanel'||lastReferenceMetric!==shared.metric)state.selectedMetrics=new Set([shared.metric]);
 lastReferenceMetric=shared.metric;
 selectedRows=new Map(shared.nameRows.map(row=>[row.key,row]));
 allNames=shared.allNames;selectionReady=true;seriesIndex=0;nameIndex=0;
 filters();selectionCount();return calculate(shared);
}
window.unifiedAnalytics=Object.freeze({init,applyReferenceScope,get displayedOverviewScope(){return displayedOverviewScope;},get displayedChartScope(){return displayedChart?.scope||null;}});
})();
