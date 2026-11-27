'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os'),vm=require('node:vm');
const {prepare,prepareHtml}=require('../prepare.cjs');
const original=fs.readFileSync(path.resolve(__dirname,'../../index.html'),'utf8');
test('one canonical application retains the whole original interface and never starts its full-history controller',()=>{
 const html=prepareHtml(original);
 for(const id of ['dashboard','changes','upload','groups','storage','chartSeriesLayout','changesSort','changesDirection','regularityStatus','groupAuditPanel','groupCatalog','appearanceSize'])assert.match(html,new RegExp('id="'+id+'"'));
 assert.ok(!html.includes('window.appReady=main();'),'old full-history main must not run');
 assert.match(html,/tauri-unified-app.js/);assert.match(html,/tauri-unified-horizontal-bars.js/);assert.ok(!html.includes('tauri-native-reader.js'));assert.ok(!html.includes('Классический интерфейс'));
 assert.ok(html.indexOf('tauri-bridge.js')<html.indexOf('tauri-unified-original.js'));
});
test('the source renderer is available without invoking old wire, storage or full report loading',()=>{
 const out=fs.mkdtempSync(path.join(os.tmpdir(),'unified-entry-'));
 try{prepare({output:out});const file=path.join(out,'tauri-unified-original.js');assert.ok(fs.existsSync(file),'canonical presentation helpers must be generated');
  const window={};vm.runInNewContext(fs.readFileSync(file,'utf8'),{window,document:{getElementById(){throw Error('unexpected bootstrap');}}});
  assert.equal(window.unifiedOriginal.state.reports.length,0);assert.equal(typeof window.unifiedOriginal.drawChart,'function');assert.equal(window.appReady,undefined);
 }finally{fs.rmSync(out,{recursive:true,force:true});}
});
test('both installed and GUI-test windows use the sole application entry',()=>{
 for(const name of ['src-tauri/tauri.conf.json','tests/ci-webview2.json']){
  const config=JSON.parse(fs.readFileSync(path.join(__dirname,'..',name),'utf8'));assert.equal(config.app.windows[0].url,'index.html');
 }
});
test('canonical DOM and all styles survive except duplicate source/year calendar selectors',()=>{
 const html=prepareHtml(original),ids=text=>[...text.matchAll(/\bid="([^"]+)"/g)].map(m=>m[1]);
 const removed=new Set(['calendarType','calendarYearSelect']),built=ids(html);
 for(const id of ids(original))if(!removed.has(id))assert.equal(built.filter(x=>x===id).length,1,'missing or duplicate canonical control '+id);
 assert.equal(html.match(/<style>([\s\S]*?)<\/style>/)[1],original.match(/<style>([\s\S]*?)<\/style>/)[1]);
 assert.match(html,/<input[^>]*id="groupAuditFrom"[^>]*type="date"/);
 assert.match(html,/<input[^>]*id="rangeStart"[^>]*type="date"/);
 const groupToolbar=html.slice(html.indexOf('id="createGroup"'),html.indexOf('id="groupCoverage"'));
 assert.match(groupToolbar,/id="importGroups"/);assert.match(groupToolbar,/id="catalogFile"/);
 assert.match(html,/id="hideEmptyPeriods"/);assert.match(html,/id="unifiedSource"/);
 assert.match(html,/id="chartOrientation"/);assert.doesNotMatch(html,/data-chart="line"/);
 for(const id of ['additionalMetrics','metricKindSelect','metricKindCalculate','metricKindChooseXlsx','metricKindChoose','metricKindApply','metricKindPreview','metricKindStatus'])assert.match(html,new RegExp('id="'+id+'"'));
 assert.doesNotMatch(html,/<script>([\s\S]*?)<\/script>/);
});
test('boot initializes each SQL module once and waits for refresh without invoking legacy entrypoints',async()=>{
 const calls=[],window={unifiedUI:{init:()=>calls.push('UI'),refresh:async()=>calls.push('refresh'),notice:()=>assert.fail('unexpected error')}};
 for(const name of ['unifiedManagement','unifiedReports','unifiedExtended','unifiedAnalytics','databaseBackups'])window[name]={init:()=>calls.push(name)};
 const code=fs.readFileSync(path.resolve(__dirname,'../unified-app.js'),'utf8');vm.runInNewContext(code,{window});
 assert.equal(await window.appReady,true);assert.deepEqual(calls,['unifiedManagement','unifiedReports','unifiedExtended','unifiedAnalytics','databaseBackups','UI','refresh']);
 assert.throws(()=>vm.runInNewContext(code,{window}),/уже запущено/);
});
test('boot failure is visible, resolves safely and never starts a partial workspace',async()=>{
 const notices=[],window={unifiedUI:{init:()=>assert.fail('partial boot'),notice:(...args)=>notices.push(args)}};
 vm.runInNewContext(fs.readFileSync(path.resolve(__dirname,'../unified-app.js'),'utf8'),{window});
 assert.equal(await window.appReady,false);assert.match(notices[0][0],/Не загружен модуль/);assert.equal(notices[0][1],true);
});
test('generated helper script retains CSV support while every old writer is blocked',()=>{
 const {script}=require('../unified-build.cjs').buildUnified(original),window={},context={window};vm.runInNewContext(script,context);
 assert.equal(typeof window.unifiedOriginal.tableCsvButton,'function');
 for(const name of ['main','wire','loadData','saveSettings','storeReports','storeAllData','saveImport','saveGroup','moveWork','saveOrganizations','importGroupFile','importBackupFile'])assert.throws(()=>vm.runInNewContext(name+'()',context),/отключены/);
});
test('distribution contains every referenced script and dynamically loaded native folder controller',()=>{
 const output=fs.mkdtempSync(path.join(os.tmpdir(),'unified-assets-'));
 try{prepare({output});const html=fs.readFileSync(path.join(output,'index.html'),'utf8');
  for(const [,src]of html.matchAll(/<script[^>]+src="([^"]+)"/g))assert.ok(fs.existsSync(path.join(output,src)),'missing referenced script '+src);
  for(const [,href]of html.matchAll(/<link[^>]+href="([^"]+)"/g))assert.ok(fs.existsSync(path.join(output,href)),'missing stylesheet '+href);
  assert.ok(fs.existsSync(path.join(output,'tauri-native-metric-kinds.js')),'additional metrics controller must ship');
  for(const name of ['native-folder-auto-import.js','native-folder-status.js'])assert.equal(fs.readFileSync(path.join(output,name),'utf8'),fs.readFileSync(path.resolve(__dirname,'..',name),'utf8'));
  assert.ok(html.indexOf('tauri-unified-app.js')<html.indexOf('tauri-updates.js'),'updates must await the installed appReady promise');
  assert.deepEqual(fs.readdirSync(output).filter(n=>n.endsWith('.html')),['index.html']);
  for(const name of ['tauri-native-workspace.js','tauri-folder-importer.js','tauri-native-reader.js','tauri-native-navigation.js','tauri-native-updates.js','tauri-startup.js'])assert.ok(!fs.existsSync(path.join(output,name)),'obsolete controller '+name);
 }finally{fs.rmSync(output,{recursive:true,force:true});}
});
test('generated year helpers use calendar bounds, year labels and Russian plurals without false gaps',()=>{
 const {script}=require('../unified-build.cjs').buildUnified(original),context={window:{}};vm.runInNewContext(script,context);
 const value=code=>vm.runInNewContext(code,context);
 assert.equal(value("periodEnd('2024-01-01','year')"),'2024-12-31');
 assert.equal(value("periodTitle('year')"),'Год');assert.equal(value("periodScale('year')"),'годы');
 for(const [n,word]of [[1,'год'],[2,'года'],[5,'лет'],[21,'год']])assert.equal(value(`periodUnit('year',${n})`),word);
 assert.equal(value("displayPeriod({type:'year',start:'2024-01-01',end:'2024-12-31'})"),'2024');
 assert.equal(value("plotPeriodLabel({type:'year',start:'2024-01-01',end:'2024-12-31'})"),'2024');
 assert.equal(value("shiftPeriod('year','2024-01-01',-1)"),'2023-01-01');
 assert.equal(value("chartTimelineGaps([{type:'year',start:'2023-01-01'},{type:'year',start:'2024-01-01'}]).length"),0);
 assert.equal(value("periodEnd('2024-02-01','month')"),'2024-02-29');
 assert.equal(value("shiftPeriod('week','2024-01-01',1)"),'2024-01-08');
});
test('generated drawChart uses pointNotice at missing points as safe text and retains fallback',()=>{
 class Node{
  constructor(tag){this.tagName=tag;this.children=[];this.textContent='';this.style={setProperty(){}};this.dataset={};this.attributes={};this.clientWidth=1640;}
  append(...nodes){this.children.push(...nodes);}
  replaceChildren(...nodes){this.children=nodes;}
  contains(){return false;}
  setAttribute(key,value){this.attributes[key]=String(value);}
 }
 const document={activeElement:null,createElement:tag=>new Node(tag),createElementNS:(_,tag)=>new Node(tag)};
 const {script}=require('../unified-build.cjs').buildUnified(original),context={window:{},document};vm.runInNewContext(script,context);
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../unified-horizontal-bars.js'),'utf8'),context);
 vm.runInNewContext("workSeriesColor=()=> '#123456';",context);
 const O=context.window.unifiedOriginal,root=new Node('div'),periods=[{type:'year',start:'2023-01-01',end:'2023-12-31'},{type:'year',start:'2024-01-01',end:'2024-12-31'}];
 const seen=[],message='Неполные данные: пропусков 4 <img src=x>';
 O.drawChart(periods,new Set(['А','Б']),'count',root,{categories:['work:one'],data:[null,null],pointNotice:i=>{seen.push(i);return i===0?message:null;}});
 const all=node=>[node,...node.children.flatMap(all)],nodes=all(root);
 assert.deepEqual(seen,[0,1]);
 const plot=root.children.find(n=>n.className==='chart-plot-scroll');
 const legend=root.children.find(n=>n.className==='chart-legend');
 assert.equal(plot.children[0].attributes.viewBox,'0 0 1640 420');
 assert.ok(root.children.indexOf(plot)<root.children.indexOf(legend));
 const markers=nodes.filter(n=>n.tagName==='text'&&n.dataset.chartTooltip);
 assert.equal(markers.length,2);
 assert.ok(markers.every(n=>n.textContent==='!'&&n.attributes.tabindex==='0'&&n.attributes['aria-label']===n.dataset.chartTooltip));
 const notes=all(nodes.find(n=>n.tagName==='details')).filter(n=>n.tagName==='li');
 assert.deepEqual(notes.map(n=>n.textContent),['2023 · '+message,'2024 · Нет отчёта']);
 assert.deepEqual(markers.map(n=>n.dataset.chartTooltip),notes.map(n=>n.textContent));
 assert.ok(!nodes.some(n=>n.tagName==='img'));
 assert.ok(!nodes.some(n=>n.tagName==='text'&&n.textContent==='Пропуск'),'adjacent years must not show a synthetic gap');
 O.drawChart(periods,new Set(['А']),'count',root,{categories:['work:one'],data:[new Map([['work:one',{count:0}]]),null],pointNotice:()=>({unsafe:'object'})});
 assert.equal(all(root).filter(n=>n.tagName==='li'&&n.textContent==='2024 · Нет отчёта').length,1);
 assert.equal(all(root).filter(n=>n.tagName==='circle').length,1,'Only the confirmed zero has a chart point');
});
test('generated original presentation uses quantity/volume terminology, standalone HTML remains unchanged',()=>{
 const {html,script}=require('../unified-build.cjs').buildUnified(original);
 const context={window:{}};vm.runInNewContext(script,context);
 assert.equal(context.window.unifiedOriginal.metricTitle('count'),'Количество / объём');
 assert.equal(context.window.unifiedOriginal.metricTitle('average'),'Сумма / количество (объём)');
 assert.doesNotMatch(html,/Средняя сумма на работу|10, 20, 10 и 20 работ/);
 assert.match(html,/Сумма \/ количество \(объём\)/);
 assert.match(original,/Средняя сумма на работу/);
});

