'use strict';
// The original markup/styles are canonical; only its full-history controller is retired.
const fs=require('node:fs'),path=require('node:path');
const dateIds=['overviewPeriod','rangeStart','rangeEnd','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','structureBase','structureCurrent','usualPeriod','groupAuditFrom','groupAuditTo','changesFrom','changesTo'];
const rangeBoundaries={rangeStart:'start',rangeEnd:'end',baseRangeStart:'start',baseRangeEnd:'end',currentRangeStart:'start',currentRangeEnd:'end',comparisonRangeStart:'start',comparisonRangeEnd:'end',groupAuditFrom:'start',groupAuditTo:'end',changesFrom:'start'};
function buildUnified(source){
 if(/analiz-unified-sql|tauri-bridge\.js/.test(source))throw Error('Настольные скрипты уже присутствуют.');
 const inline=[...source.matchAll(/<script>([\s\S]*?)<\/script>/g)];
 const presentation=inline.find(item=>item[1].includes('const state =')&&item[1].includes('window.appReady=main();'));
 if(!presentation)throw Error('Original interface presentation markers changed');
 let script=presentation[1].replace('window.appReady=main();','').replace(/\r\n/g,'\n');
 // Fresh desktop installations follow the reference theme. Saved preferences,
 // including an explicit system theme, continue through the original helper.
 const appearanceDefault="legacy?(savedTheme==='neon'?'dark':'light'):'system'";
 if(!script.includes(appearanceDefault))throw Error('Desktop appearance default changed');
 script=script.replace(appearanceDefault,"legacy?(savedTheme==='neon'?'dark':'light'):(!savedTheme&&!savedPalette&&!savedDesign?'dark':'system')");
 // The standalone browser UI stays unchanged; only the installed SQL presentation
 // must not describe a fractional quantity/volume as a count of completed jobs.
 const metricMarker="function metricTitle(metric){return {count:'Количество',amount:'Сумма',average:'Средняя сумма на одну работу'}[metric]||metric;}";
 if(!script.includes(metricMarker))throw Error('Original quantity labels changed');
 script=script.replace(metricMarker,"function metricTitle(metric){return {count:'Количество / объём',amount:'Сумма',average:'Средняя сумма на единицу'}[metric]||metric;}");
 // The original renderer predates calendar-year rollups. Adapt only generated
 // presentation helpers; the independent browser source remains unchanged.
 const rankingColor="fill.style.setProperty('--rank-color',seriesColor(row.name));";
 if(!script.includes(rankingColor))throw Error('Original ranking color marker changed');
 script=script.replace(rankingColor,"fill.style.setProperty('--rank-color','var(--accent)');");
 const rankingRows='const defaultHideZeroRankingRows=false;';
 if(!script.includes(rankingRows))throw Error('Original ranking filter marker changed');
 script=script.replace(rankingRows,'const defaultHideZeroRankingRows=true;');
 const rankingHint="+'). Для динамики выберите несколько периодов.'";
 if(!script.includes(rankingHint))throw Error('Original ranking hint changed');
 script=script.replace(rankingHint,"+').'+(options.rankingHint===false?'':' Для динамики выберите несколько периодов.')");
 const chartValues="return value===null?[]:[value||0];";
 if(!script.includes(chartValues))throw Error('Original chart value marker changed');
 script=script.replace(chartValues,"return Number.isFinite(value)?[value]:[];");
 const chartPoint="if(raw===null){flush();return;}";
 if(!script.includes(chartPoint))throw Error('Original chart point marker changed');
 script=script.replace(chartPoint,"if(!Number.isFinite(raw)){flush();return;}");
 // Use the actual desktop plot width in SVG coordinates. Scaling a fixed
 // 760px viewBox up to a wide window also magnifies its height and labels.
 const plotGeometry=[
  ['height=phone?310:350','height=phone?375:420'],
  ['Math.max(760,periods.length*Math.max(110,bar?shown.length*18:110))','Math.max(root.clientWidth||0,left+right+periods.length*Math.max(90,bar?shown.length*18:0))'],
  ['+bottom:phone?325:370','+bottom:phone?390:450'],
  ['top=27,bottom=horizontal?20:80','top=horizontal?40.5:27,bottom=horizontal?30:80'],
  ['const rowHeight=series.length*18+22;', 'const rowHeight=series.length*27+33;'],
  ['top+shown.length*(series.length*18+22)+bottom', 'top+shown.length*(series.length*27+33)+bottom'],
  ['top+i*rowHeight+index*18+3', 'top+i*rowHeight+index*27+3'],
  ['top+i*rowHeight+index*18+14', 'top+i*rowHeight+index*27+14'],
  ['Math.max(800,periods.length*Math.max(116,bar?series.length*19:116))','Math.max(root.clientWidth||0,left+right+periods.length*Math.max(90,bar?series.length*19:0))']
 ];
 for(const [original,replacement] of plotGeometry){
  if(!script.includes(original))throw Error('Original chart geometry changed: '+original);
  script=script.replace(original,replacement);
 }
 // An inline sentence per date collides with its neighbors. Keep the
 // period-specific explanation in a keyboard-accessible disclosure below.
 const pointNotice=script.indexOf('    const customNotice=chartPointNotice(data[i]');
 const noteStart=script.indexOf("      const note=svgEl('text'",pointNotice);
 const noteEnd=script.indexOf('svg.append(note);',noteStart);
 const appendChart='  svg.append(...numberMarks);root.append(svg);';
 if(pointNotice<0||noteStart<0||noteEnd<0||!script.includes(appendChart)||!script.includes('  const missingRanges=[];'))throw Error('Original chart notice markup changed');
 script=script.slice(0,noteStart)+`      const description=displayPeriod(p)+' · '+message;
      const note=svgEl('text',{x:px(i),y:height-13,'text-anchor':'middle',fill:'var(--warning)','font-size':13,
        tabindex:0,'aria-label':description,'aria-describedby':'chartTooltip'},'!');
      note.dataset.chartTooltip=description;
      note.append(svgEl('title',{},description));
      periodNotices.push(description);
      `+script.slice(noteEnd);
 script=script.replace('  const missingRanges=[];','  const missingRanges=[],periodNotices=[];')
  .replace(appendChart,`  svg.append(...numberMarks);window.unifiedChartPlot(root,svg);
  if(periodNotices.length){
    const details=elem('details',null,'chart-period-notices'),list=elem('ul');
    details.append(elem('summary','! Замечания по периодам ('+periodNotices.length+')'),list);
    for(const description of periodNotices)list.append(elem('li',description));
    root.append(details);
  }`);
 const comparisonGap='const breaks=chartTimelineGaps(periods),gapStarts=';
 const comparisonAxis="plotPeriodLabel(p));";
 const comparisonTitle="'aria-label':'Динамика по периодам'";
 for(const fragment of [comparisonGap,comparisonAxis,comparisonTitle])
  if(!script.includes(fragment))throw Error('Original comparison plot labels changed');
 script=script.replace(comparisonGap,'const breaks=options.comparison?[]:chartTimelineGaps(periods),gapStarts=')
  .replace(comparisonAxis,'options.periodLabels?.[i]||plotPeriodLabel(p));')
  .replace(comparisonTitle,"'aria-label':options.comparison?'Сравнение периодов':'Динамика по периодам'");
 const departmentPlot='  svg.append(...numbers);root.append(svg);';
 if(!script.includes(departmentPlot))throw Error('Original department chart plot changed');
 script=script.replace(departmentPlot,`  svg.append(...numbers);
  window.unifiedChartPlot(root,svg);
  const periodPager=root.querySelector('[data-sql-scroll-host="chart-periods"]');
  if(periodPager)root.append(periodPager);`);
 const seriesStart=script.indexOf('  if(pages>1){',script.indexOf('function drawChart('));
 const seriesEnd=script.indexOf('  const seriesColor=',seriesStart);
 if(seriesStart<0||seriesEnd<0||!script.slice(seriesStart,seriesEnd).includes('Видимые серии графика'))throw Error('Original chart series control changed');
 script=script.slice(0,seriesStart)+`  if(pages>1){
    window.unifiedChartPager(root,'chart-series',{index:page,size:limit,count:shown.length,total:groups.length,
      label:'Серии графика',go:index=>{options.seriesPage=index;drawChart(periods,orgs,metric,root,options);}});
  }
`+script.slice(seriesEnd);
 script=script.replace("  const restoreSeriesFocus=root.contains(document.activeElement)&&document.activeElement?.matches('.chart-series-control select');\n",'');
 const horizontalMarker="  const data=options.data||periods.map(p=>reportHasOrganizations(p,orgs)?summary(p,orgs).by:null);\n  if(periods.length===1){";
 if(!script.includes(horizontalMarker))throw Error('Original chart geometry changed');
 script=script.replace(horizontalMarker,"  const data=options.data||periods.map(p=>reportHasOrganizations(p,orgs)?summary(p,orgs).by:null);\n  if(bar&&options.orientation==='horizontal'){\n    if(typeof window.unifiedHorizontalBars!=='function')throw Error('Не загружена горизонтальная диаграмма.');\n    window.unifiedHorizontalBars(periods,shown,data,metric,root,options,seriesColor,()=>drawChart(periods,orgs,metric,root,options),seriesNumbers);\n    return;\n  }\n  if(periods.length===1){");
 script+=`
(function(){
 const original={periodEnd,periodTitle,periodScale,periodUnit,plotPeriodLabel,displayPeriod,shiftPeriod};
 periodEnd=(start,type)=>type==='year'?start.slice(0,4)+'-12-31':original.periodEnd(start,type);
 periodTitle=type=>type==='year'?'Год':original.periodTitle(type);
 periodScale=type=>type==='year'?'годы':original.periodScale(type);
 periodUnit=(type,count)=>type==='year'?ruPlural(count,'год','года','лет'):original.periodUnit(type,count);
 function partialLabel(period,compact){
   const label=day=>compact?displayDay(day).slice(0,5):displayDay(day);
   return label(period.actualStart)+(period.end===period.actualStart?'':(compact?'–':' — ')+label(period.end));
 }
 plotPeriodLabel=period=>period.partial?partialLabel(period,true):period.type==='year'?period.start.slice(0,4):original.plotPeriodLabel(period);
 displayPeriod=period=>period.partial?partialLabel(period,false):period.type==='year'?period.start.slice(0,4):original.displayPeriod(period);
 shiftPeriod=(type,start,steps)=>type==='year'?String(Number(start.slice(0,4))+steps).padStart(4,'0')+'-01-01':original.shiftPeriod(type,start,steps);
})();
`;
 // A single observation cannot form a line. Keep grouped department metrics in bars.
 const multiSingle="const scales=departmentScaleMax(series),bar=state.chart==='bar',horizontal=bar&&options.orientation==='horizontal';";
 if(!script.includes(multiSingle))throw Error('Original department chart mode changed');
 script=script.replace(multiSingle,"const scales=departmentScaleMax(series),bar=state.chart==='bar'||periods.length===1,horizontal=bar&&options.orientation==='horizontal';");
 // Distinct metric patterns remain legible at the compact 80% interface scale.
 const patternOld="const id='metric-pattern-'+(++metricPatternId),defs=svgEl('defs'),pattern=svgEl('pattern',{id,width:6,height:6,patternUnits:'userSpaceOnUse'});";
 const bgOld="pattern.append(svgEl('rect',{width:6,height:6,fill:'var(--surface)'}));";
 const marksOld="pattern.append(metric==='count'?svgEl('path',{d:'M-1 1L1 -1M0 6L6 0M5 7L7 5',stroke:color,'stroke-width':2}):svgEl('circle',{cx:3,cy:3,r:1.8,fill:color}));";
 for(const fragment of [patternOld,bgOld,marksOld])if(!script.includes(fragment))throw Error('Original metric pattern changed');
 script=script.replace(patternOld,"const id='metric-pattern-'+(++metricPatternId),defs=svgEl('defs'),pattern=svgEl('pattern',{id,width:8,height:8,patternUnits:'userSpaceOnUse'});");
 script=script.replace(bgOld,"pattern.append(svgEl('rect',{width:8,height:8,fill:'var(--surface)'}));");
 script=script.replace(marksOld,"pattern.append(metric==='count'?svgEl('path',{d:'M-2 2L2 -2M0 8L8 0M6 10L10 6',stroke:color,'stroke-width':2.4}):svgEl('circle',{cx:4,cy:4,r:2.2,fill:color}));");
 // Never allow a paged UI to reach an old whole-database writer by accident.
 script+='\nmain=wire=loadData=saveSettings=storeReports=storeAllData=saveImport=saveGroup=moveWork=saveOrganizations=importGroupFile=importBackupFile=()=>{throw Error("Полная загрузка истории и старые обработчики отключены.");};\n';
 script+='notice=(message,error)=>window.showDesktopNotice(message,error);\n';
 script+='window.unifiedOriginal=Object.freeze({state,$,el:elem,notice,fmt:formatMetric,fmtNumber,averageQuantityReason,chartNiceScale,chartTickLabel,drawChart,drawDepartments:drawDepartmentMultiChart,makeTable,tableCsvButton,initTheme,initChartTooltip,redrawCharts,renderMultiFilter,periodEnd,shiftPeriod,displayDay,displayPeriod,metricTitle,departmentChartTitle,chartRenders});\n';
 let html=source.replace(/<script>[\s\S]*?<\/script>/g,'');
 const historyControls=[
  ['usualWindow','Окон для среднего','Длина каждого окна равна анализируемому диапазону.'],
  ['regularityGap','Исторических периодов','Также порог признака «Долго без выполнений».']
 ];
 for(const [id,label,help] of historyControls){
  const select=new RegExp('<label>[^<]+<select id="'+id+'"[^>]*>[\\s\\S]*?</select></label>');
  if(!select.test(html))throw Error('Original historical period control changed: '+id);
  const baseline=id==='regularityGap'?'<label><input type="checkbox" id="regularityUseBaseline" checked> Сравнить с выбранным базовым периодом</label>':'';
  html=html.replace(select,`<div class="pattern-window"><label>${label}<input id="${id}" type="number" min="1" max="120" step="1" value="4" title="${help}"></label>
    <div class="button-row"><button id="${id}4" type="button" class="button secondary" aria-label="${label}: 4">4</button>
    <button id="${id}8" type="button" class="button secondary" aria-label="${label}: 8">8</button></div></div>${baseline}`);
 }
 html=html.replace('прошло 4 или 8 полных периодов подряд','прошло указанное число периодов подряд с доступными отчётами');

 const comparisonMetrics=/<fieldset class="multi-filter"><legend>Показатели сравнения<\/legend><div id="comparisonMetrics" class="multi-choices"><\/div><\/fieldset>/;
 const comparisonKpis='<div class="kpis" id="kpis"></div>';
 const comparisonMetricField=html.match(comparisonMetrics)?.[0];
 if(!comparisonMetricField||!html.includes(comparisonKpis))throw Error('Original comparison metric controls changed');
 html=html.replace(comparisonMetrics,'').replace(comparisonKpis,comparisonMetricField+'\n        '+comparisonKpis);
 const comparisonPlots=/<div id="comparisonTable"><\/div><div id="comparisonCharts"><\/div>\s*<details id="comparisonDynamics"[^>]*>[\s\S]*?<\/details>/;
 if(!comparisonPlots.test(html))throw Error('Original comparison charts changed');
 html=html.replace(comparisonPlots,`
        <section id="comparisonDynamics" class="extended-section" aria-labelledby="comparisonDynamicsHeading" hidden>
          <h3 id="comparisonDynamicsHeading">Динамика по периодам</h3>
          <p class="muted small">Пропуски разрывают линию графика.</p>
          <div id="comparisonDynamicsPlots"></div>
        </section>
        <div id="comparisonCharts" role="region" aria-label="Графики сравнения"></div>
        <div id="comparisonTable"></div>`);

 // The desktop controller supplies the entire selection caption.
 html=html.replace('Выбрано: <span id="categoryCount"', '<span id="categoryCount"');

 // Extend the shared preference only in the installed application.
 const desktopSizes=['80','90','standard','110','125','150'];
 script=script.replace("const UI_SIZES=['standard','large','xlarge'];", 'const UI_SIZES='+JSON.stringify(desktopSizes)+';')
  .replace("function uiSizePreference(value){","function uiSizePreference(value){if(value==='large')return '110';if(value==='xlarge')return '125';");
 const sizeSelect=/<select id="appearanceSize">[\s\S]*?<\/select>/;
 if(!sizeSelect.test(html))throw Error('Missing desktop scale selector');
 html=html.replace(sizeSelect,'<select id="appearanceSize">'+desktopSizes.map(size=>'<option value="'+size+'">'+(size==='standard'?'100':size)+'%</option>').join('')+'</select>');
 const importFields=/<label>Формат Excel<select id="importFormat">[\s\S]*?<\/label>\s*<label>Тип отчётов<select id="importType">[\s\S]*?<\/label>/;
 if(!importFields.test(html))throw Error('Original Excel period and layout fields changed');
 html=html.replace(importFields,`<label>Период отчёта<select id="importType"><option value="day">День</option><option value="week" selected>Неделя</option><option value="month">Месяц</option></select></label>
          <div class="import-layout-control"><label id="importLayoutLabel">Показатели в Excel<select id="importFormat" aria-describedby="importLayoutHint"><option value="">Выберите структуру…</option><option value="daily-wide">Единый список показателей</option><option value="legacy">Свои для каждого периода</option></select></label>
          <div id="importLayoutSummary" hidden><span>Показатели в Excel</span><strong id="importLayoutName"></strong><button id="editImportLayout" type="button" class="button secondary" aria-controls="importFormat">Изменить</button></div>
          <p id="importLayoutHint" class="muted small" aria-live="polite"></p></div>`);
 html=html.replace('Выберите отчётные листы. Итоги квартала или года проверяются отдельно и не становятся дополнительным днём. При неоднозначной шапке укажите год и ручные столбцы; пустые дни не заменяются нулями.', 'Приложение сверит значения с итогами Excel. Пустой период останется пропуском.');
 html=html.replace('</head>', '<style>#importLayoutSummary:not([hidden]){display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:center;gap:6px 8px}#importLayoutSummary span{grid-column:1/-1;font-weight:600}#importLayoutSummary strong{overflow-wrap:anywhere}#importLayoutHint{margin:8px 0 0}.import-layout-control{min-width:0}</style></head>');
 const metricMarkup=`<details id="additionalMetrics" class="panel group-section"><summary>Другие показатели — трудозатраты и план / факт</summary>
<p class="muted small">Независимые виды отчётов со своими полями и единицами измерения. Их показатели не прибавляются к суммам выполненных работ.</p>
<section><h3>Расчёт показателей</h3>
<div class="metric-controls">
<label>Вид отчёта<select id="metricKindSelect"></select></label>
<label>С<input id="metricKindFrom" type="date"></label>
<label>По<input id="metricKindTo" type="date"></label>
<button type="button" id="metricKindCalculate" class="button secondary">Рассчитать</button></div>
<p id="metricKindCount" class="muted small">Пустые даты при расчёте означают всю историю.</p>
<div id="metricKindTotals" class="metric-results"></div></section>
<section><h3>Импорт XLSX или JSON</h3>
<div class="metric-controls">
<label>Шаблон XLSX<select id="metricXlsxProfile"></select></label>
<button type="button" id="metricKindChooseXlsx" class="button secondary">Импорт XLSX</button>
<button type="button" id="metricKindChoose" class="button secondary">Импорт JSON</button>
</div>
<p class="muted small">Для XLSX укажите обе даты периода в полях «С» и «По» выше и выберите шаблон. Перед записью проверьте значения: импорт не заменяет существующие отчёты. Лимит XLSX — 30 МиБ, JSON — 8 МиБ, не более 10 000 строк в отчёте.</p>
<p id="metricKindFile" class="muted small">Файл ещё не выбран.</p>
<div id="metricKindPreview" class="metric-results"></div>
<p id="metricKindStatus" class="status muted" role="status" aria-live="polite">Загрузка видов отчётов…</p><button type="button" id="metricKindApply" class="button primary" disabled>Подтвердить импорт</button></section></details>`;
 if(!html.includes('<section id="upload" class="view">'))throw Error('Original Excel upload panel changed');
 html=html.replace('<section id="upload" class="view">','<section id="upload" class="view">'+metricMarkup);
 html=html.replaceAll('Средняя сумма на работу','Средняя сумма на единицу')
  .replace('10, 20, 10 и 20 работ','10, 20, 10 и 20 единиц количества / объёма')
  .replace('всё количество работ в этих периодах','суммарное количество / объём в этих периодах')
  .replace('средняя сумма рассчитана отдельно','отношение суммы к количеству / объёму рассчитано отдельно');
 for(const id of dateIds){
  const expression=new RegExp('<select\\b([^>]*\\bid="'+id+'"[^>]*)>[\\s\\S]*?<\\/select>');
  if(!expression.test(html))throw Error('Missing canonical period control '+id);
  const boundary=rangeBoundaries[id];
  html=html.replace(expression,(_match,attributes)=>'<input'+attributes+(boundary?' data-range-boundary="'+boundary+'"':'')+' type="date">');
 }
 // Normalize only the labels of inclusive ranges in the desktop template.
 html=html.replace(/>С периода(?=<input|<\/label>)/g,'>С')
  .replace(/>(?:До периода|По период включительно)(?=<input|<\/label>)/g,'>По');
 // The rendered trend table supplies its own scroll viewport.
 html=html.replace('<div id="dataTable" class="table-scroll" hidden>','<div id="dataTable" hidden>');
 // Issue64: daily source supports every calendar granularity, without duplicate calendar selectors.
 html=html.replace('<option value="day">Дни</option></select>','<option value="day">Дни</option><option value="year">Годы</option></select>');
 html=html.replace('<div class="filters">','<div class="filters"><label>Источник<select id="unifiedSource"><option value="daily">Дневные отчёты</option><option value="imported">Отчёты выбранного типа</option></select></label>',1);
 if(!html.includes("<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\">"))throw Error('Canonical period toolbar changed');
 html=html.replace("<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\">","<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\"><label class=\"global-baseline\">Сравнивать с<input id=\"analysisBasePeriod\" type=\"date\" aria-label=\"Общий базовый период\"></label>");
 html=html.replace('<summary>Состав работ и доли</summary>','<summary>Состав работ и доли</summary><div id="structurePeriodOverride" class="period-override" hidden><span>Локальный период изменён.</span><button id="structurePeriodReset" type="button" class="button secondary">Использовать общий период</button></div>');
 html=html.replace('<div id="periodComparisonFilters" class="comparison-filters">','<div id="comparisonPeriodOverride" class="period-override" hidden><span>Локальный период изменён.</span><button id="comparisonPeriodReset" type="button" class="button secondary">Использовать общий период</button></div><div id="periodComparisonFilters" class="comparison-filters">');
 html=html.replace('<h2 id="changesHeading">Анализ изменений</h2>','<h2 id="changesHeading">Изменения между соседними периодами</h2>');
 html=html.replace('<div class="changes-filters">','<div id="changesPeriodOverride" class="period-override" hidden><span>Локальный период изменён. Анализ изменений сопоставляет соседние периоды.</span><button id="changesPeriodReset" type="button" class="button secondary">Использовать общий период</button></div><div class="changes-filters">');
 html=html.replace('Изменения к предыдущему интервалу','Изменения к базовому периоду')
  .replace('Изменения — относительно предшествующего интервала той же длительности.', 'Изменения рассчитываются относительно выбранной в общих фильтрах базы сравнения.');
 const originalSensitivity="<details class=\"change-sensitivity\">\n          <summary>Чувствительность анализа</summary>\n          <div class=\"changes-filters\">\n            <label>Процентный порог, %<input id=\"changesPercent\" type=\"number\" min=\"0\" step=\"any\" value=\"30\"></label>";
 const sensitivityInput=html.includes(originalSensitivity)?originalSensitivity:originalSensitivity.replaceAll('\n','\r\n');
 if(!html.includes(sensitivityInput))throw Error('Original change sensitivity controls changed');
 html=html.replace(sensitivityInput,"<div class=\"changes-sensitivity-quick\" role=\"group\" aria-labelledby=\"changesSensitivityLabel\">\n          <label id=\"changesSensitivityLabel\" for=\"changesPercent\">Чувствительность: процентный порог</label>\n          <div class=\"changes-sensitivity-row\"><input id=\"changesPercent\" type=\"range\" min=\"0\" max=\"200\" step=\"5\" value=\"30\" aria-describedby=\"changesSensitivityExample\"><output id=\"changesSensitivityValue\" for=\"changesPercent\">30%</output></div>\n          <p id=\"changesSensitivityExample\" class=\"muted small\" aria-live=\"polite\">При базе 100 единиц изменение на 30 достигает порога 30%. Для сильного изменения необходим также минимальный абсолютный порог.</p>\n        </div>\n        <details class=\"change-sensitivity\">\n          <summary>Дополнительно: минимальные изменения</summary>\n          <div class=\"changes-filters\">");
 const extraToolbar=/<div class="changes-filters change-toolbar">([\s\S]*?)<\/div>/;
 if(!extraToolbar.test(html))throw Error('Original changes toolbar changed');
 html=html.replace(extraToolbar,(_match,contents)=>'<details class="changes-more-options" id="changesMoreOptions"><summary>Дополнительно: режим, поиск и сортировка <span id="changesActiveFilter" class="muted small" aria-live="polite"></span></summary><div class="changes-filters change-toolbar">'+contents+'</div></details>');
 html=html.replace('Значимость по количеству','Сначала самые заметные · количество / объём').replace('Значимость по сумме','Сначала самые заметные · сумма');
 html=html.replace('Сначала обычная база, затем малая. Значимость: выполнение порогов, затем абсолютная разница выбранного показателя. Каждая строка указывает конкретную пару соседних периодов.',
 'Сильные изменения достигают одновременно процентного и абсолютного порогов. Услуги с малой базой показаны отдельно. Каждая строка относится к конкретной паре периодов.');
 html=html.replace(/<div class="section-filter-row" hidden style="display:none"><label>Тип источника<select id="calendarType">[\s\S]*?<\/div>/,'');
 // Preserve the distinction between zero reports, missing reports and incomplete data.
 const originalChartModes="<div class=\"segmented chart-mode-controls\"><button data-chart=\"line\" class=\"active\" title=\"Показывать изменение значений по периодам линиями\">Линейный график</button><button data-chart=\"bar\" title=\"Сравнивать значения столбцами\">Столбцы</button><button data-chart=\"table\" title=\"Показывать точные значения по периодам\">Таблица значений</button></div>";
 if(!html.includes(originalChartModes))throw Error('Canonical chart modes changed');
 const chartViewControls=`<div class="chart-mode-line">
   <div><strong id="chartViewLabel">Вид</strong><div class="segmented chart-mode-controls" role="group" aria-labelledby="chartViewLabel">
     <button type="button" data-chart="bar" class="active" title="Сравнить значения по периодам">Диаграмма</button>
     <button type="button" data-chart="table" title="Показать точные значения">Таблица</button>
   </div></div>
   <label>Расположение столбцов<select id="chartOrientation"><option value="vertical">Вертикально</option><option value="horizontal">Горизонтально</option></select></label>
 </div>
 <label class="checkline"><input id="hideEmptyPeriods" type="checkbox" aria-describedby="hideEmptyExplanation"> Скрыть пропуски и нулевые периоды</label>
 <small id="hideEmptyExplanation">Скрывает периоды без отчётов и периоды с подтверждёнными нулями по сумме и количеству выбранных рядов.</small>`;
 html=html.replace(originalChartModes,'');
 html=html.replace('<div class="chart-view-controls">','<div class="chart-presentation">'+chartViewControls+'</div><div class="chart-view-controls">');


 // Chart layout select options must fit 320-390 px without clipping.
 // Long-form scale explanations remain in the chart guide and captions.
 const longBreakdown='<option value="organizations">Подразделения по показателям</option>';
 const longAll='<option value="all">Все выполненные работы</option>';
 const longScope='<option value="selected">Только отмеченные работы и группы</option>';
 const longSeparate='<option value="separate">Отдельно — исходные значения</option>';
 const longTogether='<option value="combined">Вместе — % от максимума каждого показателя</option>';
 if(!html.includes(longBreakdown)||!html.includes(longAll)||!html.includes(longScope)||!html.includes(longSeparate)||!html.includes(longTogether))throw Error('Chart layout options changed');
 html=html.replace(longBreakdown,'<option value="organizations">Подразделения</option>')
   .replace(longAll,'<option value="all">Все работы</option>')
   .replace(longScope,'<option value="selected">Отмеченные</option>')
   .replace(longSeparate,'<option value="separate">Отдельно (значения)</option>')
   .replace(longTogether,'<option value="combined">Вместе (% макс.)</option>');

 // Both analysis sections are permanent content, not disclosure widgets.
 const contributionStart=html.indexOf('<details id="contributions" class="extended-section">');
 const contributionEnd=html.indexOf('</section>',contributionStart);
 if(contributionStart<0||contributionEnd<=contributionStart)throw Error('Canonical contribution layout changed');
 const contributionMarkup=html.slice(contributionStart,contributionEnd);
 if(!/<\/details>\s*$/.test(contributionMarkup))throw Error('Canonical contribution closing tag changed');
 const contributionSection=contributionMarkup
   .replace('<details id="contributions" class="extended-section">','<section id="contributions" class="extended-section" aria-labelledby="contributionsHeading">')
   .replace('<summary>Вклад работ в изменение итогов</summary>','<h3 id="contributionsHeading">Вклад работ в изменение итогов</h3>')
   .replace(/<\/details>(\s*)$/,'</section>$1');
 html=html.slice(0,contributionStart)+contributionSection+html.slice(contributionEnd);

 // Keep the original SQLite chart and composition renderers, but give them one
 // visible presentation switch. Relocate markup, not the report calculations.
 const structureStart=html.indexOf('<details class="panel" id="structurePanel">');
 const structureEnd=html.indexOf('<details class="panel" id="usualPanel">',structureStart);
 const comparisonStart=html.indexOf('<section class="panel" id="comparison"');
 if(structureStart<0||structureEnd<=structureStart||comparisonStart<0||comparisonStart>=structureStart)
  throw Error('Canonical work views layout changed');
 const originalStructure=html.slice(structureStart,structureEnd).trimEnd();
 if(!originalStructure.endsWith('</details>'))throw Error('Canonical composition closing tag changed');
 const structure=originalStructure
   .replace('<details class="panel" id="structurePanel">','<section class="panel" id="structurePanel" hidden aria-labelledby="structureHeading">')
   .replace('<summary>Состав работ и доли</summary>','<h2 id="structureHeading">Состав работ и доли</h2>')
   .replace(/<\/details>$/,'</section>');
 html=html.slice(0,structureStart)+html.slice(structureEnd);
 const switcher='<div class="work-view-switcher" role="group" aria-labelledby="workViewLabel">'+
 '<label id="workViewLabel" for="workViewMode">Показать</label>'+
 '<select id="workViewMode"><option value="trend">Динамика работ и подразделений</option><option value="composition">Состав работ и доли</option></select>'+
 '<p id="workViewDescription" class="muted small">Изменение значений по выбранным периодам. Для долей переключите представление.</p></div>\n      ';
 const chartStart=html.indexOf('<section class="panel" id="chartPanel"');
 if(chartStart<0||chartStart>=comparisonStart)throw Error('Canonical chart panel changed');
 html=html.slice(0,chartStart)+switcher+html.slice(chartStart);
 const relocated=html.indexOf('<section class="panel" id="comparison"');
 html=html.slice(0,relocated)+'      '+structure+'\n      '+html.slice(relocated);

 // The desktop route owns tab visibility. Keep only the inline help collapsible.
 for(const [id,title,heading,next] of [
  ['usualPanel','Отклонения от обычного уровня','usualHeading','<details class="panel" id="regularityPanel">'],
  ['regularityPanel','Частота выполнения работ','regularityHeading','<section class="panel" id="analyticsDetails"']
 ]){
  const opening='<details class="panel" id="'+id+'">';
  const start=html.indexOf(opening),end=html.indexOf(next,start),summary='<summary>'+title+'</summary>';
  const markup=html.slice(start,end);
  if(start<0||end<0||!markup.includes(summary)||!/<\/details>\s*$/.test(markup))throw Error('Canonical analytics tab changed: '+id);
  const section=markup.replace(opening,'<section class="panel" id="'+id+'" aria-labelledby="'+heading+'">')
   .replace(summary,'<h2 id="'+heading+'">'+title+'</h2>')
   .replace(/<\/details>(\s*)$/,'</section>$1');
  html=html.slice(0,start)+section+html.slice(end);
 }

 html=html.replace('</head>','<meta name="analiz-unified-sql" content="true"><link rel="stylesheet" href="tauri-app-status.css"><link rel="stylesheet" href="tauri-unified.css"><link rel="stylesheet" href="tauri-sql-row-scroll.css"><script src="tauri-shared-workspace.js"></script><script src="tauri-bridge.js"></script></head>');
 // Derive option samples from the same theme tokens as the analytical charts.
 const paletteCss=Array.from(source.matchAll(/:root\[data-palette="([^"]+)"\]\[data-theme="([^"]+)"\]\{([^}]+)\}/g),([,palette,theme,tokens])=>{
  const colors=[0,1,2].map(i=>'--sample-'+i+':'+tokens.match(new RegExp('--plot-'+i+':(#[0-9a-f]+);','i'))[1]);
  return 'html[data-theme="'+theme+'"] .palette-colors[data-sample="'+palette+'"]{'+colors.join(';')+'}';
 }).join('\n');
 html=html.replace('</head>','<style>'+paletteCss+'</style></head>');
 const filterSetting='<label class="placement-setting"><span id="filterPlacementLabel">Закреплять фильтр слева</span>'+
  '<input type="checkbox" role="switch" id="appearanceFilterPlacement" aria-labelledby="filterPlacementLabel" aria-describedby="filterPlacementHint"><span class="placement-value" aria-hidden="true"><span>Выкл</span><span>Вкл</span></span>'+
  '<small class="muted" id="filterPlacementHint">В узком окне фильтр остаётся сверху.</small></label>';
 const designLabel='<label>Дизайн<select id="appearanceDesign">';
 if(!html.includes(designLabel))throw Error('Missing desktop appearance settings');
 html=html.replace(designLabel,filterSetting+designLabel);
 const storageView='<section id="storage" class="view">';
 const uploadView='<section id="upload" class="view">';
 if(!html.includes(storageView)||!html.includes(uploadView))throw Error('Missing desktop management view');
 const backupStart=html.indexOf('<div class="panel"><h2>Резервная копия</h2>');
 const backupEnd=html.indexOf('<div class="panel danger">',backupStart);
 if(backupStart<0||backupEnd<backupStart)throw Error('Missing desktop backup controls');
 const appInfo='<section id="onlineUpdates" class="panel" aria-labelledby="appInfoHeading">'+
  '<div class="app-info-heading"><h2 id="appInfoHeading">Приложение и обновления</h2>'+
  '<span id="appVersion" class="app-version" role="status"></span></div></section>';
 const clearEnd=html.indexOf('</div>',backupEnd);
 if(clearEnd<0||!html.slice(backupEnd,clearEnd).includes('id="clearReports"'))throw Error('Missing desktop clear reports control');
 html=html.slice(0,backupStart)+appInfo+html.slice(clearEnd+'</div>'.length);
 html=html.replace(storageView,()=>fs.readFileSync(path.join(__dirname,'database-panel.html'),'utf8')+storageView);
 html=html.replace(uploadView,uploadView+
  '<div class="panel database-destination" id="sharedWorkspacePanel"><div>Отчёты сохраняются в выбранную базу <strong id="sharedWorkspaceMode" role="status">Проверка подключения…</strong></div>'+
  '<button type="button" class="button secondary" id="sharedWorkspaceShortcut">Управление базами</button></div>');
 html=html.replace('Данные остаются в этом браузере','Локальная база SQLite');
 html=html.replace('и защиты от очистки данных браузера.','и защиты от потери данных.');
 html=html.replace('Работает в браузере · Без отправки Excel на сервер · На другом устройстве данные не появляются автоматически','Анализ работ · SQLite');
 const exchangeStart=html.indexOf('<button id="exportGroups"');
 const exchangeEnd=html.indexOf('</label></div>',exchangeStart);
 if(exchangeStart<0||exchangeEnd<0)throw Error('Missing group file commands');
 const commands=html.slice(exchangeStart,exchangeEnd+8)
  .replace('Скачать группы (JSON)','Скачать группы в JSON')
  .replace('Импорт групп JSON','Заменить группы из JSON…')
  .replaceAll(' hidden>',' class="group-file-input">');
 const description=html.match(/<p class="muted small">Файл JSON содержит[^<]+<\/p>/);
 if(!description)throw Error('Missing group exchange description');
 html=html.slice(0,exchangeStart)+'<details id="groupExchange"><summary>Обмен файлами</summary>'+
  '<div class="actions">'+commands+'</div>'+description[0]+'</details>'+html.slice(exchangeEnd+8).replace(description[0],'');
 const groupNav='<nav id="groupSectionLinks" aria-label="Разделы групп работ">'+
  '<a href="#groupHeading">Группы</a><a href="#groupAuditPanel">Проверка названий</a>'+
  '<a href="#groupCatalog">Справочник услуг</a></nav>';
 html=html.replace('<h2 id="groupHeading">Группы работ</h2>',groupNav+'<h2 id="groupHeading" tabindex="-1">Группы</h2>');
 // Mount the shared navigation and SQL forms once, retaining their inputs and listeners.
 const nativeStart=html.indexOf('<header class="topbar">');
 const nativeEnd=html.indexOf('<section id="printReport"');
 if(nativeStart<0||nativeEnd<nativeStart)throw Error('Missing native workspace layout');
 const reference=fs.readFileSync(path.join(__dirname,'reference-shell.html'),'utf8');
 const workspace=html.slice(nativeStart,nativeEnd);
 html=html.slice(0,nativeStart)+reference.replace('<div id="refNative"></div>',()=>'<div id="refNative">'+workspace+'</div>')+html.slice(nativeEnd);
 html=html.replace('</head>','<link rel="stylesheet" href="tauri-reference-shell.css"><link rel="stylesheet" href="tauri-reference-detail.css"></head>');
 const modules=['unified-original','unified-horizontal-bars','native-errors','native-period-selection','app-status','analysis-summary','unified-base','sql-row-scroll','unified-coverage','unified-management','database-backups','unified-reports','native-metric-kinds','unified-extended','unified-analytics','reference-calendar','reference-detail','reference-results','reference-filters','reference-desktop','unified-app','updates'];
 html=html.replace('</body>',modules.map(name=>'<script src="tauri-'+name+'.js"></script>').join('\n')+'\n</body>');
 return {html,script};
}
module.exports={buildUnified};
