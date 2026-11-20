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
 script=script.replace(metricMarker,"function metricTitle(metric){return {count:'Количество / объём',amount:'Сумма',average:'Сумма / количество (объём)'}[metric]||metric;}");
 // The original renderer predates calendar-year rollups. Adapt only generated
 // presentation helpers; the independent browser source remains unchanged.
 const rankingColor="fill.style.setProperty('--rank-color',seriesColor(row.name));";
 if(!script.includes(rankingColor))throw Error('Original ranking color marker changed');
 script=script.replace(rankingColor,"fill.style.setProperty('--rank-color','var(--accent)');");
 const rankingRows=".filter(item=>Number.isFinite(item.value))";
 if(!script.includes(rankingRows))throw Error('Original ranking filter marker changed');
 script=script.replace(rankingRows,".filter(item=>Number.isFinite(item.value)&&item.value!==0)");
 const noZero="Нет сопоставимых значений за выбранный период.";
 if(!script.includes(noZero))throw Error('Original empty ranking marker changed');
 script=script.replace(noZero,"Нет ненулевых значений за выбранный период. Нулевые строки рейтинга скрыты.");
 const chartValues="return value===null?[]:[value||0];";
 if(!script.includes(chartValues))throw Error('Original chart value marker changed');
 script=script.replace(chartValues,"return Number.isFinite(value)?[value]:[];");
 const chartPoint="if(raw===null){flush();return;}";
 if(!script.includes(chartPoint))throw Error('Original chart point marker changed');
 script=script.replace(chartPoint,"if(!Number.isFinite(raw)){flush();return;}");
 const horizontalMarker="  const data=options.data||periods.map(p=>reportHasOrganizations(p,orgs)?summary(p,orgs).by:null);\n  if(periods.length===1){";
 if(!script.includes(horizontalMarker))throw Error('Original chart geometry changed');
 script=script.replace(horizontalMarker,"  const data=options.data||periods.map(p=>reportHasOrganizations(p,orgs)?summary(p,orgs).by:null);\n  if(bar&&options.orientation==='horizontal'){\n    if(typeof window.unifiedHorizontalBars!=='function')throw Error('Не загружена горизонтальная диаграмма.');\n    window.unifiedHorizontalBars(periods,shown,data,metric,root,options,seriesColor,()=>drawChart(periods,orgs,metric,root,options));\n    return;\n  }\n  if(periods.length===1){");
 script+=`
(function(){
 const original={periodEnd,periodTitle,periodScale,periodUnit,plotPeriodLabel,displayPeriod,shiftPeriod};
 periodEnd=(start,type)=>type==='year'?start.slice(0,4)+'-12-31':original.periodEnd(start,type);
 periodTitle=type=>type==='year'?'Год':original.periodTitle(type);
 periodScale=type=>type==='year'?'годы':original.periodScale(type);
 periodUnit=(type,count)=>type==='year'?ruPlural(count,'год','года','лет'):original.periodUnit(type,count);
 plotPeriodLabel=period=>period.type==='year'?period.start.slice(0,4):original.plotPeriodLabel(period);
 displayPeriod=period=>period.type==='year'?period.start.slice(0,4):original.displayPeriod(period);
 shiftPeriod=(type,start,steps)=>type==='year'?String(Number(start.slice(0,4))+steps).padStart(4,'0')+'-01-01':original.shiftPeriod(type,start,steps);
})();
`;
 const missingMessage="      const message=available?'Неполные данные: '+available+' из '+orgs.size+' подразделений':'Нет отчёта';";
 if(!script.includes(missingMessage))throw Error('Original chart missing-data notice marker changed');
 script=script.replace(missingMessage,`      const suppliedNotice=typeof options.pointNotice==='function'?options.pointNotice(i):null;
      const customNotice=typeof suppliedNotice==='string'&&suppliedNotice.trim()?suppliedNotice:null;
      const message=customNotice||(available?'Неполные данные: '+available+' из '+orgs.size+' подразделений':'Нет отчёта');`);
 script=script.replace("      if(available){note.append(svgEl('tspan'", "      if(available&&!customNotice){note.append(svgEl('tspan'");

 // Never allow a paged UI to reach an old whole-database writer by accident.
 script+='\nmain=wire=loadData=saveSettings=storeReports=storeAllData=saveImport=saveGroup=moveWork=saveOrganizations=importGroupFile=importBackupFile=()=>{throw Error("Полная загрузка истории и старые обработчики отключены.");};\n';
 script+='window.unifiedOriginal=Object.freeze({state,$,el:elem,notice,fmt:formatMetric,fmtNumber,drawChart,drawDepartments:drawDepartmentMultiChart,makeTable,tableCsvButton,initTheme,initChartTooltip,redrawCharts,renderMultiFilter,periodEnd,shiftPeriod,displayDay,displayPeriod,metricTitle,chartRenders});\n';
 let html=source.replace(/<script>[\s\S]*?<\/script>/g,'');
 const metricMarkup="<details id=\"additionalMetrics\" class=\"panel group-section\"><summary>Другие показатели — трудозатраты и план / факт</summary>\n<p class=\"muted small\">Независимые виды отчётов со своими полями и единицами измерения. Их показатели не прибавляются к суммам выполненных работ.</p>\n<div class=\"metric-controls\">\n<label>Вид отчёта<select id=\"metricKindSelect\"></select></label>\n<label>С<input id=\"metricKindFrom\" type=\"date\"></label>\n<label>По<input id=\"metricKindTo\" type=\"date\"></label>\n<button type=\"button\" id=\"metricKindCalculate\" class=\"button secondary\">Рассчитать</button></div>\n<p id=\"metricKindCount\" class=\"muted small\">Пустые даты при расчёте означают всю историю.</p>\n<div id=\"metricKindTotals\" class=\"metric-results\"></div>\n<div class=\"metric-controls\">\n<label>Шаблон XLSX<select id=\"metricXlsxProfile\"></select></label>\n<button type=\"button\" id=\"metricKindChooseXlsx\" class=\"button secondary\">Импорт XLSX</button>\n<button type=\"button\" id=\"metricKindChoose\" class=\"button secondary\">Импорт JSON</button>\n<button type=\"button\" id=\"metricKindApply\" class=\"button primary\" disabled>Подтвердить импорт</button></div>\n<p class=\"muted small\">Для XLSX сначала укажите обе даты и шаблон. Перед записью проверьте значения: импорт не заменяет существующие отчёты. Лимит XLSX — 30 МиБ, JSON — 8 МиБ, не более 10 000 строк в отчёте.</p>\n<p id=\"metricKindFile\" class=\"muted small\">Файл ещё не выбран.</p>\n<div id=\"metricKindPreview\" class=\"metric-results\"></div>\n<p id=\"metricKindStatus\" class=\"status muted\" role=\"status\" aria-live=\"polite\">Загрузка видов отчётов…</p></details>";
 if(!html.includes('<section id="upload" class="view">'))throw Error('Original Excel upload panel changed');
 html=html.replace('<section id="upload" class="view">','<section id="upload" class="view">'+metricMarkup);
 html=html.replaceAll('Средняя сумма на работу','Сумма / количество (объём)')
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
 // Issue64: daily source supports every calendar granularity, without duplicate calendar selectors.
 html=html.replace('<option value="day">Дни</option></select>','<option value="day">Дни</option><option value="year">Годы</option></select>');
 html=html.replace('<div class="filters">','<div class="filters"><label>Источник<select id="unifiedSource"><option value="daily">Дневные отчёты</option><option value="imported">Отчёты выбранного типа</option></select></label>',1);
 if(!html.includes("<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\">"))throw Error('Canonical period toolbar changed');
 html=html.replace("<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\">","<div class=\"period-toolbar\" aria-label=\"Общий период для выбранных подразделений\"><label class=\"global-baseline\">Сравнивать с<input id=\"analysisBasePeriod\" type=\"date\" aria-label=\"Общий базовый период\"></label>");
 html=html.replace('<summary>Состав работ и доли</summary>','<summary>Состав работ и доли</summary><div id="structurePeriodOverride" class="period-override" hidden><span>Локальный период изменён.</span><button id="structurePeriodReset" type="button" class="button secondary">Использовать общий период</button></div>');
 html=html.replace('<div id="periodComparisonFilters" class="comparison-filters">','<div id="comparisonPeriodOverride" class="period-override" hidden><span>Локальный период изменён.</span><button id="comparisonPeriodReset" type="button" class="button secondary">Использовать общий период</button></div><div id="periodComparisonFilters" class="comparison-filters">');
 html=html.replace('<div class="changes-filters">','<div id="changesPeriodOverride" class="period-override" hidden><span>Локальный период изменён. Анализ изменений сопоставляет соседние периоды.</span><button id="changesPeriodReset" type="button" class="button secondary">Использовать общий период</button></div><div class="changes-filters">');
 html=html.replace('Изменения к предыдущему интервалу','Изменения к базовому периоду');
 const originalSensitivity="<details class=\"change-sensitivity\">\n          <summary>Чувствительность анализа</summary>\n          <div class=\"changes-filters\">\n            <label>Процентный порог, %<input id=\"changesPercent\" type=\"number\" min=\"0\" step=\"any\" value=\"30\"></label>";
 const sensitivityInput=html.includes(originalSensitivity)?originalSensitivity:originalSensitivity.replaceAll('\n','\r\n');
 if(!html.includes(sensitivityInput))throw Error('Original change sensitivity controls changed');
 html=html.replace(sensitivityInput,"<div class=\"changes-sensitivity-quick\" role=\"group\" aria-labelledby=\"changesSensitivityLabel\">\n          <label id=\"changesSensitivityLabel\" for=\"changesPercent\">Чувствительность: процентный порог</label>\n          <div class=\"changes-sensitivity-row\"><input id=\"changesPercent\" type=\"range\" min=\"0\" max=\"200\" step=\"5\" value=\"30\" aria-describedby=\"changesSensitivityExample\"><output id=\"changesSensitivityValue\" for=\"changesPercent\">30%</output></div>\n          <p id=\"changesSensitivityExample\" class=\"muted small\" aria-live=\"polite\">При базе 100 единиц изменение на 30 достигает порога 30%. Для сильного изменения необходим также минимальный абсолютный порог.</p>\n        </div>\n        <details class=\"change-sensitivity\">\n          <summary>Дополнительно: минимальные изменения</summary>\n          <div class=\"changes-filters\">");
 const extraToolbar=/<div class="changes-filters change-toolbar">([\s\S]*?)<\/div>/;
 if(!extraToolbar.test(html))throw Error('Original changes toolbar changed');
 html=html.replace(extraToolbar,(_match,contents)=>'<details class="changes-more-options" id="changesMoreOptions"><summary>Дополнительно: режим, поиск и сортировка<span id="changesActiveFilter" class="muted small" aria-live="polite"></span></summary><div class="changes-filters change-toolbar">'+contents+'</div></details>');
 html=html.replace('Значимость по количеству','Сначала самые заметные · количество / объём').replace('Значимость по сумме','Сначала самые заметные · сумма');
 html=html.replace('Сначала обычная база, затем малая. Значимость: выполнение порогов, затем абсолютная разница выбранного показателя. Каждая строка указывает конкретную пару соседних периодов.',
 'Сильные изменения достигают одновременно процентного и абсолютного порогов. Услуги с малой базой показаны отдельно. Каждая строка относится к конкретной паре периодов.');
 html=html.replace(/<div class="section-filter-row" hidden style="display:none"><label>Тип источника<select id="calendarType">[\s\S]*?<\/div>/,'');
 // Issue67: hide only confirmed zero periods, keep missing periods and natural vertical layout.
 if(!html.includes("<div class=\"segmented chart-mode-controls\"><button data-chart=\"line\" class=\"active\" title=\"Показывать изменение значений по периодам линиями\">Линейный график</button><button data-chart=\"bar\" title=\"Сравнивать значения столбцами\">Столбцы</button><button data-chart=\"table\" title=\"Показывать точные значения по периодам\">Таблица значений</button></div>"))throw Error('Canonical chart modes changed');
 html=html.replace("<div class=\"segmented chart-mode-controls\"><button data-chart=\"line\" class=\"active\" title=\"Показывать изменение значений по периодам линиями\">Линейный график</button><button data-chart=\"bar\" title=\"Сравнивать значения столбцами\">Столбцы</button><button data-chart=\"table\" title=\"Показывать точные значения по периодам\">Таблица значений</button></div>",'<label class="checkline"><input id="hideEmptyPeriods" type="checkbox"> Скрывать подтверждённые нулевые периоды</label>'+"<div class=\"chart-mode-line\"><div class=\"segmented chart-mode-controls\"><button type=\"button\" data-chart=\"bar\" class=\"active\" title=\"Сравнить значения по периодам\">Диаграмма</button><button type=\"button\" data-chart=\"table\" title=\"Показать точные значения\">Таблица значений</button></div><label>Расположение столбцов<select id=\"chartOrientation\"><option value=\"vertical\">Вертикально</option><option value=\"horizontal\">Горизонтально</option></select></label></div>");

 // Keep the original SQLite chart and composition renderers, but give them one
 // visible presentation switch. Relocate markup, not the report calculations.
 const structureStart=html.indexOf('<details class="panel" id="structurePanel">');
 const structureEnd=html.indexOf('<details class="panel" id="usualPanel">',structureStart);
 const comparisonStart=html.indexOf('<section class="panel" id="comparison"');
 if(structureStart<0||structureEnd<=structureStart||comparisonStart<0||comparisonStart>=structureStart)
  throw Error('Canonical work views layout changed');
 const structure=html.slice(structureStart,structureEnd).trimEnd().replace('id="structurePanel"','id="structurePanel" hidden');
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

 html=html.replace('</head>','<meta name="analiz-unified-sql" content="true"><link rel="stylesheet" href="tauri-app-status.css"><link rel="stylesheet" href="tauri-unified.css"><link rel="stylesheet" href="tauri-sql-row-scroll.css"><script src="tauri-bridge.js"></script></head>');
 html=html.replace('Данные остаются в этом браузере','Локальная база SQLite');
 html=html.replace('и защиты от очистки данных браузера.','и защиты от потери данных.');
 html=html.replace('Работает в браузере · Без отправки Excel на сервер · На другом устройстве данные не появляются автоматически','Локальная база SQLite · Без отправки Excel на сервер · Для переноса на другое устройство сохраните резервную копию');
 html=html.replace('>Скачать группы (JSON)</button>','>Экспорт групп</button>').replace('>Импорт групп JSON<input','>Импорт групп<input').replace('Добавить справочник услуг из Excel\n','Импорт справочника Excel\n');
 const catalog=html.match(/<div class="catalog-toolbar">[\s\S]*?<\/div>/);
 if(catalog){html=html.replace(catalog[0],'');const groups=html.indexOf('id="importGroups"');const end=html.indexOf('</div>',groups);if(groups<0||end<0)throw Error('Missing group import toolbar');html=html.slice(0,end)+catalog[0]+html.slice(end);}
 // Mount the reference and the existing SQL forms in one document. Moving the
 // native workspace between the panel and the modal preserves inputs and listeners.
 const nativeStart=html.indexOf('<header class="topbar">');
 const nativeEnd=html.indexOf('<section id="printReport"');
 if(nativeStart<0||nativeEnd<nativeStart)throw Error('Missing native workspace layout');
 const reference=fs.readFileSync(path.join(__dirname,'reference-shell.html'),'utf8');
 const workspace=html.slice(nativeStart,nativeEnd);
 html=html.slice(0,nativeStart)+reference.replace('<div id="refNative"></div>',()=>'<div id="refNative">'+workspace+'</div>')+html.slice(nativeEnd);
 html=html.replace('</head>','<link rel="stylesheet" href="tauri-reference-shell.css"><link rel="stylesheet" href="tauri-reference-detail.css"></head>');
 const modules=['unified-original','unified-horizontal-bars','native-errors','native-period-selection','app-status','unified-base','sql-row-scroll','unified-coverage','unified-management','unified-reports','native-metric-kinds','unified-extended','unified-analytics','reference-calendar','reference-detail','reference-results','reference-filters','reference-desktop','unified-app','updates'];
 html=html.replace('</body>',modules.map(name=>'<script src="tauri-'+name+'.js"></script>').join('\n')+'\n</body>');
 return {html,script};
}
module.exports={buildUnified};