test('Windows CRLF presentation still generates the column-orientation controls',()=>{
 const windows=original.replace(/\r?\n/g,'\r\n');
 const html=prepareHtml(windows);
 assert.match(html,/id="chartOrientation"/);assert.match(html,/id="changesPeriodOverride"/);assert.match(html,/id="analysisBasePeriod"/);assert.match(html,/id="changesSensitivityValue"/);assert.match(html,/id="changesMoreOptions"/);assert.doesNotMatch(html,/data-chart="line"/);
});

test('Tauri canonical page exposes one shared baseline and visible local period overrides',()=>{
 const {html}=require('../unified-build.cjs').buildUnified(original);
 for(const id of ['analysisBasePeriod','comparisonPeriodOverride','comparisonPeriodReset','structurePeriodOverride','structurePeriodReset','changesPeriodOverride','changesPeriodReset'])
  assert.match(html,new RegExp('id="'+id+'"'));
 assert.match(html,/Изменения к базовому периоду/);
});

test('installed analytics uses an accessible sensitivity slider and folds secondary filters',()=>{
 const html=prepareHtml(original);
 for(const id of ['changesPercent','changesSensitivityValue','changesSensitivityExample','changesMoreOptions','changesActiveFilter']){
  assert.equal([...html.matchAll(new RegExp('id="'+id+'"','g'))].length,1,id+' should appear once');
 }
 assert.match(html,/<input id="changesPercent" type="range"[^>]*max="200"/);
 assert.match(html,/<details class="changes-more-options" id="changesMoreOptions">/);
 assert.doesNotMatch(html,/Сначала обычная база, затем малая/);
});

