'use strict';
// The original markup/styles are canonical; only its full-history controller is retired.
const dateIds=['overviewPeriod','rangeStart','rangeEnd','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','structureBase','structureCurrent','usualPeriod','groupAuditFrom','groupAuditTo','changesFrom','changesTo'];
function buildUnified(source){
 if(/analiz-unified-sql|tauri-bridge\.js/.test(source))throw Error('Настольные скрипты уже присутствуют.');
 const inline=[...source.matchAll(/<script>([\s\S]*?)<\/script>/g)];
 const presentation=inline.find(item=>item[1].includes('const state =')&&item[1].includes('window.appReady=main();'));
 if(!presentation)throw Error('Original interface presentation markers changed');
 let script=presentation[1].replace('window.appReady=main();','').replace(/\r\n/g,'\n');
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
  html=html.replace(expression,(_match,attributes)=>'<input'+attributes+' type="date">');
 }
 // Issue64: daily source supports every calendar granularity, without duplicate calendar selectors.
 html=html.replace('<option value="day">Дни</option></select>','<option value="day">Дни</option><option value="year">Годы</option></select>');
 html=html.replace('<div class="filters">','<div class="filters"><label>Источник<select id="unifiedSource"><option value="daily">Дневные отчёты</option><option value="imported">Отчёты выбранного типа</option></select></label>',1);
 html=html.replace(/<div class="section-filter-row"><label>Тип источника<select id="calendarType">[\s\S]*?<\/div>/,'');
 // Issue67: hide only confirmed zero periods, keep missing periods and natural vertical layout.
 if(!html.includes("<div class=\"segmented chart-mode-controls\"><button data-chart=\"line\" class=\"active\" title=\"Показывать изменение значений по периодам линиями\">Линейный график</button><button data-chart=\"bar\" title=\"Сравнивать значения столбцами\">Столбцы</button><button data-chart=\"table\" title=\"Показывать точные значения по периодам\">Таблица значений</button></div>"))throw Error('Canonical chart modes changed');
 html=html.replace("<div class=\"segmented chart-mode-controls\"><button data-chart=\"line\" class=\"active\" title=\"Показывать изменение значений по периодам линиями\">Линейный график</button><button data-chart=\"bar\" title=\"Сравнивать значения столбцами\">Столбцы</button><button data-chart=\"table\" title=\"Показывать точные значения по периодам\">Таблица значений</button></div>",'<label class="checkline"><input id="hideEmptyPeriods" type="checkbox"> Скрывать подтверждённые нулевые периоды</label>'+"<div class=\"chart-mode-line\"><div class=\"segmented chart-mode-controls\"><button type=\"button\" data-chart=\"bar\" class=\"active\" title=\"Сравнить значения по периодам\">Диаграмма</button><button type=\"button\" data-chart=\"table\" title=\"Показать точные значения\">Таблица значений</button></div><label>Расположение столбцов<select id=\"chartOrientation\"><option value=\"vertical\">Вертикально</option><option value=\"horizontal\">Горизонтально</option></select></label></div>");
 html=html.replace('</head>','<meta name="analiz-unified-sql" content="true"><link rel="stylesheet" href="tauri-app-status.css"><link rel="stylesheet" href="tauri-unified.css"><script src="tauri-bridge.js"></script></head>');
 html=html.replace('Данные остаются в этом браузере','Локальная база SQLite');
 html=html.replace('и защиты от очистки данных браузера.','и защиты от потери данных.');
 html=html.replace('Работает в браузере · Без отправки Excel на сервер · На другом устройстве данные не появляются автоматически','Локальная база SQLite · Без отправки Excel на сервер · Для переноса на другое устройство сохраните резервную копию');
 html=html.replace('>Скачать группы (JSON)</button>','>Экспорт групп</button>').replace('>Импорт групп JSON<input','>Импорт групп<input').replace('Добавить справочник услуг из Excel\n','Импорт справочника Excel\n');
 const catalog=html.match(/<div class="catalog-toolbar">[\s\S]*?<\/div>/);
 if(catalog){html=html.replace(catalog[0],'');const groups=html.indexOf('id="importGroups"');const end=html.indexOf('</div>',groups);if(groups<0||end<0)throw Error('Missing group import toolbar');html=html.slice(0,end)+catalog[0]+html.slice(end);}
 const modules=['unified-original','unified-horizontal-bars','native-errors','native-period-selection','app-status','unified-base','unified-management','unified-reports','native-metric-kinds','unified-extended','unified-analytics','unified-app','updates'];
 html=html.replace('</body>',modules.map(name=>'<script src="tauri-'+name+'.js"></script>').join('\n')+'\n</body>');
 return {html,script};
}
module.exports={buildUnified};
