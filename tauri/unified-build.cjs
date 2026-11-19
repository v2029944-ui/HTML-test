'use strict';
// The original markup/styles are canonical; only its full-history controller is retired.
const dateIds=['overviewPeriod','rangeStart','rangeEnd','basePeriod','currentPeriod','baseRangeStart','baseRangeEnd','currentRangeStart','currentRangeEnd','comparisonPeriod','comparisonRangeStart','comparisonRangeEnd','structureBase','structureCurrent','usualPeriod','groupAuditFrom','groupAuditTo','changesFrom','changesTo'];
function buildUnified(source){
 if(/analiz-unified-sql|tauri-bridge\.js/.test(source))throw Error('Настольные скрипты уже присутствуют.');
 const inline=[...source.matchAll(/<script>([\s\S]*?)<\/script>/g)];
 const presentation=inline.find(item=>item[1].includes('const state =')&&item[1].includes('window.appReady=main();'));
 if(!presentation)throw Error('Original interface presentation markers changed');
 let script=presentation[1].replace('window.appReady=main();','');
 // The original renderer predates calendar-year rollups. Adapt only generated
 // presentation helpers; the independent browser source remains unchanged.
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
 html=html.replace('<div class="segmented chart-mode-controls">','<label class="checkline"><input id="hideEmptyPeriods" type="checkbox"> Скрывать подтверждённые нулевые периоды</label><div class="segmented chart-mode-controls">',1);
 html=html.replace('</head>','<meta name="analiz-unified-sql" content="true"><link rel="stylesheet" href="tauri-app-status.css"><link rel="stylesheet" href="tauri-unified.css"><script src="tauri-bridge.js"></script></head>');
 html=html.replace('Данные остаются в этом браузере','Локальная база SQLite');
 html=html.replace('и защиты от очистки данных браузера.','и защиты от потери данных.');
 html=html.replace('Работает в браузере · Без отправки Excel на сервер · На другом устройстве данные не появляются автоматически','Локальная база SQLite · Без отправки Excel на сервер · Для переноса на другое устройство сохраните резервную копию');
 html=html.replace('>Скачать группы (JSON)</button>','>Экспорт групп</button>').replace('>Импорт групп JSON<input','>Импорт групп<input').replace('Добавить справочник услуг из Excel\n','Импорт справочника Excel\n');
 const catalog=html.match(/<div class="catalog-toolbar">[\s\S]*?<\/div>/);
 if(catalog){html=html.replace(catalog[0],'');const groups=html.indexOf('id="importGroups"');const end=html.indexOf('</div>',groups);if(groups<0||end<0)throw Error('Missing group import toolbar');html=html.slice(0,end)+catalog[0]+html.slice(end);}
 const modules=['unified-original','native-errors','native-period-selection','app-status','unified-base','unified-management','unified-reports','unified-extended','unified-analytics','unified-app','updates'];
 html=html.replace('</body>',modules.map(name=>'<script src="tauri-'+name+'.js"></script>').join('\n')+'\n</body>');
 return {html,script};
}
module.exports={buildUnified};