test('trend and composition occupy one presentation switch, retaining native SQL controls',()=>{
 const html=prepareHtml(original);
 const selector=html.indexOf('id="workViewMode"'),chart=html.indexOf('id="chartPanel"'),composition=html.indexOf('id="structurePanel"'),comparison=html.indexOf('id="comparison"');
 assert.ok(selector>=0&&selector<chart&&chart<composition&&composition<comparison);
 assert.match(html,/<section class="panel" id="structurePanel" hidden aria-labelledby="structureHeading">/);
 assert.match(html,/<h2 id="structureHeading">Состав работ и доли<\/h2>/);
 assert.match(html,/<section id="contributions" class="extended-section" aria-labelledby="contributionsHeading">/);
 assert.match(html,/<h3 id="contributionsHeading">Вклад работ в изменение итогов<\/h3>/);
 assert.doesNotMatch(html,/<details\b[^>]*\bid="(?:structurePanel|contributions)"/);
 assert.match(html,/id="structureLimit"/);
 assert.match(html,/id="chartsContainer"/);
 assert.equal([...html.matchAll(/id="structurePanel"/g)].length,1);
 assert.equal([...html.matchAll(/id="chartPanel"/g)].length,1);
});

test('the installed trend table lets its inner viewport own the scroll',()=>{
 const html=prepareHtml(original);
 assert.match(html,/<div id="dataTable" hidden><\/div>/);
 assert.match(original,/<div id="dataTable" class="table-scroll" hidden><\/div>/);
});

