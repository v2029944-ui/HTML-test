// Dashboard presentation uses the original DOM and pure drawing helpers.
// SQLite returns metadata pages and bounded aggregates, never report JSON.
(function(){
'use strict';
const UI=window.unifiedUI,O=window.unifiedOriginal,api=UI.api,$=UI.$;
const titles=window.analysisSummary.titles;
let initialized=false,scope=null,ticket=0,nameTicket=0,chartTicket=0,selectionReady=false,allNames=false,totalNames=0;
let displayedOverviewScope=null,displayedChart=null;
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
function selectionCount(){state.selectedCategories=new Set([...selectedRows.values()].map(r=>r.name));$('categoryCount').textContent=allNames?'Все ('+totalNames+')':selectedRows.size+' из '+totalNames;}
function pager(root,index,run,options){
 const pages=Math.max(1,Math.ceil(options.total/options.size));
 const go=i=>{if(options.current()&&Number.isSafeInteger(i)&&i>=0&&i<pages&&i!==index)return run(i);return false;};
 if(window.sqlRowScroll?.attach(root,{index,size:options.size,total:options.total,count:options.count,hasNext:index+1<pages,label:options.label,controlsOnly:root.id==='chartsContainer',direct:true,go}))return;
 const bar=UI.el('div',null,'change-pagination'),previous=UI.el('button','← Предыдущая','button secondary'),forward=UI.el('button','Следующая →','button secondary');previous.type=forward.type='button';previous.disabled=index===0;forward.disabled=index+1>=pages;previous.addEventListener('click',()=>go(index-1));forward.addEventListener('click',()=>go(index+1));bar.append(previous,UI.el('span','Показано '+options.count+' из '+options.total,'muted small'),forward);root.append(bar);
}
async function names(index=0,paging=false){
 if(!Number.isSafeInteger(UI.revision))return;const mine=++nameTicket,revision=UI.revision;clear('categories','Загрузка названий…',!paging);
 try{const page=await api.unifiedNameOptionsAtPage(revision,value('categorySearch'),index,50);if(mine!==nameTicket||revision!==UI.revision)return;namePage=page;nameIndex=index;if(!value('categorySearch'))totalNames=page.total_rows;
  clear('categories','',false);const root=$('categories');
  for(const row of page.rows){const label=UI.el('label',null,'category-row'),box=UI.el('input');box.type='checkbox';box.checked=allNames||selectedRows.has(row.key);box.disabled=allNames;box.addEventListener('change',()=>{if(box.checked){if(selectedRows.size>=50){box.checked=false;UI.notice('Можно отметить до 50 отдельных работ. «Выбрать все» включает все работы без ограничения.',true);return;}selectedRows.set(row.key,row);}else selectedRows.delete(row.key);selectionCount();publish();seriesIndex=0;renderCharts();});label.append(box,UI.el('span',row.name+(row.kind==='group'?' (группа)':'')));root.append(label);}
  if(!page.rows.length)root.append(UI.el('p','Совпадений нет.','muted small'));
  if(allNames)root.append(UI.el('p','Выбраны все работы, включая строки, которые не отображаются сейчас. Чтобы отметить отдельные работы, нажмите «Снять всё».','muted small'));
  pager(root,index,i=>names(i,true),{size:50,total:page.total_rows,count:page.rows.length,label:'Названия работ',current:()=>mine===nameTicket&&revision===UI.revision});selectionCount();
 }catch(error){if(mine===nameTicket&&revision===UI.revision)clear('categories','Не удалось загрузить названия. '+errorText(error));}
}
async function top(eager=false){
 if(!scope)return false;const mine=ticket,s={...scope,organizations:orgs()};
 try{const result=await api.nativeAnalysisRanking(s.revision,s.type,s.from,s.to,s.organizations,null,chosenMetrics()[0]||'amount','all',null,null);if(!valid(mine,s.revision))return false;
  if(!result.complete){if(!eager)UI.notice('Для рейтинга нужны полные отчёты выбранных подразделений за весь диапазон.',true);return false;}
  selectedRows=new Map(result.rows.slice(0,8).map(row=>[row.key,{key:row.key,name:row.name,kind:row.kind}]));allNames=false;selectionReady=true;selectionCount();return true;
 }catch(error){if(!eager&&valid(mine,s.revision))UI.notice(errorText(error),true);return false;}
}
function overviewDifference(now,before,m){if(!Number.isFinite(now)||!Number.isFinite(before))return 'Недостаточно данных для сравнения';if(now===before)return 'Без изменений';const delta=now-before,change=(delta>0?'+':'')+fmt(delta,m);return change+' ('+(before===0?'процент не рассчитан':(delta/Math.abs(before)*100).toLocaleString('ru-RU',{maximumFractionDigits:1})+'%')+')';}
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
 for(const [metric,label] of [['amount','Сумма выполненных работ'],['count','Количество / объём'],['average','Сумма / количество (объём)']]){const card=UI.el('div',null,'kpi'),now=current.total[metric],old=previous.total[metric];card.append(UI.el('span',label),UI.el('strong',fmt(now,metric)),UI.el('small',overviewDifference(now,old,metric),Number.isFinite(now)&&Number.isFinite(old)?now>old?'numeric-rise':now<old?'numeric-fall':'':''));root.append(card);}
 const orgCard=UI.el('div',null,'kpi');orgCard.append(UI.el('span','Выбрано подразделений'),UI.el('strong',String(s.organizations.length)),UI.el('small','Из '+UI.snapshot.org_names.length+' подразделений'));root.append(orgCard);
 for(const item of organizations){const line=UI.el('div',null,'overview-unit');line.append(UI.el('span',item.org),UI.el('strong',item.complete?fmt(item.total.amount,'amount')+' · '+'Количество / объём: '+fmt(item.total.count,'count'):'Нет полного отчёта (пропусков '+item.missing+')',item.complete?'':'quality-bad'));$('overviewOrganizations').append(line);}
 const flagged=!current.complete||Boolean(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports);
 if(!current.complete){
  const alert=UI.el('div',null,'overview-alert warn');
  alert.setAttribute('role','status');
  alert.append(UI.el('p','Неполные данные: '+current.missing+' пропусков (подразделение × '+({day:'день',week:'неделя',month:'месяц'}[s.type]||'период')+') за '+sourceRangeLabel(s)+'.'));
  alert.append(coverageDetails('Показать где',s));
  if(current.available_reports>0)alert.append(UI.el('p','Частичный итог по '+current.available_reports+' доступным отчётам: сумма '+fmt(current.available_total.amount,'amount')+' · количество / объём '+fmt(current.available_total.count,'count')+'.','muted small'));
  $('overviewAlerts').append(alert);
 }
 if(current.complete)$('overviewAlerts').append(coverageDetails('Качество данных по подразделениям и периодам',s));
 if(flagged){
  const info=UI.el('div',null,'overview-alert');
  if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)
   info.append(UI.el('p','Есть замечания к отчётам: некорректных '+current.invalid_reports+', повторов '+current.duplicate_issues+', пересечений '+current.overlap_issues+', с предупреждениями импорта '+current.import_warning_reports+'.','muted small'));
  const inspect=UI.el('button','Проверить данные','button primary');inspect.type='button';
  inspect.addEventListener('click',()=>window.referenceDesktop.openPanel('upload',inspect));info.append(inspect);$('overviewAlerts').append(info);
 }
 insights.append(UI.el('p','Сравнение с базовым периодом: '+sourceRangeLabel(before)+'.','muted small'));
 if(!current.complete||!previous.complete)insights.append(UI.el('p',!current.complete?'Для выводов нужны полные отчёты выбранных подразделений.':'Базовый период заполнен не полностью; процентные изменения не оцениваются.','muted small'));
 else {for(const metric of ['amount','count']){const line=UI.el('div',null,'overview-line');line.append(UI.el('strong',titles[metric]+': '+overviewDifference(current.total[metric],previous.total[metric],metric)));insights.append(line);}const metric=state.selectedMetrics.has('amount')?'amount':'count';for(const row of (result['top_'+metric]||[]).filter(row=>row[metric]?.delta!==0).slice(0,2)){const m=row[metric],line=UI.el('div',null,'overview-line');line.append(UI.el('strong',row.name+': '+(m.delta>0?'+':'')+fmt(m.delta,metric),m.delta>0?'numeric-rise':'numeric-fall'),UI.el('small','Один из крупнейших вкладов в изменение '+titles[metric].toLowerCase()));insights.append(line);}}
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
  if(!selectionReady){const ranked=await top(true);if(!valid(mine,revision))return;if(!ranked){const page=await api.unifiedNameOptionsAtPage(revision,'',0,8);if(!valid(mine,revision))return;selectedRows=new Map(page.rows.map(r=>[r.key,r]));}selectionReady=true;}
  if(!shared)publish();
  selectionCount();void names(nameIndex);if($('advancedAnalytics').open)await renderCharts();
 }catch(error){if(valid(mine,revision)){invalidateScope();clear('overviewAlerts','Не удалось загрузить обзор. '+errorText(error));}}
 finally{if(valid(mine,revision))for(const id of overviewResults){$(id).inert=false;$(id).setAttribute('aria-busy','false');}}
}
function descriptors(points,granularity,s){const kind=granularity==='period'?s.displayType:granularity;return points.map(p=>{const start=p.start.length===7?p.start+'-01':p.start;const end=O.periodEnd(start,kind);return {type:kind,start,actualStart:s.source==='daily'&&s.from>start?s.from:start,end:s.source==='daily'&&s.to<end?s.to:end};});}
const allPeriodsHidden='Фильтр скрыл все подтверждённые нулевые периоды в этой выборке. Снимите фильтр нулевых периодов, чтобы увидеть их.';
function confirmedZero(point){return point&&point.missing===0&&point.has_activity!==true&&point.total?.count===0&&point.total?.amount===0;}
function chartInterval(s){const to=s.source==='daily'?s.to:O.periodEnd(s.to,s.displayType);return O.displayDay(s.from)+(to===s.from?'':' — '+O.displayDay(to));}
function plotSection(root,metric,title=titles[metric]){const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');section.append(UI.el('h3',title),plot);root.append(section);return plot;}
function chartExport(scope,isCurrent,table,metrics){
 if(window.referenceDetail?.active!=='chartPanel')return null;
 const req=UI.request(scope),options={table,metrics:[...metrics],hideEmpty:$('hideEmptyPeriods').checked};
 const valid=()=>isCurrent()&&window.referenceDetail?.active==='chartPanel';
 return {export:async()=>{
  if(!valid())throw Error('Параметры графика изменились. Повторите расчёт.');
  const path=await api.chooseCsvPath('dinamika-'+table+'-'+scope.from+'-'+scope.to+'.csv');
  if(!path)return;
  if(!valid())throw Error('Данные изменились. Повторите расчёт.');
  await api.referenceTrendCsv(req,options,path);
 }};
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
  const pageSize=state.chart==='table'?50:8;let rows,next=null,localOffset=0,seriesTotal=selectedRows.size;
  if(allNames){const page=await api.unifiedNameOptionsAtPage(revision,'',index,pageSize,true);if(!isCurrent())return;rows=page.rows;next=page.next;seriesTotal=page.total_rows;}
  else {localOffset=index*pageSize;rows=[...selectedRows.values()].slice(localOffset,localOffset+pageSize);next=localOffset+pageSize<selectedRows.size?localOffset+pageSize:null;}
  if(!rows.length){commitView();clear('chartsContainer','Нет работ для графика.');clear('dataTable','Нет работ для таблицы.');clear('chartOrgTotals');return;}
  const [totals,...series]=await Promise.all([api.nativeAnalysisTimeline(revision,s.type,s.from,s.to,s.organizations,null,s.source==='daily'?s.displayType:null,s.selectedNames),...rows.slice(0,4).map(row=>api.nativeWorkTimeline(revision,s.type,s.from,s.to,s.organizations,null,row.key,null,s.source==='daily'?s.displayType:null,s.selectedNames))]);
  if(!isCurrent())return;
  for(let i=4;i<rows.length;i+=4){const batch=await Promise.all(rows.slice(i,i+4).map(row=>api.nativeWorkTimeline(revision,s.type,s.from,s.to,s.organizations,null,row.key,null,s.source==='daily'?s.displayType:null,s.selectedNames)));if(!isCurrent())return;series.push(...batch);}
  if(series.some(item=>item.granularity!==totals.granularity||item.points.length!==totals.points.length||item.points.some((p,i)=>p.start!==totals.points[i].start)))throw Error('Несовпадающие границы SQL-серий. Повторите расчёт.');
  const keep=totals.points.map((p,i)=>!$('hideEmptyPeriods').checked||!confirmedZero(p)||series.some(item=>!confirmedZero(item.points[i]))),points=totals.points.filter((p,i)=>keep[i]),periods=descriptors(points,totals.granularity,s);
  const hiddenPeriods=descriptors(totals.points.filter((_,i)=>!keep[i]),totals.granularity,s);
  const data=points.map((p,i)=>{if(p.missing!==0)return null;const index=totals.points.indexOf(p);return new Map(rows.map((row,j)=>[row.key,series[j].points[index].missing===0?series[j].points[index].total:{count:null,amount:null,average:null}]));});
  const root=state.chart==='table'?$('dataTable'):$('chartsContainer');
  for(const id of chartResults)clear(id,'',$(id)!==root);
  const filteredAll=hiddenPeriods.length>0&&!points.length;
  if(filteredAll)clear(state.chart==='table'?'dataTable':'chartsContainer',allPeriodsHidden,false);
  for(const metric of chosenMetrics())if(state.chart==='table'){
   const raw=[['Итого по выбранным работам',...points.map(p=>p.missing===0?p.total[metric]:null)],...rows.map(row=>[row.name,...data.map(map=>map?.get(row.key)?.[metric]??null)])];
   UI.table($('dataTable'),metric,chartInterval(s)+' · прочерк означает неполные данные.',['Работа / группа',...periods.map(O.displayPeriod)],raw.map(row=>[row[0],...row.slice(1).map(n=>fmt(n,metric))]),{csvRows:raw,csvContext:chartExport(s,isCurrent,'works',[metric])});
  }else if(!filteredAll)UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric,titles[metric]+' · '+chartInterval(s)),{chart:state.chart,orientation:orientation(),hiddenPeriods,categories:rows.map(r=>r.key),data,label:key=>rows.find(r=>r.key===key).name,pointNotice:i=>points[i].missing?'Неполные данные: пропусков '+points[i].missing:''});
  commitView();seriesIndex=index;pager(root,index,i=>{seriesIndex=i;return renderCharts(i,true);},{size:pageSize,total:seriesTotal,count:rows.length,label:'Работы на графике',current:isCurrent});
  $('chartHelp').textContent='Выбрано подразделений: '+s.organizations.length+' из '+UI.snapshot.org_names.length+'. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[totals.granularity]||'по периодам')+'. Пропуски не считаются нулём.';
 }catch(error){if(isCurrent()){for(const id of ['chartsContainer','dataTable'])clear(id,'Не удалось построить аналитику. '+errorText(error));clear('chartOrgTotals');clear('chartHelp');commitView();}}
 finally{if(isCurrent())chartBusy(false);}
}
async function departmentCharts(base,isCurrent){
 const all=value('chartOrgScope')==='all',s={...base,selectedNames:all?[]:base.selectedNames},chosen=[...state.chartMetrics];
 if(!all&&!base.allNames&&!selectedRows.size)throw Error('Нет отмеченных работ. Выберите работы или учитывайте все выполненные работы.');
 const [trends,organizations]=await Promise.all([api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(0,8),null,s.source==='daily'?s.displayType:null,s.selectedNames),api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,s.selectedNames)]);
 if(!isCurrent())return;
 for(let offset=8;offset<s.organizations.length;offset+=8){
  const page=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations.slice(offset,offset+8),null,s.source==='daily'?s.displayType:null,s.selectedNames);
  if(!isCurrent())return;
  trends.series.push(...page.series);
 }
 for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id);
 const rawSeries=trends.series||[],first=rawSeries[0]?.points||[],keep=first.map((p,i)=>!$('hideEmptyPeriods').checked||!rawSeries.every(series=>confirmedZero(series.points[i]))),points=first.filter((p,i)=>keep[i]),periods=descriptors(points,trends.granularity,s);
 const hiddenPeriods=descriptors(first.filter((_,i)=>!keep[i]),trends.granularity,s);
 const data=first.map((p,i)=>new Map(rawSeries.map(series=>[series.organization,series.points[i]?.missing===0?series.points[i].total:{count:null,amount:null,average:null}]))).filter((p,i)=>keep[i]);
 function pointNotice(i){
  const index=first.indexOf(points[i]);
  const missing=rawSeries.filter(series=>series.points[index]?.missing>0).map(series=>series.organization);
  return missing.length?'Нет полного отчёта: '+missing.join(', ')+'.':'';
 }
 const filteredAll=hiddenPeriods.length>0&&!points.length;
 if(filteredAll)clear(state.chart==='table'?'dataTable':'chartsContainer',allPeriodsHidden,false);
 if(state.chart==='table'){
  const raw=s.organizations.flatMap(org=>chosen.map(metric=>[org,titles[metric],...data.map(map=>map.get(org)?.[metric]??null)]));
  UI.table($('dataTable'),'amount',chartInterval(s),['Подразделение','Показатель',...periods.map(O.displayPeriod)],raw.map((row,i)=>[...row.slice(0,2),...row.slice(2).map(n=>fmt(n,chosen[i%chosen.length]))]),{title:'Все выбранные показатели по подразделениям',totalRow:false,csvRows:raw,csvContext:chartExport(s,isCurrent,'organizations',chosen)});
 }else if(!filteredAll&&chosen.length>1&&value('chartSeriesLayout')==='combined')UI.drawDepartments(periods,s.organizations,chosen,data,plotSection($('chartsContainer'),'amount',O.departmentChartTitle(s.organizations.length,chosen.length)+' · '+chartInterval(s)),{orientation:orientation()});
 else if(!filteredAll)for(const metric of chosen)UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric,titles[metric]+' · '+chartInterval(s)),{chart:state.chart,orientation:orientation(),hiddenPeriods,seriesType:'organizations',categories:s.organizations,data,pointNotice});
 const raw=organizations.map(org=>[org.org,...chosen.map(metric=>org.total[metric]),org.complete?'Полные данные':'Пропусков: '+org.missing]);
 UI.table($('chartOrgTotals'),'amount',chartInterval(s),['Подразделение',...chosen.map(m=>titles[m]),'Полнота данных'],raw.map(row=>[row[0],...row.slice(1,-1).map((n,i)=>fmt(n,chosen[i])),row.at(-1)]),{title:'Итог по подразделениям за диапазон',totalRow:false,csvRows:raw,
  headerHelp:['Подразделение из выбранного списка.',...chosen.map(m=>
   (m==='average'?'Общая сумма / суммарное количество (объём)':'Итоговый показатель «'+titles[m]+'»')+' за весь выбранный диапазон. При пропуске отчёта значение недоступно.'),
   'Полнота отчётов этого подразделения за диапазон. При неполных данных показано число пропущенных периодов.'],
  csvContext:chartExport(s,isCurrent,'organizationTotals',chosen)});
 $('chartHelp').textContent='Подразделений: '+s.organizations.length+'. Учитываются '+(all?'все выполненные работы.':'отмеченные работы и группы.')+' Прочерк — неполные данные; средняя сумма рассчитана из общей суммы и количества. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[trends.granularity]||'по периодам')+'.';
}
async function configure(snapshot){
 if(!snapshot){ticket++;nameTicket++;invalidateScope();clear('overviewAlerts','Подготовка данных…');clear('categories');return;}
 const mine=++ticket,revision=snapshot.revision;invalidateScope();
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
 add('Основные показатели',['Показатель','Текущий результат','Относительно базового периода'],Object.keys(titles).map(metric=>[titles[metric],fmt(captured.summary.after.total[metric],metric),overviewDifference(captured.summary.after.total[metric],captured.summary.before.total[metric],metric)]));
 if(!captured.summary.after.complete)root.append(UI.el('p','ВНИМАНИЕ: отчёты некоторых выбранных подразделений неполны. Пропуски не считаются нулём.'));
 const current=captured.summary.after;
 if(!current.complete&&current.available_reports>0)add('Частичный итог по доступным отчётам',['Исходных отчётов','Сумма','Количество / объём','Сумма / количество (объём)'],[[String(current.available_reports),fmt(current.available_total.amount,'amount'),fmt(current.available_total.count,'count'),fmt(current.available_total.average,'average')]]);
 if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)root.append(UI.el('p','В исходных отчётах есть замечания. Некорректных: '+current.invalid_reports+', дубликатов: '+current.duplicate_issues+', перекрытий: '+current.overlap_issues+', с замечаниями импорта: '+current.import_warning_reports+'. Суммы относятся к распознанной детализации; перед использованием проверьте контроль качества данных.'));
 add('По подразделениям',['Подразделение','Сумма','Количество / объём'],captured.organizations.map(org=>[org.org,fmt(org.total.amount,'amount'),fmt(org.total.count,'count')]));
 if(current.complete&&captured.summary.before.complete){for(const metric of ['amount','count']){const entries=(captured.result['top_'+metric]||[]).filter(row=>Number.isFinite(row[metric]?.delta)&&row[metric].delta!==0).slice(0,5);if(entries.length)add('Крупнейшие изменения: '+titles[metric],['Услуга или группа','Было','Стало','Изменение'],entries.map(row=>[row.name+(row.kind==='group'?' (группа)':''),fmt(row[metric].base,metric),fmt(row[metric].current,metric),(row[metric].delta>0?'+':'')+fmt(row[metric].delta,metric)]));}}
 else root.append(UI.el('p','Причины изменений не рассчитаны: нужны полные текущий и базовый интервалы.'));
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
 $('selectAll').addEventListener('click',()=>{allNames=true;selectedRows.clear();selectionReady=true;seriesIndex=0;selectionCount();publish();void names(nameIndex);void renderCharts();});
 $('clearAll').addEventListener('click',()=>{allNames=false;selectedRows.clear();selectionReady=true;seriesIndex=0;selectionCount();publish();void names(nameIndex);void renderCharts();});
 $('selectTop').addEventListener('click',async()=>{if(await top()){seriesIndex=0;publish();void names(nameIndex);void renderCharts();}});
 $('advancedAnalytics').addEventListener('toggle',()=>{if($('advancedAnalytics').open)void renderCharts();});$('printSummary').addEventListener('click',print);
 UI.onSnapshot(configure);UI.onView('dashboard',()=>{if(!scope&&UI.snapshot)void configure(UI.snapshot);});
}
// Apply the shared shell filters in one revision. Dispatching several native change
// events here would publish transient mixtures of old and new date boundaries.
function applyReferenceScope(shared){
 if(!shared||shared.revision!==UI.revision){++ticket;invalidateScope();return;}
 const kind=shared.displayType;
 $('periodType').value=kind;$('unifiedSource').value=shared.source;$('overviewMode').value=shared.overviewMode;
 for(const [id,date] of [['rangeStart',shared.from],['rangeEnd',shared.to],['overviewPeriod',shared.from],['analysisBasePeriod',shared.baseFrom]])UI.setPeriodInput(id,kind,date);
 $('analysisBasePeriod').dataset.userSelected='true';
 state.selectedOrganizations=new Set(shared.organizations);
 state.selectedMetrics=new Set([shared.metric]);
 selectedRows=new Map(shared.nameRows.map(row=>[row.key,row]));
 allNames=shared.allNames;selectionReady=true;seriesIndex=0;nameIndex=0;
 filters();selectionCount();return calculate(shared);
}
window.unifiedAnalytics=Object.freeze({init,applyReferenceScope,get displayedOverviewScope(){return displayedOverviewScope;},get displayedChartScope(){return displayedChart?.scope||null;}});
})();
