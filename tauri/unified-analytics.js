// Dashboard presentation uses the original DOM and pure drawing helpers.
// SQLite returns metadata pages and bounded aggregates, never report JSON.
(function(){
'use strict';
const UI=window.unifiedUI,O=window.unifiedOriginal,api=UI.api,$=UI.$;
const titles={amount:'Сумма',count:'Количество / объём',average:'Сумма / количество (объём)'};
let initialized=false,scope=null,ticket=0,nameTicket=0,chartTicket=0,selectionReady=false,allNames=false,totalNames=0;
let selectedRows=new Map(),namePage=null,namePositions=[null],seriesPositions=[null],printCapture=null,lastRevision=null,searchTimer=null;
const state=UI.state;
function clear(id,text=''){const node=$(id);if(!node)return;node.replaceChildren();if(text)node.append(UI.el('p',text,'muted small'));}
function value(id){return $(id)?.value||'';}
 function orientation(){return value('chartOrientation')==='horizontal'?'horizontal':'vertical';}
function invalidateScope(){scope=null;printCapture=null;chartTicket++;UI.publishScope(null);}
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
 $('chartsContainer').hidden=state.chart==='table';$('dataTable').hidden=state.chart!=='table';
 document.querySelectorAll('[data-chart]').forEach(button=>button.classList.toggle('active',button.dataset.chart===state.chart));
}
function filters(){O.renderMultiFilter('orgFilter',(UI.snapshot?.org_names||[]).map(n=>[n,n]),state.selectedOrganizations,()=>calculate());O.renderMultiFilter('metric',[['amount','Сумма'],['count','Количество / объём']],state.selectedMetrics,()=>{if(printCapture?.revision===UI.revision)drawOverview(printCapture.result,printCapture.organizations,printCapture.scope,printCapture.before);publish();renderCharts();});O.renderMultiFilter('chartMetrics',[['amount','Сумма'],['count','Количество / объём'],['average','Сумма / количество (объём)']],state.chartMetrics,()=>renderCharts());}
function publish(){if(!scope)return;scope={...scope,organizations:orgs(),allNames,selectedNames:allNames?[]:[...selectedRows.values()].map(r=>r.name),selectedKeys:allNames?[]:[...selectedRows.keys()],nameRows:[...selectedRows.values()]};state.selectedCategories=new Set(scope.selectedNames);UI.publishScope(scope);}
function selectionCount(){state.selectedCategories=new Set([...selectedRows.values()].map(r=>r.name));$('categoryCount').textContent=allNames?'Все ('+totalNames+')':selectedRows.size+' из '+totalNames;}
function pager(root,positions,next,run,label){const bar=UI.el('div',null,'change-pagination'),previous=UI.el('button','← Предыдущая','button secondary'),forward=UI.el('button','Следующая →','button secondary');previous.type=forward.type='button';previous.disabled=positions.length<2;forward.disabled=!next;previous.addEventListener('click',()=>run(positions.slice(0,-1)));forward.addEventListener('click',()=>{if(next)run([...positions,next]);});bar.append(previous,UI.el('span',label||('Страница '+positions.length),'muted small'),forward);root.append(bar);}
async function names(positions=[null]){
 if(!Number.isSafeInteger(UI.revision))return;const mine=++nameTicket,revision=UI.revision;clear('categories','Загрузка названий…');
 try{const page=await api.unifiedNameOptions(revision,value('categorySearch'),positions.at(-1),50);if(mine!==nameTicket||revision!==UI.revision)return;namePage=page;namePositions=positions;if(!value('categorySearch'))totalNames=page.total_rows;
  clear('categories');const root=$('categories');
  for(const row of page.rows){const label=UI.el('label',null,'category-row'),box=UI.el('input');box.type='checkbox';box.checked=allNames||selectedRows.has(row.key);box.disabled=allNames;box.addEventListener('change',()=>{if(box.checked){if(selectedRows.size>=50){box.checked=false;UI.notice('Можно отметить до 50 отдельных работ. «Выбрать все» включает все работы без ограничения.',true);return;}selectedRows.set(row.key,row);}else selectedRows.delete(row.key);selectionCount();publish();seriesPositions=[null];renderCharts();});label.append(box,UI.el('span',row.name+(row.kind==='group'?' (группа)':'')));root.append(label);}
  if(!page.rows.length)root.append(UI.el('p','Совпадений нет.','muted small'));
  if(allNames)root.append(UI.el('p','Выбраны все работы, включая следующие страницы. Чтобы отметить отдельные работы, нажмите «Снять всё».','muted small'));
  pager(root,positions,page.next,names,'Страница '+positions.length+' · '+page.total_rows+' названий');selectionCount();
 }catch(error){if(mine===nameTicket&&revision===UI.revision)clear('categories','Не удалось загрузить названия. '+errorText(error));}
}
async function top(eager=false){
 if(!scope)return false;const mine=ticket,s={...scope,organizations:orgs()};
 try{const result=await api.nativeAnalysisRanking(s.revision,s.type,s.from,s.to,s.organizations,null,chosenMetrics()[0]||'amount','all',null,null);if(!valid(mine,s.revision))return false;
  if(!result.complete){if(!eager)UI.notice('Для рейтинга нужны полные отчёты выбранных подразделений за весь диапазон.',true);return false;}
  selectedRows=new Map(result.rows.slice(0,8).map(row=>[row.key,{key:row.key,name:row.name,kind:row.kind}]));allNames=false;selectionReady=true;selectionCount();return true;
 }catch(error){if(!eager&&valid(mine,s.revision))UI.notice(errorText(error),true);return false;}
}
function overviewDifference(now,before,m){if(!Number.isFinite(now)||!Number.isFinite(before))return 'Недостаточно данных для сравнения';if(now===before)return 'Без изменений';const delta=now-before,change=(delta>0?'+':'')+fmt(delta,m);return change+' ('+(before===0?'процент не рассчитан':fmt(delta/Math.abs(before)*100,'count')+'%')+')';}
function dateLabel(iso){return /^\d{4}-\d{2}-\d{2}$/.test(iso)?iso.slice(8,10)+'.'+iso.slice(5,7)+'.'+iso.slice(0,4):iso;}
function rangeLabel(from,to){return dateLabel(from)+(from===to?'':' — '+dateLabel(to));}
async function renderMissingMatrix(root,s){
 const days=(Date.parse(s.to)-Date.parse(s.from))/86400000+1;
 if(s.type==='day'&&days>31){root.replaceChildren(UI.el('p','Для просмотра пропусков по дням выберите диапазон до 31 дня.','muted small'));return;}
 root.replaceChildren(UI.el('p','Загрузка таблицы пропусков…','muted small'));
 try{
  const result=await api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations,null,s.type==='day'?'day':null,[]);
  if(UI.revision!==s.revision||printCapture?.scope!==s)return;
  const series=result.series||[],periods=series[0]?.points||[];
  if(!periods.length){root.replaceChildren(UI.el('p','Нет отчётов выбранного типа. Проверьте источник.','muted small'));return;}
  const table=UI.el('table',null,'table coverage-matrix'),header=UI.el('tr');
  header.append(UI.el('th','Подразделение'));
  for(const period of periods)header.append(UI.el('th',dateLabel(period.start)));
  const head=UI.el('thead');head.append(header);table.append(head);
  const body=UI.el('tbody');
  for(const name of s.organizations){
   const serie=series.find(x=>x.organization===name),line=UI.el('tr');
   line.append(UI.el('th',name));
   for(let i=0;i<periods.length;i++){
    const found=serie?.points?.[i],complete=found?.missing===0;
    const cell=UI.el('td',complete?'Есть':'Нет',complete?'coverage-ok':'coverage-missing');
    cell.title=complete?'Полный отчёт':'Нет полного отчёта — не означает ноль';line.append(cell);
   }body.append(line);
  }
  table.append(body);const scroll=UI.el('div',null,'coverage-matrix-scroll');scroll.append(table);
  root.replaceChildren(UI.el('p','«Нет» означает отсутствие полного отчёта, не нулевое значение.','muted small'),scroll);
 }catch(err){if(UI.revision===s.revision&&printCapture?.scope===s)root.replaceChildren(UI.el('p','Не удалось показать пропуски: '+errorText(err),'warn small'));}
}
async function suggestWeeklyReports(s){
 if(s.source!=='daily'||s.displayType!=='week')return;
 try{
  const options=await api.unifiedPeriodOptions(s.revision,'week',null,null,500);
  if(UI.revision!==s.revision||printCapture?.scope!==s||!options.rows?.some(p=>p.start>=s.from&&p.start<=s.to))return;
  const message=UI.el('p','В выбранном диапазоне есть недельные отчёты. Сейчас выбраны дневные, поэтому полнота проверяется по каждому дню.','muted small');
  const button=UI.el('button','Переключить на недельные отчёты','button secondary');
  button.type='button';button.addEventListener('click',()=>{if(UI.revision!==s.revision)return;$('unifiedSource').value='imported';$('periodType').value='week';void configure(UI.snapshot);});
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
   $('unifiedSource').value='imported';$('periodType').value='week';
   if(weekly.max){for(const id of ['overviewPeriod','rangeStart','rangeEnd'])UI.setPeriodInput(id,'week',weekly.max);}
   void configure(UI.snapshot);
  });
  message.append(button);$('overviewAlerts').append(message);
 }catch{/* Подсказка не заменяет обязательную проверку источников. */}
}
function drawOverview(result,organizations,s,before){
 for(const id of ['overviewKpis','overviewInsights','overviewOrganizations','overviewAlerts'])clear(id);
 const summary=result.summary,current=summary.after,previous=summary.before,root=$('overviewKpis'),insights=$('overviewInsights');
 $('overviewPeriodText').textContent=rangeLabel(s.from,s.to)+' · '+s.organizations.join(', ');$('printSummary').disabled=!current.periods;
 for(const [metric,label] of [['amount','Сумма выполненных работ'],['count','Количество / объём'],['average','Сумма / количество (объём)']]){const card=UI.el('div',null,'kpi'),now=current.total[metric],old=previous.total[metric];card.append(UI.el('span',label),UI.el('strong',fmt(now,metric)),UI.el('small',overviewDifference(now,old,metric),Number.isFinite(now)&&Number.isFinite(old)?now>old?'numeric-rise':now<old?'numeric-fall':'':''));root.append(card);}
 const orgCard=UI.el('div',null,'kpi');orgCard.append(UI.el('span','Выбрано подразделений'),UI.el('strong',String(s.organizations.length)),UI.el('small','Из '+UI.snapshot.org_names.length+' подразделений'));root.append(orgCard);
 for(const item of organizations){const line=UI.el('div',null,'overview-unit');line.append(UI.el('span',item.org),UI.el('strong',item.complete?fmt(item.total.amount,'amount')+' · '+'Количество / объём: '+fmt(item.total.count,'count'):'Нет полного отчёта (пропусков '+item.missing+')',item.complete?'':'quality-bad'));$('overviewOrganizations').append(line);}
 const flagged=!current.complete||Boolean(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports);
 if(!current.complete){
  const alert=UI.el('div',null,'overview-alert warn');
  alert.setAttribute('role','status');
  alert.append(UI.el('p','Неполные данные: '+current.missing+' пропусков (подразделение × '+({day:'день',week:'неделя',month:'месяц'}[s.type]||'период')+') за '+rangeLabel(s.from,s.to)+'.'));
  const details=UI.el('details',null,'coverage-details'),matrix=UI.el('div');
  details.append(UI.el('summary','Показать где'),matrix);
  let loaded=false;details.addEventListener('toggle',()=>{if(details.open&&!loaded){loaded=true;void renderMissingMatrix(matrix,s);}});
  alert.append(details);
  if(current.available_reports>0)alert.append(UI.el('p','Частичный итог по '+current.available_reports+' доступным отчётам: сумма '+fmt(current.available_total.amount,'amount')+' · количество / объём '+fmt(current.available_total.count,'count')+'.','muted small'));
  $('overviewAlerts').append(alert);
 }
 if(flagged){
  const info=UI.el('div',null,'overview-alert');
  if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)
   info.append(UI.el('p','Есть замечания к отчётам: некорректных '+current.invalid_reports+', повторов '+current.duplicate_issues+', пересечений '+current.overlap_issues+', с предупреждениями импорта '+current.import_warning_reports+'.','muted small'));
  const inspect=UI.el('button','Проверить данные','button primary');inspect.type='button';
  inspect.addEventListener('click',()=>UI.activate('upload'));info.append(inspect);$('overviewAlerts').append(info);
 }
 if(!current.complete||!previous.complete)insights.append(UI.el('p',!current.complete?'Для выводов нужны полные отчёты выбранных подразделений.':'Предыдущий интервал заполнен не полностью; процентные изменения не оцениваются.','muted small'));
 else {insights.append(UI.el('p','Сравнение с базовым периодом: '+rangeLabel(before.from,before.to)+'.','muted small'));for(const metric of ['amount','count']){const line=UI.el('div',null,'overview-line');line.append(UI.el('strong',titles[metric]+': '+overviewDifference(current.total[metric],previous.total[metric],metric)));insights.append(line);}const metric=state.selectedMetrics.has('amount')?'amount':'count';for(const row of (result['top_'+metric]||[]).filter(row=>row[metric]?.delta!==0).slice(0,2)){const m=row[metric],line=UI.el('div',null,'overview-line');line.append(UI.el('strong',row.name+': '+(m.delta>0?'+':'')+fmt(m.delta,metric),m.delta>0?'numeric-rise':'numeric-fall'),UI.el('small','Один из крупнейших вкладов в изменение '+titles[metric].toLowerCase()));insights.append(line);}}
 insights.append(UI.el('p','Единицы суммы и количества / объёма необходимо сверять с источником. Дробное количество может означать объём, а не число работ.','muted small'));
 printCapture={revision:s.revision,scope:s,before,summary,organizations,result};
}
async function calculate(){
 const mine=++ticket,revision=UI.revision;invalidateScope();$('printSummary').disabled=true;for(const id of ['overviewKpis','overviewInsights','overviewOrganizations','overviewAlerts'])clear(id,'Загрузка…');
 try{if(!Number.isSafeInteger(revision))throw Error('Дождитесь подготовки данных.');layout();const displayType=value('periodType'),source=value('unifiedSource'),range=UI.range(displayType,value('rangeStart'),value('rangeEnd'),source);if(!range||!orgs().length)throw Error('Выберите корректный период и подразделения.');const s={...range,revision,organizations:orgs(),selectedNames:[],selectedKeys:[],allNames:false,nameRows:[]},before=chosenBaseRange(s);if(!before)throw Error('Базовый период выходит за допустимые календарные границы.');s.baseFrom=before.from;s.baseTo=before.to;s.overviewMode=value('overviewMode');scope=s;
  const [overview,organizations]=await Promise.all([api.nativeAnalysisCompare(revision,s.type,before.from,before.to,s.from,s.to,s.organizations,null,null),api.nativeAnalysisOrganizations(revision,s.type,s.from,s.to,s.organizations,null,null)]);
  if(!valid(mine,revision))return;drawOverview(overview,organizations,s,before);
   if(!overview.summary.after.complete)void suggestWeeklyReports(s);
  if(!selectionReady){const ranked=await top(true);if(!valid(mine,revision))return;if(!ranked){const page=await api.unifiedNameOptions(revision,'',null,8);if(!valid(mine,revision))return;selectedRows=new Map(page.rows.map(r=>[r.key,r]));}selectionReady=true;}
  publish();selectionCount();void names(namePositions);if($('advancedAnalytics').open)await renderCharts();
 }catch(error){if(valid(mine,revision)){scope=null;for(const id of ['overviewKpis','overviewInsights','overviewOrganizations'])clear(id);clear('overviewAlerts','Не удалось загрузить обзор. '+errorText(error));for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id);}}
}
function descriptors(points,granularity,s){const kind=granularity==='period'?s.displayType:granularity;return points.map(p=>{const start=p.start.length===7?p.start+'-01':p.start;return {type:kind,start,end:O.periodEnd(start,kind)};});}
function confirmedZero(point){return point&&point.missing===0&&point.total?.count===0&&point.total?.amount===0;}
function plotSection(root,metric,title=titles[metric]){const section=UI.el('section',null,'metric-chart'),plot=UI.el('div',null,'metric-plot');section.append(UI.el('h3',title),plot);root.append(section);return plot;}
async function renderCharts(positions=seriesPositions){
 layout();if(!scope||scope.revision!==UI.revision)return;const mine=++chartTicket,s={...scope,organizations:orgs(),selectedNames:allNames?[]:[...selectedRows.values()].map(r=>r.name),allNames},revision=s.revision;
 const isCurrent=()=>chartTicket===mine&&UI.revision===revision;
 for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id,'Загрузка…');
 try{if(!allNames&&!selectedRows.size)throw Error('Выберите хотя бы одну работу или группу.');
  if(value('chartBreakdown')==='organizations'){await departmentCharts(s,isCurrent);return;}
  const pageSize=state.chart==='table'?50:8;let rows,next=null,localOffset=0;
  if(allNames){const page=await api.unifiedNameOptions(revision,'',positions.at(-1),pageSize);if(!isCurrent())return;rows=page.rows;next=page.next;}
  else {localOffset=Number(positions.at(-1)||0);rows=[...selectedRows.values()].slice(localOffset,localOffset+pageSize);next=localOffset+pageSize<selectedRows.size?localOffset+pageSize:null;}
  if(!rows.length){clear('chartsContainer','Нет работ для графика.');clear('dataTable','Нет работ для таблицы.');clear('chartOrgTotals');return;}
  const [totals,...series]=await Promise.all([api.nativeAnalysisTimeline(revision,s.type,s.from,s.to,s.organizations,null,s.source==='daily'?s.displayType:null,s.selectedNames),...rows.slice(0,4).map(row=>api.nativeWorkTimeline(revision,s.type,s.from,s.to,s.organizations,null,row.key,null,s.source==='daily'?s.displayType:null,s.selectedNames))]);
  if(!isCurrent())return;
  for(let i=4;i<rows.length;i+=4){const batch=await Promise.all(rows.slice(i,i+4).map(row=>api.nativeWorkTimeline(revision,s.type,s.from,s.to,s.organizations,null,row.key,null,s.source==='daily'?s.displayType:null,s.selectedNames)));if(!isCurrent())return;series.push(...batch);}
  if(series.some(item=>item.granularity!==totals.granularity||item.points.length!==totals.points.length||item.points.some((p,i)=>p.start!==totals.points[i].start)))throw Error('Несовпадающие границы SQL-серий. Повторите расчёт.');
  const keep=totals.points.map((p,i)=>!$('hideEmptyPeriods').checked||!confirmedZero(p)||series.some(item=>!confirmedZero(item.points[i]))),points=totals.points.filter((p,i)=>keep[i]),periods=descriptors(points,totals.granularity,s);
  const data=points.map((p,i)=>{if(p.missing!==0)return null;const index=totals.points.indexOf(p);return new Map(rows.map((row,j)=>[row.key,series[j].points[index].missing===0?series[j].points[index].total:{count:null,amount:null,average:null}]));});
  for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id);
  for(const metric of chosenMetrics())if(state.chart==='table'){
   const raw=[['Итого по выбранным работам',...points.map(p=>p.missing===0?p.total[metric]:null)],...rows.map(row=>[row.name,...data.map(map=>map?.get(row.key)?.[metric]??null)])];
   UI.table($('dataTable'),metric,s.from+' — '+s.to+' · прочерк означает неполные данные.',['Работа / группа',...periods.map(O.displayPeriod)],raw.map(row=>[row[0],...row.slice(1).map(n=>fmt(n,metric))]),{csvRows:raw});
  }else UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric),{chart:state.chart,orientation:orientation(),categories:rows.map(r=>r.key),data,label:key=>rows.find(r=>r.key===key).name,pointNotice:i=>points[i].missing?'Неполные данные: пропусков '+points[i].missing:''});
  seriesPositions=positions;const root=state.chart==='table'?$('dataTable'):$('chartsContainer');pager(root,positions,next,p=>{seriesPositions=p;renderCharts(p);},'Страница '+positions.length+' · '+rows.length+' работ из '+(allNames?totalNames:selectedRows.size));
  $('chartHelp').textContent='Выбрано подразделений: '+s.organizations.length+' из '+UI.snapshot.org_names.length+'. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[totals.granularity]||'по периодам')+'. Пропуски не считаются нулём.';
 }catch(error){if(isCurrent()){for(const id of ['chartsContainer','dataTable'])clear(id,'Не удалось построить аналитику. '+errorText(error));clear('chartOrgTotals');}}
}
async function departmentCharts(base,isCurrent){
 const all=value('chartOrgScope')==='all',s={...base,selectedNames:all?[]:base.selectedNames},chosen=[...state.chartMetrics];
 if(!all&&!base.allNames&&!selectedRows.size)throw Error('Нет отмеченных работ. Выберите работы или учитывайте все выполненные работы.');
 const [trends,organizations]=await Promise.all([api.nativeOrganizationTrends(s.revision,s.type,s.from,s.to,s.organizations,null,s.source==='daily'?s.displayType:null,s.selectedNames),api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations,null,s.selectedNames)]);
 if(!isCurrent())return;for(const id of ['chartsContainer','dataTable','chartOrgTotals'])clear(id);
 const rawSeries=trends.series||[],first=rawSeries[0]?.points||[],keep=first.map((p,i)=>!$('hideEmptyPeriods').checked||!rawSeries.every(series=>confirmedZero(series.points[i]))),points=first.filter((p,i)=>keep[i]),periods=descriptors(points,trends.granularity,s);
 const data=first.map((p,i)=>new Map(rawSeries.map(series=>[series.organization,series.points[i]?.missing===0?series.points[i].total:{count:null,amount:null,average:null}]))).filter((p,i)=>keep[i]);
 if(state.chart==='table'){
  const raw=s.organizations.flatMap(org=>chosen.map(metric=>[org,titles[metric],...data.map(map=>map.get(org)?.[metric]??null)]));
  UI.table($('dataTable'),'amount',s.from+' — '+s.to,['Подразделение','Показатель',...periods.map(O.displayPeriod)],raw.map((row,i)=>[...row.slice(0,2),...row.slice(2).map(n=>fmt(n,chosen[i%chosen.length]))]),{title:'Все выбранные показатели по подразделениям',totalRow:false,csvRows:raw});
 }else if(chosen.length>1&&value('chartSeriesLayout')==='combined'&&orientation()!=='horizontal')UI.drawDepartments(periods,s.organizations,chosen,data,plotSection($('chartsContainer'),'amount','Совместное сравнение: '+s.organizations.length+' подразделений × '+chosen.length+' показателя'));
 else for(const metric of chosen)UI.drawChart(periods,new Set(s.organizations),metric,plotSection($('chartsContainer'),metric),{chart:state.chart,orientation:orientation(),seriesType:'organizations',categories:s.organizations,data});
 const raw=organizations.map(org=>[org.org,...chosen.map(metric=>org.total[metric]),org.complete?'Полные данные':'Пропусков: '+org.missing]);
 UI.table($('chartOrgTotals'),'amount',s.from+' — '+s.to,['Подразделение',...chosen.map(m=>titles[m]),'Полнота данных'],raw.map(row=>[row[0],...row.slice(1,-1).map((n,i)=>fmt(n,chosen[i])),row.at(-1)]),{title:'Итог по подразделениям за диапазон',totalRow:false,csvRows:raw});
 $('chartHelp').textContent='Сравниваются '+s.organizations.length+' подразделений. Учитываются '+(all?'все выполненные работы.':'отмеченные работы и группы.')+' Прочерк — неполные данные; средняя сумма рассчитана из общей суммы и количества. Данные сгруппированы '+({day:'по дням',week:'по неделям',month:'по месяцам',year:'по годам',period:'по выбранным периодам'}[trends.granularity]||'по периодам')+'.';
}
async function configure(snapshot){
 if(!snapshot){ticket++;nameTicket++;invalidateScope();clear('overviewAlerts','Подготовка данных…');for(const id of ['overviewKpis','overviewInsights','overviewOrganizations','chartsContainer','dataTable','chartOrgTotals','categories'])clear(id);$('printSummary').disabled=true;return;}
 const mine=++ticket,revision=snapshot.revision;invalidateScope();
 if(lastRevision!==null&&lastRevision!==revision){selectionReady=false;allNames=false;selectedRows.clear();}lastRevision=revision;
 filters();const kind=value('periodType')||'week',source=value('unifiedSource')||'daily';
 if(kind==='year'&&source!=='daily'){$('unifiedSource').value='daily';UI.notice('Для календарного года используются дневные отчёты.');}
 try{const actualSource=value('unifiedSource'),reportType=actualSource==='daily'?'day':kind,options=await api.unifiedPeriodOptions(revision,reportType,actualSource==='daily'?kind:null,null,1);if(!valid(mine,revision))return;
  for(const id of ['overviewPeriod','rangeStart','rangeEnd']){const old=$(id).dataset.periodType,reset=old!==kind||!$(id).value;UI.setPeriodInput(id,kind,reset?(options.max||new Date().toISOString().slice(0,10)):undefined);}
  if(!options.total_rows){scope=null;for(const id of ['overviewKpis','overviewInsights','overviewOrganizations','chartsContainer','dataTable','chartOrgTotals'])clear(id);clear('overviewAlerts','Для выбранного источника нет отчётов. Загрузите данные или выберите другой источник.');$('printSummary').disabled=true;void names();if(actualSource==='daily'&&kind==='week')void suggestWeeklyWhenDailyEmpty(revision,mine);return;}
  namePositions=[null];seriesPositions=[null];await calculate();
 }catch(error){if(valid(mine,revision)){scope=null;clear('overviewAlerts','Не удалось открыть периоды. '+errorText(error));}}
}
function print(){const captured=printCapture;if(!captured||captured.revision!==UI.revision){UI.notice('Дождитесь загрузки сводки.',true);return;}const root=$('printReport');root.replaceChildren();root.append(UI.el('h1','Сводка выполненных работ'),UI.el('p','Период: '+rangeLabel(captured.scope.from,captured.scope.to)),UI.el('p','Подразделения: '+captured.scope.organizations.join(', ')),UI.el('p','Дата подготовки: '+new Date().toLocaleDateString('ru-RU')));
 const add=(title,headers,rows)=>{root.append(UI.el('h2',title));const table=UI.el('table'),head=UI.el('thead'),tr=UI.el('tr');for(const label of headers)tr.append(UI.el('th',label));head.append(tr);table.append(head);const body=UI.el('tbody');for(const values of rows){const line=UI.el('tr');for(const v of values)line.append(UI.el('td',v));body.append(line);}table.append(body);root.append(table);};
 add('Основные показатели',['Показатель','Текущий результат','Относительно предыдущего интервала'],Object.keys(titles).map(metric=>[titles[metric],fmt(captured.summary.after.total[metric],metric),overviewDifference(captured.summary.after.total[metric],captured.summary.before.total[metric],metric)]));
 if(!captured.summary.after.complete)root.append(UI.el('p','ВНИМАНИЕ: отчёты некоторых выбранных подразделений неполны. Пропуски не считаются нулём.'));
 const current=captured.summary.after;
 if(!current.complete&&current.available_reports>0)add('Частичный итог по доступным отчётам',['Исходных отчётов','Сумма','Количество / объём','Сумма / количество (объём)'],[[String(current.available_reports),fmt(current.available_total.amount,'amount'),fmt(current.available_total.count,'count'),fmt(current.available_total.average,'average')]]);
 if(current.invalid_reports||current.duplicate_issues||current.overlap_issues||current.import_warning_reports)root.append(UI.el('p','В исходных отчётах есть замечания. Некорректных: '+current.invalid_reports+', дубликатов: '+current.duplicate_issues+', перекрытий: '+current.overlap_issues+', с замечаниями импорта: '+current.import_warning_reports+'. Суммы относятся к распознанной детализации; перед использованием проверьте контроль качества данных.'));
 add('По подразделениям',['Подразделение','Сумма','Количество / объём'],captured.organizations.map(org=>[org.org,fmt(org.total.amount,'amount'),fmt(org.total.count,'count')]));
 if(current.complete&&captured.summary.before.complete){for(const metric of ['amount','count']){const entries=(captured.result['top_'+metric]||[]).filter(row=>Number.isFinite(row[metric]?.delta)&&row[metric].delta!==0).slice(0,5);if(entries.length)add('Крупнейшие изменения: '+titles[metric],['Услуга или группа','Было','Стало','Изменение'],entries.map(row=>[row.name+(row.kind==='group'?' (группа)':''),fmt(row[metric].base,metric),fmt(row[metric].current,metric),(row[metric].delta>0?'+':'')+fmt(row[metric].delta,metric)]));}}
 else root.append(UI.el('p','Причины изменений не рассчитаны: нужны полные текущий и предыдущий интервалы.'));
 root.append(UI.el('p','Единицы суммы и количества / объёма необходимо сверять с исходными Excel. Дробное количество может означать объём, а не число работ.'),UI.el('p','Сводка сформирована локально. Группы учитываются без двойного подсчёта. Отсутствие отчёта не считается нулём. Рост показателя сам по себе не означает улучшение эффективности.'));window.print();
}
function init(){if(initialized)return;initialized=true;state.selectedMetrics=state.selectedMetrics||new Set(['amount','count']);state.chartMetrics=state.chartMetrics||new Set(['amount','count']);state.chart='bar';
 for(const id of ['periodType','unifiedSource'])$(id).addEventListener('change',()=>{invalidateScope();void configure(UI.snapshot);});
 for(const id of ['overviewMode','overviewPeriod','rangeStart','rangeEnd'])$(id).addEventListener('change',()=>{seriesPositions=[null];void calculate();});
 $('analysisBasePeriod').addEventListener('change',()=>{$('analysisBasePeriod').dataset.userSelected='true';seriesPositions=[null];void calculate();});
 for(const id of ['chartBreakdown','chartOrgScope','chartSeriesLayout','hideEmptyPeriods','chartOrientation'])$(id).addEventListener('change',()=>{seriesPositions=[null];void renderCharts();});
 document.querySelectorAll('[data-chart]').forEach(button=>button.addEventListener('click',()=>{state.chart=button.dataset.chart;seriesPositions=[null];void renderCharts();}));
 $('categorySearch').addEventListener('input',()=>{nameTicket++;if(searchTimer)clearTimeout(searchTimer);searchTimer=setTimeout(()=>{searchTimer=null;void names();},160);});
 $('selectAll').addEventListener('click',()=>{allNames=true;selectedRows.clear();selectionReady=true;seriesPositions=[null];selectionCount();publish();void names(namePositions);void renderCharts();});
 $('clearAll').addEventListener('click',()=>{allNames=false;selectedRows.clear();selectionReady=true;seriesPositions=[null];selectionCount();publish();void names(namePositions);void renderCharts();});
 $('selectTop').addEventListener('click',async()=>{if(await top()){seriesPositions=[null];publish();void names(namePositions);void renderCharts();}});
 $('advancedAnalytics').addEventListener('toggle',()=>{if($('advancedAnalytics').open)void renderCharts();});$('printSummary').addEventListener('click',print);
 UI.onSnapshot(configure);UI.onView('dashboard',()=>{if(!scope&&UI.snapshot)void configure(UI.snapshot);});
}
window.unifiedAnalytics=Object.freeze({init});
})();