test('partial monthly chart and table headings show the actual dates but keep calendar keys for gap checks',()=>{
 const context={window:{}};vm.runInNewContext(require('../unified-build.cjs').buildUnified(original).script,context);
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),context);
 const O=context.window.unifiedOriginal,scope={source:'daily',displayType:'day',from:'2024-01-28',to:'2024-06-03'};
 const periods=context.window.analysisSummary.timelinePeriods(['2024-01','2024-02','2024-06'].map(start=>({start})),'month',scope,O.periodEnd);
 assert.equal(periods[0].start,'2024-01-01');
 assert.equal(O.displayPeriod(periods[0]),'28.01.2024 — 31.01.2024');
 assert.equal(O.displayPeriod(periods[2]),'01.06.2024 — 03.06.2024');
 assert.match(O.displayPeriod(periods[1]),/февраль/);
 context.period=periods[0];assert.equal(vm.runInNewContext('plotPeriodLabel(period)',context),'28.01–31.01');
});

test('a monthly bucket of imported weeks spans the actual reports across month and year boundaries',()=>{
 const context={window:{}};vm.runInNewContext(require('../unified-build.cjs').buildUnified(original).script,context);
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),context);
 const O=context.window.unifiedOriginal,scope={source:'imported',displayType:'week',from:'2023-12-27',to:'2026-05-20'};
 const points=[{start:'2023-12',first_start:'2023-12-27',last_start:'2023-12-27'},
               {start:'2024-01',first_start:'2024-01-03',last_start:'2024-01-31'}];
 const periods=context.window.analysisSummary.timelinePeriods(points,'month',scope,O.periodEnd);
 assert.equal(periods[0].start,'2023-12-01');assert.equal(O.displayPeriod(periods[0]),'27.12.2023 — 02.01.2024');
 assert.equal(periods[1].start,'2024-01-01');assert.equal(O.displayPeriod(periods[1]),'03.01.2024 — 06.02.2024');
});

test('installed department comparison presents charts before the table',()=>{
 const html=prepareHtml(original);
 const order=(source,...ids)=>ids.map(id=>source.indexOf('id="'+id+'"')).every((offset,i,offsets)=>offset>=0&&(i===0||offset>offsets[i-1]));
 assert.ok(order(html,'comparisonMetrics','comparisonDynamics','comparisonCharts','comparisonTable'));
 assert.match(html,/<h3 id="comparisonDynamicsHeading">Динамика по периодам<\/h3>/);
 assert.ok(order(original,'comparisonTable','comparisonCharts','comparisonDynamics'));
});
