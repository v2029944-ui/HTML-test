'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
function mount(overrides={}){
 const nodes={};
 class E{
  constructor(tag='div'){this.tag=tag;this.nodeType=1;this.children=[];this.events={};this.dataset={};this.attrs={};this.value='';this.checked=false;this.disabled=false;this.isConnected=true;}
  set id(v){this._id=v;nodes[v]=this;}get id(){return this._id;}
  append(...xs){for(const x of xs){if(x&&typeof x==='object'){x.parentElement=this;}this.children.push(x);}}
  prepend(x){x.parentElement=this;this.children.unshift(x);}after(x){x.parentElement=this.parentElement;this.parentElement?.children.push(x);}before(x){this.after(x);}
  replaceChildren(...xs){for(const x of this.children)if(x&&typeof x==='object')x.isConnected=false;this.children=[];this.append(...xs);}
  remove(){if(this.parentElement)this.parentElement.children=this.parentElement.children.filter(x=>x!==this);this.isConnected=false;}
  addEventListener(e,fn){(this.events[e]??=[]).push(fn);}async fire(e,extra={}){if(this.disabled&&e==='click')return;for(const fn of this.events[e]||[])await fn({preventDefault(){},stopImmediatePropagation(){},...extra});}
  click(){return this.fire('click');}focus(){this.focused=true;}scrollIntoView(){}setAttribute(k,v){this.attrs[k]=v;}
  querySelectorAll(s){const all=this.children.flatMap(x=>x&&typeof x==='object'?[x,...x.querySelectorAll('*')]:[]);if(s==='*')return all;if(s==='button')return all.filter(x=>x.tag==='button');if(s==='input,select,textarea,button')return all.filter(x=>['input','select','textarea','button'].includes(x.tag));if(s==='[data-mapping]')return all.filter(x=>x.dataset.mapping);if(s==='input[data-ordinal]')return all.filter(x=>x.tag==='input'&&x.dataset.ordinal!==undefined);return [];}
  querySelector(s){return this.querySelectorAll('*').find(x=>s==='[data-field="'+x.dataset.field+'"]')||null;}
 }
 const ids=['reports','reportCount','upload','calendarDetails','reportCalendar','calendarRange','calendarPrev','calendarNext','qualitySource','qualityPeriod','qualityOrg','qualityType','qualityTable','qualitySummary','qualityPage','qualityPrev','qualityNext','qualityHeading','reportPeriods','uploadedReports','importAdvanced','replaceExisting','importPreview','excelFile','importFormat','importType','importWeek','importMonth','sheetSelect','dailyYear','importMappingMode','inspectFile','saveImport','dailyWideOptions','weekStartLabel','monthStartLabel','importDateCaption','importManualWrap','importColumnMappings','addImportMapping','excelFileStatus','dailySheetChoices','importPeriodDates','saveDailyFolderRule'];
 const root=new E();for(const id of ids){const e=new E(id.includes('File')?'input':'div');e.id=id;const p=new E();root.append(p);p.append(e);}
 const filterRoot=new E(),qualityLabel=new E();filterRoot.append(qualityLabel);qualityLabel.append(nodes.qualityType);
 Object.assign(nodes.importFormat,{value:'legacy'});nodes.importType.value='day';nodes.importWeek.value='2024-01-01';nodes.importMappingMode.value='auto';nodes.qualitySource.value='saved';for(const id of ['qualityPeriod','qualityOrg','qualityType'])nodes[id].value='all';
 const api={managementReportWindow:async()=>({rows:[],total_rows:0}),managementReportSummary:async()=>({reports:0,rows:0,amount:0,latest:'2024-01-01'}),managementReportOrgWindow:async()=>({rows:[],total_rows:0}),unifiedCalendar:async(revision,year)=>({revision,year,days:[]}),nativeDailyRollup:async()=>({points:[],next_cursor:null}),unifiedQualityPage:async()=>({issues:[],next_offset:null,total:0,counts:{}}),chooseWideXlsx:async()=>null,nativeImportFolderContext:async()=>null,nativeImportSheets:async()=>[{name:'Дни',component:'xl/worksheets/sheet1.xml'}],nativeLegacyImportSheets:async()=>['xl/worksheets/sheet1.xml'],nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,rowCount:2,conflicts:{},quality:{},sampleReports:[]}),nativeLegacyImportApply:async()=>({reports:1,rows:2,revision:2}),chooseSqliteBackupPath:async()=>null,...overrides};
 let snapshotHook,viewHook;const UI={api,$:id=>nodes[id],el:(tag,text,cls)=>{const e=new E(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;},tableHelp:(section,_table,headers,help)=>{section.headerHints=headers.filter((_,i)=>help[i]);},revision:1,snapshot:{org_names:['A','B','C','D']},fmt:v=>v??'—',onSnapshot:fn=>snapshotHook=fn,onView:(_,fn)=>viewHook=fn,refresh:async()=>{UI.revision++;snapshotHook(UI.snapshot);}};
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),unifiedUI:UI,confirm:()=>true};const document={createElement:t=>new E(t),getElementById:id=>nodes[id],head:new E('head')};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../unified-reports.js'),'utf8'),{window,document,console});window.unifiedReports.init();return {nodes,UI,window,startFolder:async()=>{
  window.desktopAPI=api;
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-folder-status.js'),'utf8'),{window,document});
  await window.nativeFolderReady;
 },refresh:async()=>{await window.unifiedReports.openView('list');await window.unifiedReports.openView('calendar');await window.unifiedReports.openView('quality');},snapshot:(value=UI.snapshot)=>snapshotHook(value),view:()=>viewHook()};
}
const text=e=>[e.textContent||'',...e.children.map(x=>typeof x==='object'?text(x):'')].join(' ');
const flush=()=>new Promise(r=>setImmediate(r));
test('both Excel previews show workbook cells independently of imported rows', async () => {
 for (const format of ['legacy', 'daily-wide']) {
  const preview = async () => ({
   reviewToken: 'a'.repeat(64), reportCount: 1, rowCount: 2, processedCellCount: 12345
  });
  const ui = mount({
   chooseWideXlsx: async () => '/tmp/cells.xlsx',
   nativeLegacyImportPreview: preview, nativeWideImportPreview: preview
  });
  await ui.nodes.excelFile.click();
  ui.nodes.importFormat.value = format;
  await ui.nodes.importFormat.fire('change');
  await ui.nodes.inspectFile.click();
  const cards = ui.nodes.importPreview.querySelectorAll('*').filter(el => el.className === 'kpi');
  const values = Object.fromEntries(cards.map(el => [el.children[1].textContent, el.children[0].textContent]));
  assert.equal(values['Строк работ'], '2', format);
  assert.equal(values['Обработано ячеек в книге'], (12345).toLocaleString('ru-RU'), format);
 }
});
test('wide import selects multiple named sheets and keeps legacy component identifiers',async()=>{
 const previews=[];const ui=mount({chooseWideXlsx:async()=>'/tmp/quarters.xlsx',nativeImportSheets:async()=>[
  {name:'Первый квартал',component:'xl/worksheets/sheet1.xml'},
  {name:'Второй; квартал',component:'xl/custom/sheet2.xml'}
 ],nativeWideImportPreview:async(_source,options)=>{previews.push(options);return {reviewToken:'a'.repeat(64),reportCount:182,rowCount:10,conflicts:{},quality:{},sampleReports:[]};}});
 await ui.nodes.excelFile.click();
 assert.equal(ui.nodes.sheetSelect.children[1].textContent,'Второй; квартал');
 assert.equal(ui.nodes.sheetSelect.children[1].value,'xl/custom/sheet2.xml');
 ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');
 for(const i of [0,1]){ui.nodes['unifiedWideSheet'+i].checked=true;await ui.nodes['unifiedWideSheet'+i].fire('change');}
 await ui.nodes.inspectFile.click();
 assert.deepEqual(Array.from(previews[0].sheets),['Первый квартал','Второй; квартал']);
 assert.equal(ui.nodes.saveImport.disabled,false);
 ui.nodes.unifiedWideSheet1.checked=false;await ui.nodes.unifiedWideSheet1.fire('change');
 assert.equal(ui.nodes.saveImport.disabled,true);
});
test('undated quarter requires a confirmed year and invalidates review when changed',async()=>{
 const previews=[];const ui=mount({chooseWideXlsx:async()=>'/tmp/quarters.xlsx',nativeWideImportPreview:async(_source,options)=>{previews.push(options);return {reviewToken:'a'.repeat(64),reportCount:91,rowCount:10,conflicts:{},quality:{},sampleReports:[]};}});
 await ui.nodes.excelFile.click();ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');
 ui.nodes.unifiedWidePeriod0.value='q2';await ui.nodes.unifiedWidePeriod0.fire('change');
 await ui.nodes.inspectFile.click();assert.equal(previews.length,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/год/);
 ui.nodes.dailyYear.value='2024';await ui.nodes.inspectFile.click();
 assert.deepEqual(JSON.parse(JSON.stringify(previews[0].sequential_periods)),{'Дни':{year:2024,quarter:2}});
 assert.equal(ui.nodes.saveImport.disabled,false);assert.equal(ui.nodes.saveDailyFolderRule.hidden,true);
 ui.nodes.unifiedWidePeriod0.value='q3';await ui.nodes.unifiedWidePeriod0.fire('change');assert.equal(ui.nodes.saveImport.disabled,true);
});
test('empty explicit sheet selection cannot silently become automatic import',async()=>{
 let reads=0;const ui=mount({chooseWideXlsx:async()=>'/tmp/quarters.xlsx',nativeWideImportPreview:async()=>{reads++;}});
 await ui.nodes.excelFile.click();ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');
 ui.nodes.unifiedWideSheetMode.value='selected';await ui.nodes.unifiedWideSheetMode.fire('change');
 await ui.nodes.inspectFile.click();assert.equal(reads,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/лист/);
});
test('twelve canonical calendar months, leap days, close day/month restores focus',async()=>{
 const ui=mount();await ui.refresh();assert.equal(ui.nodes.reportCalendar.children.length,12);const feb=ui.nodes.reportCalendar.children[1];await feb.click();const buttons=ui.nodes.calendarDetails.querySelectorAll('button');assert.equal(buttons.length,30);const day=buttons.at(-1);await day.click();await flush();let all=ui.nodes.calendarDetails.querySelectorAll('button');const closeDay=all.find(b=>b.textContent==='Закрыть подробности дня');await closeDay.click();assert.equal(day.focused,true);await buttons[0].click();assert.equal(feb.focused,true);assert.equal(ui.nodes.calendarDetails.children.length,0);
});
test('old deferred calendar replies cannot replace a newer snapshot',async()=>{
 let resolve;const ui=mount({unifiedCalendar:(rev,year)=>rev===1?new Promise(r=>resolve=r):Promise.resolve({year,days:[]})});const pending=ui.refresh();await flush();ui.UI.revision=2;await ui.refresh();resolve({year:2024,days:[{date:'2024-02-29',day_reports:1,warnings:999}]});await pending;assert.equal(ui.nodes.reportCalendar.children.length,12);assert.doesNotMatch(text(ui.nodes.reportCalendar),/999/);
});
test('canonical XLSX chooser cancellation never previews or saves',async()=>{let previews=0,applies=0;const ui=mount({nativeLegacyImportPreview:async()=>{previews++;},nativeLegacyImportApply:async()=>{applies++;}});await ui.nodes.excelFile.click();assert.match(ui.nodes.unifiedImportStatus.textContent,/отменён|Настройки/);assert.equal(ui.nodes.inspectFile.disabled,true);assert.equal(previews,0);assert.equal(applies,0);});
test('reviewed replacement needs acknowledgement and a chosen backup before apply',async()=>{
 let applies=0;const ui=mount({chooseWideXlsx:async()=>'/tmp/book.xlsx',nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:2,rowCount:3,conflicts:{replacements:1},quality:{issues:1},sampleReports:[]}),nativeLegacyImportApply:async()=>{applies++;}});await ui.nodes.excelFile.click();ui.nodes.sheetSelect.value='xl/worksheets/sheet1.xml';await ui.nodes.inspectFile.click();assert.equal(ui.nodes.saveImport.disabled,false);await ui.nodes.saveImport.click();assert.equal(applies,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/Подтвердите/);ui.nodes.replaceExisting.checked=true;ui.nodes.unifiedAcceptQuality.checked=true;await ui.nodes.saveImport.click();assert.equal(applies,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/резервная копия/);
});
test('import acknowledgements are hidden until current SQL preview identifies a matching reason',async()=>{
 let result={reviewToken:'a'.repeat(64),reportCount:1,rowCount:2,conflicts:{},quality:{},sampleReports:[]};
 const ui=mount({chooseWideXlsx:async()=>'/tmp/review.xlsx',nativeLegacyImportPreview:async()=>result});
 const overlap=ui.nodes.unifiedAcceptOverlap,quality=ui.nodes.unifiedAcceptQuality,box=ui.nodes.unifiedImportAcknowledgements;
 assert.equal(box.hidden,true);assert.equal(overlap.parentElement.hidden,true);assert.equal(quality.parentElement.hidden,true);
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();assert.equal(box.hidden,true);
 result={...result,conflicts:{cross_type_overlaps:3}};await ui.nodes.inspectFile.click();
 assert.equal(box.hidden,false);assert.equal(overlap.parentElement.hidden,false);assert.equal(quality.parentElement.hidden,true);
 assert.match(text(overlap.parentElement),/3/);assert.match(text(overlap.parentElement),/период|дат/);assert.equal(overlap.checked,false);
 for(const [key,count] of [['issues',2],['mismatch_reports',4],['relevant_source_issues',5]]){
  result={...result,conflicts:{},quality:{[key]:count}};await ui.nodes.inspectFile.click();
  assert.equal(overlap.parentElement.hidden,true);assert.equal(quality.parentElement.hidden,false);
  assert.match(text(quality.parentElement),new RegExp(String(count)));assert.equal(quality.checked,false);
 }
 result={...result,quality:{source_issues:7,deferred_source_issues:7}};await ui.nodes.inspectFile.click();assert.equal(box.hidden,true);
});
test('conditional agreements reset on new review, changed parameters and another source, with no writes on denial',async()=>{
 let source='/tmp/A.xlsx',applies=0;
 const ui=mount({chooseWideXlsx:async()=>source,nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,rowCount:2,conflicts:{cross_type_overlaps:1},quality:{issues:2},sampleReports:[]}),nativeLegacyImportApply:async()=>{applies++;}});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 await ui.nodes.saveImport.click();assert.equal(applies,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/Подтвердите/);
 ui.nodes.unifiedAcceptOverlap.checked=true;ui.nodes.unifiedAcceptQuality.checked=true;
 ui.window.confirm=async()=>false;await ui.nodes.saveImport.click();assert.equal(applies,0);assert.equal(ui.UI.revision,1);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/отменено/);
 await ui.nodes.inspectFile.click();assert.equal(ui.nodes.unifiedAcceptOverlap.checked,false);assert.equal(ui.nodes.unifiedAcceptQuality.checked,false);
 ui.nodes.importWeek.value='2024-02-01';await ui.nodes.importWeek.fire('change');assert.equal(ui.nodes.unifiedImportAcknowledgements.hidden,true);
 await ui.nodes.inspectFile.click();ui.nodes.unifiedAcceptOverlap.checked=true;ui.nodes.unifiedAcceptQuality.checked=true;
 source='/tmp/B.xlsx';await ui.nodes.excelFile.click();assert.equal(ui.nodes.unifiedImportAcknowledgements.hidden,true);
 assert.equal(ui.nodes.unifiedAcceptOverlap.checked,false);assert.equal(ui.nodes.unifiedAcceptQuality.checked,false);assert.equal(applies,0);
});
test('quality date fields, individual saved periods and all-period reset match the native query',async()=>{
 const calls=[],report={id:'saved',report_type:'week',start:'2024-02-26',end:'2024-03-03'};
 const ui=mount({managementReportWindow:async()=>({rows:[report],total_rows:1}),unifiedQualityPage:async(_revision,query)=>{calls.push(query);return {issues:[],next_offset:null,total:0,counts:{}};}});
 await ui.refresh();
 const {qualityPeriod:period,qualityFrom:from,qualityTo:to}=ui.nodes;
 await ui.nodes.reports.querySelectorAll('button').find(b=>b.textContent==='Контроль качества').click();await flush();
 assert.equal(ui.nodes.qualitySummary.textContent,'Замечаний по выбранным условиям нет. Отсутствие замечаний само по себе не подтверждает полноту.');
 assert.deepEqual([from.value,to.value,calls.at(-1).from,calls.at(-1).to],Array(4).fill('2024-02-26'));
 from.value='2024-02-01';await from.fire('change');await flush();
 assert.equal(calls.at(-1).from,'2024-02-01');assert.equal(period.children[0].textContent,'Диапазон по датам');
 period.value='week:2024-02-26';await period.fire('change');await flush();
 assert.deepEqual([from.value,to.value,calls.at(-1).from,calls.at(-1).report_type],['','','2024-02-26','week']);
 to.value='2024-02-29';await to.fire('change');await flush();
 assert.deepEqual([period.value,calls.at(-1).from,calls.at(-1).to],['all',null,'2024-02-29']);
 await period.fire('change');await flush();
 assert.deepEqual([from.value,to.value,calls.at(-1).from,calls.at(-1).to],['','',null,null]);
});
test('quality filters and offset go to native global query, old responses clear on failure',async()=>{
 const calls=[];const ui=mount({unifiedQualityPage:async(...args)=>{calls.push(args);return {issues:[],next_offset:50,total:80,counts:{missing:80}};}});await ui.refresh();ui.nodes.qualitySearch.value='needle';await ui.nodes.qualitySearch.fire('input');await flush();assert.equal(calls.at(-1)[1].search,'needle');await ui.nodes.qualityTable.scrollWindow.go(1);await flush();assert.equal(calls.at(-1)[2],50);assert.match(ui.nodes.qualitySummary.textContent,/80/);
});

test('snapshot invalidation accepts null and clears stale reports and quality',async()=>{const ui=mount();await ui.refresh();assert.equal(ui.nodes.reportCalendar.children.length,12);ui.snapshot(null);assert.equal(ui.nodes.reportCalendar.children.length,0);assert.equal(ui.nodes.reports.children.length,0);assert.equal(ui.nodes.qualityTable.children.length,0);assert.equal(ui.nodes.saveImport.disabled,true);});

test('changing XLSX A to B clears dates year and manual mappings before B review',async()=>{
 const choices=['/tmp/A.xlsx','/tmp/B.xlsx'];let previews=0;
 const ui=mount({chooseWideXlsx:async()=>choices.shift(),nativeLegacyImportPreview:async()=>{previews++;throw Error('should require new date');}});
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2020-01-01';ui.nodes.importMonth.value='2020-01';ui.nodes.dailyYear.value='2020';ui.nodes.importMappingMode.value='manual';await ui.nodes.addImportMapping.click();ui.nodes.replaceExisting.checked=true;
 await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'B.xlsx');assert.equal(ui.nodes.importWeek.value,'');assert.equal(ui.nodes.importMonth.value,'');assert.equal(ui.nodes.dailyYear.value,'');assert.equal(ui.nodes.importMappingMode.value,'auto');assert.equal(ui.nodes.importColumnMappings.children.length,0);assert.equal(ui.nodes.replaceExisting.checked,false);
 await ui.nodes.inspectFile.click();assert.equal(previews,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/первую дату/);
});

test('canceling the replacement chooser retains the reviewed workbook and its parameters',async()=>{const choices=['/tmp/A.xlsx',null];const ui=mount({chooseWideXlsx:async()=>choices.shift()});await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-03-01';await ui.nodes.inspectFile.click();assert.equal(ui.nodes.saveImport.disabled,false);await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'A.xlsx');assert.equal(ui.nodes.importWeek.value,'2024-03-01');assert.equal(ui.nodes.inspectFile.disabled,false);assert.equal(ui.nodes.saveImport.disabled,false);assert.match(ui.nodes.unifiedImportStatus.textContent,/отменён/);});

test('a selected Windows source receives only its checked folder rule',async()=>{const ui=mount({chooseWideXlsx:async()=>String.raw`C:\Excel\B.xlsx`,nativeImportFolderContext:async()=>({folder:String.raw`\\?\C:\Excel`,name:'B.xlsx',revision:1,nativeSignature:'sig',rule:{format:'daily-wide',type:'day',start:'',year:2025,sheets:['Дни'],mappings:{'Дни':{workCol:0,headerRow:1,firstDayCol:1}}}})});await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'B.xlsx');assert.equal(ui.nodes.importFormat.value,'daily-wide');assert.equal(ui.nodes.dailyYear.value,'2025');assert.equal(ui.nodes.unifiedWideSheets.value,'Дни');assert.match(ui.nodes.unifiedWideMapping.value,/Дни; 2; A; B/);});

test('deferred preview locks dynamic fields and rejects parameter events before exposing review',async()=>{
 let resolve;const ui=mount({chooseWideXlsx:async()=>'/tmp/A.xlsx',nativeLegacyImportPreview:()=>new Promise(r=>resolve=r)});await ui.nodes.excelFile.click();const pending=ui.nodes.inspectFile.click();await flush();assert.equal(ui.nodes.addImportMapping.disabled,true);assert.equal(ui.nodes.unifiedWideSheets.disabled,true);assert.equal(ui.nodes.unifiedWideMapping.disabled,true);
 ui.nodes.unifiedWideMapping.value='Data; 2; A; B;;';await ui.nodes.unifiedWideMapping.fire('input');resolve({reviewToken:'a'.repeat(64),reportCount:1,rowCount:1,conflicts:{},quality:{},sampleReports:[]});await pending;assert.equal(ui.nodes.saveImport.disabled,true);assert.match(ui.nodes.unifiedImportStatus.textContent,/изменились/);assert.equal(ui.nodes.addImportMapping.disabled,false);
});

test('reports and their organizations request an arbitrary page directly and keep the active revision',async()=>{
 const calls=[],orgReads=[],report=i=>({id:'r'+i,start:'2024-01-01',end:'2024-01-01',report_type:'day',rows:1,amount:2,warnings:0,organizations:103});
 const ui=mount({managementReportWindow:async(rev,filter,index,limit)=>{
  calls.push([rev,filter,index,limit]);return {rows:Array.from({length:Math.min(limit,151-index*limit)},(_,i)=>report(index*limit+i)),total_rows:151};
 },managementReportSummary:async()=>({reports:151,rows:151,amount:302,latest:'2024-01-01'}),
 managementReportOrgWindow:async(rev,id,index,limit)=>{orgReads.push([rev,id,index,limit]);return {rows:Array.from({length:Math.min(limit,103-index*limit)},(_,i)=>[index*limit+i,'Орг '+(index*limit+i)]),total_rows:103};}});
 await ui.refresh();
 const root=ui.nodes.reports;
 await root.scrollWindow.go(3);assert.equal(root.children.length,1);assert.equal(root.scrollWindow.total,151);assert.equal(root.scrollWindow.index,3);
 const details=root.children[0].querySelectorAll('*').find(n=>n.tag==='details');details.open=true;await details.fire('toggle');await flush();
 const list=details.children[1];await list.scrollWindow.go(2);assert.equal(list.children.length,3);assert.deepEqual(orgReads,[[1,'r150',0,50],[1,'r150',2,50]]);
 await root.scrollWindow.go(0);assert.deepEqual(calls.map(c=>c[2]),[0,3,0]);assert.ok(calls.every(c=>c[3]===50));
});

test('connected-file picker preserves Windows verbatim separators and Unix paths',async()=>{
 for(const [folder,expected] of [[String.raw`\\?\C:\Excel`,String.raw`\\?\C:\Excel\B.xlsx`],['/tmp/excel','/tmp/excel/B.xlsx']]){
  let selected=null;const ui=mount({list:async()=>({folder,files:[{name:'B.xlsx'}],rules:{}}),chooseFolder:async()=>null,nativeImportSheets:async source=>{selected=source;return [{name:'Дни',component:'xl/worksheets/sheet1.xml'}];}});
  await ui.window.unifiedReports.openSource(expected);assert.equal(selected,expected);
 }
});

const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};
async function reviewedReplacement(){
 const calls={prompts:0,pickers:0,applies:0},answer=deferred();
 const ui=mount({chooseWideXlsx:async()=>'/tmp/book.xlsx',nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,rowCount:1,conflicts:{replacements:1},quality:{},sampleReports:[]}),chooseSqliteBackupPath:async()=>{calls.pickers++;return '/tmp/backup.sqlite3';},nativeLegacyImportApply:async()=>{calls.applies++;return {reports:1,rows:1,revision:2};}});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();ui.nodes.replaceExisting.checked=true;ui.window.confirm=()=>{calls.prompts++;return answer.promise;};return {ui,calls,answer};
}
test('native Promise import confirmation denies without picker/write and blocks duplicate prompts',async()=>{
 const {ui,calls,answer}=await reviewedReplacement();const pending=ui.nodes.saveImport.click();await flush();await ui.nodes.saveImport.click();assert.equal(calls.prompts,1);assert.equal(calls.pickers,0);assert.equal(calls.applies,0);assert.equal(ui.nodes.unifiedWideMapping.disabled,true);answer.resolve(false);await pending;assert.equal(calls.pickers,0);assert.equal(calls.applies,0);assert.equal(ui.nodes.saveImport.disabled,false);assert.match(ui.nodes.unifiedImportStatus.textContent,/отменено/);
});
test('native Promise import confirmation accepts exactly once after answering',async()=>{
 const {ui,calls,answer}=await reviewedReplacement();const pending=ui.nodes.saveImport.click();await flush();assert.equal(calls.pickers,0);assert.equal(calls.applies,0);answer.resolve(true);await pending;assert.equal(calls.prompts,1);assert.equal(calls.pickers,1);assert.equal(calls.applies,1);
});
test('rejected native import confirmation is caught without picker or write',async()=>{
 const {ui,calls,answer}=await reviewedReplacement();const pending=ui.nodes.saveImport.click();await flush();answer.reject(Error('dialog unavailable'));await pending;assert.equal(calls.pickers,0);assert.equal(calls.applies,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/dialog unavailable/);assert.equal(ui.nodes.excelFile.disabled,false);
});
test('confirmed import is invalidated by changed revision or background snapshot while dialog waits',async()=>{
 for(const invalidate of [ui=>{ui.UI.revision=2;},ui=>ui.snapshot(null)]){const {ui,calls,answer}=await reviewedReplacement();const pending=ui.nodes.saveImport.click();await flush();invalidate(ui);answer.resolve(true);await pending;assert.equal(calls.pickers,0);assert.equal(calls.applies,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/изменились/);}
});
async function deletableReport(){
 const calls={prompts:0,pickers:0,deletes:0},answer=deferred(),notices=[];
 const ui=mount({managementReportWindow:async()=>({rows:[{id:'r',start:'2024-01-01',end:'2024-01-01',report_type:'day',rows:1,amount:2,warnings:0,organizations:1}],total_rows:1}),managementReportSummary:async()=>({reports:1,rows:1,amount:2,latest:'2024-01-01'}),chooseSqliteBackupPath:async()=>{calls.pickers++;return '/tmp/backup.sqlite3';},nativeDeleteReport:async()=>{calls.deletes++;}});ui.UI.notice=(message)=>notices.push(message);ui.window.confirm=()=>{calls.prompts++;return answer.promise;};await ui.refresh();const action=ui.nodes.reports.querySelectorAll('button').find(b=>b.textContent==='Удалить отчёт');return {ui,calls,answer,notices,action};
}
test('native delete confirmation awaits deny/accept and prevents duplicate prompts',async()=>{
 for(const accepted of [false,true]){const {calls,answer,action}=await deletableReport();const pending=action.click();await flush();await action.click();assert.equal(calls.prompts,1);assert.equal(calls.pickers,0);assert.equal(calls.deletes,0);answer.resolve(accepted);await pending;assert.equal(calls.pickers,0);assert.equal(calls.deletes,accepted?1:0);}
});
test('native delete confirmation rejects safely and rechecks revision before deletion',async()=>{
 for(const stale of [false,true]){const {ui,calls,answer,notices,action}=await deletableReport();const pending=action.click();await flush();if(stale){ui.UI.revision=2;answer.resolve(true);}else answer.reject(Error('dialog unavailable'));await pending;assert.equal(calls.pickers,0);assert.equal(calls.deletes,0);assert.equal(action.disabled,false);assert.match(notices.at(-1),stale?/изменились/:/dialog unavailable/);}
});
test('confirmed import rejects parameter edits and revision changes during backup selection',async()=>{
 {const {ui,calls,answer}=await reviewedReplacement();const pending=ui.nodes.saveImport.click();await flush();ui.nodes.unifiedWideMapping.value='Data; 2; A; B';await ui.nodes.unifiedWideMapping.fire('input');answer.resolve(true);await pending;assert.equal(calls.pickers,0);assert.equal(calls.applies,0);}
 {const {ui,calls,answer}=await reviewedReplacement(),backup=deferred();ui.UI.api.chooseSqliteBackupPath=()=>{calls.pickers++;return backup.promise;};const pending=ui.nodes.saveImport.click();answer.resolve(true);await flush();assert.equal(calls.pickers,1);assert.equal(calls.applies,0);ui.UI.revision=2;backup.resolve('/tmp/backup.sqlite3');await pending;assert.equal(calls.applies,0);}
});
test('confirmed report delete uses only its revision and id even when no backup picker is available',async()=>{const {ui,calls,answer,action}=await deletableReport();delete ui.UI.api.chooseSqliteBackupPath;const args=[];ui.UI.api.nativeDeleteReport=async(...values)=>args.push(values);const pending=action.click();answer.resolve(true);await pending;assert.equal(calls.pickers,0);assert.deepEqual(JSON.parse(JSON.stringify(args)),[[1,'r']]);});
test('successful XLSX commit remains successful when view refresh fails',async()=>{
 let applies=0;
 const ui=mount({chooseWideXlsx:async()=>'/tmp/book.xlsx',nativeLegacyImportApply:async()=>{applies++;return {reports:1,rows:2,revision:2};}});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 ui.UI.refresh=async()=>{throw Error('refresh unavailable');};
 await ui.nodes.saveImport.click();
 assert.equal(applies,1);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Сохранено отчётов: 1/);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Не удалось обновить отображение/);
 assert.doesNotMatch(ui.nodes.unifiedImportStatus.textContent,/Не сохранено/);
 await ui.nodes.saveImport.click();assert.equal(applies,1,'review must be invalidated after commit');
});
test('successful report deletion is not called a failure when view refresh fails',async()=>{
 const {ui,calls,answer,notices,action}=await deletableReport();
 ui.UI.refresh=async()=>{throw Error('refresh unavailable');};
 const pending=action.click();answer.resolve(true);await pending;
 assert.equal(calls.deletes,1);
 assert.match(notices.at(-1),/Отчёт удалён из SQLite/);
 assert.match(notices.at(-1),/не удалось обновить отображение/i);
 assert.doesNotMatch(notices.at(-1),/Не удалось удалить отчёт/);
});
test('saved daily-folder rule stays saved when view refresh fails',async()=>{
 let writes=0;
 const ui=mount({
  chooseWideXlsx:async()=>'/tmp/book.xlsx',
  nativeWideImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,rowCount:1,conflicts:{},quality:{},sampleReports:[]}),
  nativeImportFolderContext:async()=>({folder:'/tmp',name:'book.xlsx',nativeSignature:'sig',rule:null,revision:5}),
  nativeSetRuleChecked:async()=>{writes++;return {revision:6};}
 });
 await ui.nodes.excelFile.click();
 ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');
 ui.nodes.dailyYear.value='2024';await ui.nodes.inspectFile.click();
 ui.UI.refresh=async()=>{throw Error('refresh unavailable');};
 await ui.nodes.saveDailyFolderRule.click();
 assert.equal(writes,1);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Правило файла сохранено в SQLite/);
 assert.doesNotMatch(ui.nodes.unifiedImportStatus.textContent,/Правило не сохранено/);
});

test('choosing a report calendar year loads the confirmed year and keeps the same focus target',async()=>{
 const years=[],ui=mount({unifiedCalendar:async(_revision,year)=>{years.push(year);return {days:[]};}});
 let open;
 ui.window.referenceCalendar={open:options=>{open=options;}};
 await ui.refresh();
 const first=years.at(-1),button=ui.nodes.referenceReportYear;
 await button.click();assert.equal(open.anchor,button);assert.equal(open.unit,'year');
 assert.equal(open.initial,String(first)+'-01-01');assert.equal(years.length,1);
 assert.equal(open.allowed.has('9999-01-01'),false);assert.equal(open.allowed.has('0001-01-01'),true);
 open.apply('2020-01-01');await flush();
 assert.deepEqual(years,[first,2020]);assert.equal(ui.nodes.referenceReportYear,button);
 assert.equal(ui.nodes.calendarRange.children[0],button);assert.match(button.textContent,/2020/);
 await ui.nodes.calendarNext.click();await flush();assert.equal(years.at(-1),2021);
});

test('saved and staged quality export the full native query, invalidate stale destinations and keep advice and Excel addresses',async()=>{
 const saved=[],staged=[],reads=[];
 const page={issues:[{date:'2024-01-01',kind:'invalid',reason:'Координата',source_row:11,sheet:'Лист A',cells:['D11']}],total:64,next_offset:50,counts:{invalid:64}};
 const ui=mount({chooseWideXlsx:async()=>'/tmp/reviewed.xlsx',unifiedQualityPage:async()=>page,nativePreviewQualityPage:async(...args)=>{reads.push(args);return page;},chooseCsvPath:async()=>'/tmp/selected.csv',unifiedQualityCsv:async(...args)=>saved.push(args),nativePreviewQualityCsv:async(...args)=>staged.push(args)});
 await ui.refresh();
 assert.match(text(ui.nodes.qualityTable),/Как проверить и исправить/);
 assert.match(text(ui.nodes.qualityTable),/'Лист A'!D11/);
 assert.deepEqual(Array.from(ui.nodes.qualityTable.children[0].headerHints),['Строка Excel','Ячейка Excel']);
 await ui.nodes.qualityExportCsv.click();assert.equal(saved.length,1);
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-01-01';await ui.nodes.inspectFile.click();
 ui.nodes.qualitySource.value='pending';await ui.nodes.qualitySource.fire('change');await flush();
 ui.nodes.qualitySearch.value='Координата';await ui.nodes.qualitySearch.fire('input');await flush();
 await ui.nodes.qualityTable.scrollWindow.go(1);await flush();
 assert.equal(reads.at(-1)[1],50);
 assert.deepEqual(Array.from(ui.nodes.qualityTable.children[0].headerHints),['Строка Excel','Ячейка Excel']);
 await ui.nodes.qualityExportCsv.click();
 assert.equal(staged[0][0].source,'/tmp/reviewed.xlsx');assert.equal(staged[0][0].selection.format,'legacy');assert.equal(staged[0][0].reviewToken,'a'.repeat(64));assert.equal(staged[0][0].filter.search,'Координата');
 let finish;ui.UI.api.chooseCsvPath=()=>new Promise(r=>finish=r);
 const opened=ui.nodes.qualityExportCsv.click();await flush();
 ui.nodes.importWeek.value='2024-01-02';await ui.nodes.importWeek.fire('change');
 finish('/tmp/wrong-source.csv');await opened;
 assert.equal(staged.length,1);assert.match(ui.nodes.qualitySummary.textContent,/Сначала проверьте/);
});

test('a failed native picker keeps the reviewed workbook and source together',async()=>{
 let first=true;const ui=mount({chooseWideXlsx:async()=>{if(first){first=false;return '/tmp/first.xlsx';}throw Error('picker failed');}});
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-01-01';await ui.nodes.inspectFile.click();
 await ui.nodes.excelFile.click();assert.equal(ui.nodes.saveImport.disabled,false);assert.equal(ui.nodes.inspectFile.disabled,false);
 assert.equal(ui.nodes.excelFileStatus.textContent,'first.xlsx');assert.match(ui.nodes.unifiedImportStatus.textContent,/picker failed/);
});

test('quality calendar uses only observed staged period starts, including dates beyond preview samples',async()=>{
 const ui=mount({
  managementReportWindow:async()=>({rows:[{id:'saved',report_type:'week',start:'2023-08-07',end:'2023-08-13'}],total_rows:1}),
  chooseWideXlsx:async()=>'/tmp/periods.xlsx',
  nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:83,rowCount:84,conflicts:{},quality:{},sampleReports:[],qualityPeriodMonths:{'day:2024-02':(1<<28)|1,'day:2024-03':2},qualityPeriodsPartial:false}),
  nativePreviewQualityPage:async()=>({issues:[],total:0,counts:{},next_offset:null})
 });
 const values=()=>ui.nodes.qualityPeriod.children.map(o=>o.value);
 await ui.refresh();assert.deepEqual(values(),['all','week:2023-08-07']);
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-01-01';await ui.nodes.inspectFile.click();
 ui.nodes.qualityPeriod.value='week:2023-08-07';ui.nodes.qualitySource.value='pending';await ui.nodes.qualitySource.fire('change');await flush();
 assert.deepEqual(values(),['all','day:2024-02-01','day:2024-02-29','day:2024-03-02']);assert.equal(ui.nodes.qualityPeriod.value,'all');
 assert.equal(ui.nodes.qualityPeriodHint.textContent,'');
 ui.nodes.importWeek.value='2024-01-03';await ui.nodes.importWeek.fire('change');assert.deepEqual(values(),['all']);
 ui.nodes.qualitySource.value='saved';await ui.nodes.qualitySource.fire('change');assert.deepEqual(values(),['all','week:2023-08-07']);
});
test('partial older staged metadata lists only confirmed dates and explains manual date range',async()=>{
 const ui=mount({chooseWideXlsx:async()=>'/tmp/review.xlsx',nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:15,rowCount:46,conflicts:{},quality:{},sampleReports:[{kind:'day',start:'2024-02-03',rows:1}]}),nativePreviewQualityPage:async()=>({issues:[],total:0,counts:{},next_offset:null})});
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-02-01';await ui.nodes.inspectFile.click();
 ui.nodes.qualitySource.value='pending';await ui.nodes.qualitySource.fire('change');await flush();
 assert.deepEqual(ui.nodes.qualityPeriod.children.map(o=>o.value),['all','day:2024-02-03']);assert.match(ui.nodes.qualityPeriodHint.textContent,/в полях «С» и «По»/);
});


test('import opening makes no calendar/list/quality reads; each history view loads only its own bounded query',async()=>{
 const calls=[];const ui=mount({managementReportWindow:async()=>{calls.push('reports');return {rows:[],total_rows:0};},managementReportSummary:async()=>{calls.push('summary');return {reports:0,rows:0,amount:null};},unifiedCalendar:async()=>{calls.push('calendar');return {days:[]};},nativeDailyRollup:async()=>{calls.push('rollup');return {points:[],next_cursor:null};},unifiedQualityPage:async()=>{calls.push('quality');return {issues:[],total:0,counts:{}};}});
 await ui.view();ui.snapshot();await flush();assert.deepEqual(calls,[]);
 await ui.window.unifiedReports.openView('list');assert.deepEqual(calls,['reports','summary']);calls.length=0;
 await ui.window.unifiedReports.openView('calendar');assert.deepEqual(calls,['calendar','rollup','rollup','rollup']);calls.length=0;
 await ui.window.unifiedReports.openView('quality');assert.deepEqual(calls,['quality']);
});
test('a legacy folder rule resolves the same logical second sheet as native folder import',async()=>{
 let received;const ui=mount({chooseWideXlsx:async()=>'/excel/two.xlsx',nativeImportSheets:async()=>[{name:'Первый',component:'xl/first.xml'},{name:'Второй',component:'xl/second.xml'}],nativeImportFolderContext:async()=>({folder:'/excel',name:'two.xlsx',revision:1,nativeSignature:'sig',rule:{format:'legacy',type:'week',start:'2024-02-05',sheet:'Второй'}}),nativeLegacyImportPreview:async(_,options)=>{received=options;return {reviewToken:'a'.repeat(64),reportCount:1,conflicts:{},quality:{}};}});
 await ui.nodes.excelFile.click();assert.equal(ui.nodes.sheetSelect.value,'xl/second.xml');await ui.nodes.inspectFile.click();assert.equal(received.sheet,'xl/second.xml');assert.equal(received.kind,'week');assert.equal(received.start,'2024-02-05');
});
test('changing parameters clears displayed old numbers immediately, including while preview is pending',async()=>{
 const ui=mount({chooseWideXlsx:async()=>'/tmp/A.xlsx'});await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();assert.ok(ui.nodes.importPreview.children.length);
 await ui.nodes.importWeek.fire('change');assert.equal(ui.nodes.importPreview.children.length,0);assert.equal(ui.nodes.saveImport.disabled,true);
 let finish;ui.UI.api.nativeLegacyImportPreview=()=>new Promise(r=>finish=r);const pending=ui.nodes.inspectFile.click();await flush();await ui.nodes.importMonth.fire('change');finish({reviewToken:'b'.repeat(64),reportCount:99,quality:{},conflicts:{}});await pending;assert.equal(ui.nodes.importPreview.children.length,0);assert.equal(ui.nodes.saveImport.disabled,true);
});
test('daily rule action exists only for a checked file in connected folder',async()=>{
 for(const path of ['/outside/day.xlsx','/excel/day.xlsx']){const ui=mount({chooseWideXlsx:async()=>path,nativeImportFolderContext:async source=>source=== '/excel/day.xlsx'?{folder:'/excel',name:'day.xlsx',revision:1,nativeSignature:'sig',rule:null}:null,nativeSetRuleChecked:async()=>{},nativeWideImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}})});await ui.nodes.excelFile.click();ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');await ui.nodes.inspectFile.click();assert.equal(ui.nodes.saveDailyFolderRule.hidden,path.startsWith('/outside'));assert.equal(ui.nodes.sheetSelect.parentElement.hidden,true);assert.notEqual(ui.nodes.unifiedYearDetails.open,true);}
});
test('saved weekly import opens its exact native range through the shared filter seam',async()=>{
 const range={kind:'week',from:'2024-02-05',to:'2024-03-03'},opened=[],ui=mount({chooseWideXlsx:async()=>'/tmp/week.xlsx',nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:4,rowCount:4,quality:{},conflicts:{},periodRange:range})});ui.window.referenceFilters={openImportedPeriod:r=>opened.push(r)};ui.window.referenceDesktop={navigate:id=>opened.push(id)};await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();assert.match(text(ui.nodes.importPreview),/2024-02-05 — 2024-03-03/);await ui.nodes.saveImport.click();const action=ui.nodes.importPreview.querySelectorAll('button').find(b=>b.textContent==='Открыть загруженный период');assert.ok(action);await action.click();assert.deepEqual(opened,[range,'s1']);
});
test('saved report count wording remains grammatical for 1,2,5,11,21',async()=>{
 for(const reports of [1,2,5,11,21]){const ui=mount({chooseWideXlsx:async()=>'/tmp/count.xlsx',nativeLegacyImportApply:async()=>({reports,rows:reports,revision:2})});await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();await ui.nodes.saveImport.click();assert.match(ui.nodes.unifiedImportStatus.textContent,new RegExp('Сохранено отчётов: '+reports+' ·'));}
});

test('saved auto rule uses native format for daily and ordinary books through chooser and folder entry',async()=>{
 for(const dailyWide of [true,false])for(const entry of ['chooser','folder']){
  const calls=[],rule={format:'auto',type:'week',start:'2024-02-05',sheet:'Second'};
  const ui=mount({chooseFolder:async()=>null,chooseWideXlsx:async()=>'/excel/two.xlsx',list:async()=>({folder:'/excel',files:[],rules:{}}),nativeImportFolderContext:async()=>({folder:'/excel',name:'two.xlsx',revision:1,nativeSignature:'sig',rule}),nativeImportSheets:async(source,received)=>{calls.push({source,rule:received});return [{name:'First',component:'xl/first.xml',dailyWide:false},{name:'Second',component:'xl/second.xml',dailyWide}];},nativeLegacyImportPreview:async(_,options)=>{calls.push({format:'legacy',options});return {reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}};},nativeWideImportPreview:async(_,options)=>{calls.push({format:'daily-wide',options});return {reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}};}});
  if(entry==='chooser')await ui.nodes.excelFile.click();else await ui.window.unifiedReports.openSource('/excel/two.xlsx');
  assert.equal(calls[0].rule,rule);assert.equal(ui.nodes.importFormat.value,dailyWide?'daily-wide':'legacy');
  await ui.nodes.inspectFile.click();assert.equal(calls[1].format,dailyWide?'daily-wide':'legacy');
  if(dailyWide){assert.deepEqual(Array.from(calls[1].options.sheets),['Second']);assert.equal(calls[1].options.kind,'week');assert.equal(calls[1].options.year,null);}else{assert.equal(calls[1].options.sheet,'xl/second.xml');assert.equal(calls[1].options.kind,'week');assert.equal(calls[1].options.start,'2024-02-05');}
 }
});
test('auto-rule detection failure cannot silently choose legacy or retain an older review',async()=>{
 const ui=mount({chooseWideXlsx:async()=>'/excel/a.xlsx',nativeImportFolderContext:async()=>({folder:'/excel',name:'a.xlsx',revision:1,nativeSignature:'sig',rule:{format:'auto',type:'week',start:'2024-01-01',sheet:'Дни'}})});await ui.nodes.excelFile.click();assert.match(ui.nodes.unifiedImportStatus.textContent,/однозначно определить структуру/);assert.equal(ui.nodes.inspectFile.disabled,true);assert.equal(ui.nodes.saveImport.disabled,true);assert.equal(ui.nodes.importPreview.children.length,0);
});

test('a local workbook can be reviewed and saved when enumeration of the connected folder is unavailable',async()=>{
 const calls=[];
 const ui=mount({chooseWideXlsx:async()=>'/local/month.xlsx',
  list:async()=>{calls.push('folder list');throw Error('offline share');},
  nativeImportFolderContext:async path=>{assert.equal(path,'/local/month.xlsx');return null;},
  nativeLegacyImportApply:async()=>{calls.push('apply');return {reports:1,rows:1,revision:2};}});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 assert.equal(ui.nodes.saveDailyFolderRule.hidden,true);
 assert.equal(ui.nodes.saveImport.disabled,false);
 await ui.nodes.saveImport.click();
 assert.deepEqual(calls,['apply']);
});

test('unresolved folder sheet stays open and requires an explicit choice before preview',async()=>{
 const sheets=[{name:'Current',component:'first.xml'},{name:'Other',component:'second.xml'}];
 for(const stale of ['', 'Old name']){
  const calls=[],ui=mount({chooseWideXlsx:async()=>'/folder/book.xlsx',
   nativeImportSheets:async()=>sheets,nativeImportFolderContext:async()=>({name:'book.xlsx',folder:'/folder',revision:1,
    nativeSignature:'sig',rule:{format:'legacy',type:'week',start:'2024-02-05',sheet:stale}}),
   nativeLegacyImportPreview:async(_path,opts)=>{calls.push(opts.sheet);return {reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}};}});
  await ui.nodes.excelFile.click();
  assert.equal(ui.nodes.excelFileStatus.textContent,'book.xlsx');
  assert.equal(ui.nodes.sheetSelect.value,'');assert.equal(ui.nodes.sheetSelect.disabled,false);
  await ui.nodes.inspectFile.click();assert.deepEqual(calls,[]);
  assert.equal(ui.nodes.saveImport.disabled,true);
  ui.nodes.sheetSelect.value='second.xml';await ui.nodes.sheetSelect.fire('change');await ui.nodes.inspectFile.click();
  assert.deepEqual(calls,['second.xml']);assert.equal(ui.nodes.saveImport.disabled,false);
 }
});

test('a renamed daily sheet can be reselected and its verified folder rule repaired',async()=>{
 const file={folder:String.raw`\\?\C:\Excel`,name:'quarter.xlsx',revision:8,nativeSignature:'reviewed',
  rule:{format:'daily-wide',type:'day',year:2024,sheets:['Old'],mappings:{Old:{workCol:0,headerRow:1,firstDayCol:2}}}};
 const previews=[],writes=[],ui=mount({chooseWideXlsx:async()=>String.raw`C:\Excel\quarter.xlsx`,
  nativeImportFolderContext:async()=>file,nativeImportSheets:async()=>[{name:'New',component:'new.xml'}],
  nativeWideImportPreview:async(_,opts)=>{previews.push(Array.from(opts.sheets));return {reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}};},
  nativeSetRuleChecked:async(...args)=>{writes.push(args);}});
 await ui.nodes.excelFile.click();
 assert.equal(ui.nodes.unifiedWideSheetMode.value,'selected');assert.equal(ui.nodes.unifiedWideSheet0.checked,false);
 assert.equal(ui.nodes.unifiedWideMapping.value,'');
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.deepEqual(previews,[]);assert.deepEqual(writes,[]);
 ui.nodes.unifiedWideSheet0.checked=true;await ui.nodes.unifiedWideSheet0.fire('change');
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.deepEqual(previews,[['New']]);assert.equal(writes.length,1);
 const [name,rule,revision,folder,signature]=writes[0];
 assert.deepEqual([name,revision,folder,signature],[file.name,file.revision,file.folder,file.nativeSignature]);
 assert.deepEqual(Array.from(rule.sheets),['New']);assert.equal(rule.mappings,undefined);
});

test('saving a rule after importing the same book uses the receipt revision and original signature',async()=>{
 const writes=[],file={folder:'/excel',name:'book.xlsx',revision:1,nativeSignature:'first-signature',rule:null};
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',nativeImportFolderContext:async()=>file,
  nativeWideImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,quality:{},conflicts:{}}),
  nativeWideImportApply:async()=>({reports:1,rows:1,revision:2}),
  nativeSetRuleChecked:async(...args)=>{writes.push(args);}});
 await ui.nodes.excelFile.click();ui.nodes.importFormat.value='daily-wide';await ui.nodes.importFormat.fire('change');
 await ui.nodes.inspectFile.click();await ui.nodes.saveImport.click();
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.equal(writes.length,1);assert.deepEqual(writes[0].slice(2),[2,'/excel','first-signature']);
});

const importReview={reviewToken:'b'.repeat(64),reportCount:2,rowCount:2,conflicts:{},quality:{}};
const changeImportField=async(ui,id,value)=>{
 ui.nodes[id].value=value;
 await ui.nodes[id].fire('change');
};

for(const context of ['sheet','first week','first month','type','format','column sets']){
 test('per-set dates are scoped to the reviewed layout when changing '+context,async()=>{
  const previews=[],writes=[];
  const ui=mount({chooseWideXlsx:async()=>'/tmp/book.xlsx',
   nativeImportSheets:async()=>[{name:'A',component:'a.xml'},{name:'B',component:'b.xml'}],
   nativeLegacyImportPreview:async(path,options)=>{previews.push(options);return importReview;},
   nativeLegacyImportApply:async(...args)=>{writes.push(args);return {revision:2,reports:2,rows:2};}});
  await ui.nodes.excelFile.click();
  await changeImportField(ui,'importType',context==='first month'?'month':'week');
  if(context==='first month')await changeImportField(ui,'importMonth','2024-01');
  const dates=()=>ui.nodes.importPeriodDates.querySelectorAll('input[data-ordinal]');
  await ui.nodes.inspectFile.click();
  dates()[1].value=context==='first month'?'2024-04-01':'2024-01-22';
  await dates()[1].fire('change');await ui.nodes.inspectFile.click();
  assert.deepEqual(Array.from(previews.at(-1).periodDates),['2024-01-01',dates()[1].value]);
  if(context==='sheet')await changeImportField(ui,'sheetSelect','b.xml');
  if(context==='first week')await changeImportField(ui,'importWeek','2024-02-05');
  if(context==='first month')await changeImportField(ui,'importMonth','2024-06');
  if(context==='type')await changeImportField(ui,'importType','day');
  if(context==='format'){
   await changeImportField(ui,'importFormat','daily-wide');
   await changeImportField(ui,'importFormat','legacy');
  }
  if(context==='column sets'){
   await changeImportField(ui,'importMappingMode','manual');
   const row=ui.nodes.importColumnMappings.children[0];
   for(const [key,value] of Object.entries({header_row:'1',work_col:'A',count_col:'B',amount_col:'C'})){
    const input=row.querySelector('[data-field="'+key+'"]');input.value=value;await input.fire('input');
   }
  }
  await ui.nodes.saveImport.click();assert.equal(writes.length,0);
  await ui.nodes.inspectFile.click();
  const reviewed=previews.at(-1);
  assert.equal(reviewed.periodDates,undefined);
  if(context==='sheet')assert.equal(reviewed.sheet,'b.xml');
  if(context==='first week')assert.deepEqual(dates().map(d=>d.value),['2024-02-05','2024-02-12']);
  if(context==='first month')assert.deepEqual(dates().map(d=>d.value),['2024-06-01','2024-07-01']);
  await ui.nodes.saveImport.click();assert.equal(writes.length,1);
  assert.deepEqual(writes[0].slice(0,3),['/tmp/book.xlsx',reviewed,importReview.reviewToken]);
 });
}

test('two daily rule saves update their row and use consecutive checked receipts with the original signature',async()=>{
 const file={name:'book.xlsx',nativeSignature:'sig',size:10,tooLarge:false};
 const preceding=Array.from({length:100},(_,i)=>({...file,name:i+'.xlsx'}));
 let revision=8,rule=null;const writes=[];
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',chooseFolder:async()=>null,
  list:async()=>({folder:'/excel',revision,files:[...preceding,file],rules:rule?{[file.name]:rule}:{}}),
  nativeImportFolderContext:async()=>({folder:'/excel',name:file.name,nativeSignature:file.nativeSignature,revision,rule}),
  nativeWideImportPreview:async()=>importReview,
  nativeSetRuleChecked:async(name,next,expected,folder,signature)=>{
   writes.push({name,year:next.year,expected,folder,signature});
   if(expected!==revision||signature!==file.nativeSignature)throw Error('stale selection');
   rule=next;return ++revision;
  }});
 ui.UI.revision=revision;
 await ui.startFolder();
 const rows=ui.nodes.sourceFolderRows;
 await rows.scrollWindow.go(1);
 assert.match(text(rows),/Правило не задано/);
 await ui.nodes.excelFile.click();await changeImportField(ui,'importFormat','daily-wide');
 for(const year of [2024,2025]){
  await changeImportField(ui,'dailyYear',String(year));
  await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 }
 assert.deepEqual(writes.map(w=>[w.expected,w.year,w.folder,w.signature]),[[8,2024,'/excel','sig'],[9,2025,'/excel','sig']]);
 assert.equal(rule.year,2025);
 assert.match(text(rows),/book\.xlsx.*Есть правило/);
 ++revision;
 await changeImportField(ui,'dailyYear','2026');await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Правило не сохранено/);
 assert.equal(rule.year,2025);
});

test('advanced daily mappings round-trip a sheet name containing semicolons through inspection and rule save',async()=>{
 const name='I; 2; квартал';
 const mapping={headerRow:1,workCol:0,firstDayCol:1,totalCountCol:3,totalAmountCol:4};
 const rule={format:'daily-wide',type:'day',sheets:['I',name],mappings:{I:mapping,[name]:mapping}};
 const previews=[],writes=[];
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',
  nativeImportSheets:async()=>[{name:'I',component:'a.xml'},{name,component:'b.xml'},{name:'I; 2; A',component:'c.xml'}],
  nativeImportFolderContext:async()=>({name:'book.xlsx',folder:'/excel',revision:1,nativeSignature:'sig',rule}),
  nativeWideImportPreview:async(_path,options)=>{previews.push(options);return importReview;},
  nativeSetRuleChecked:async(_name,updated)=>{writes.push(updated);return 2;}});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 assert.equal(previews.length,1,ui.nodes.unifiedImportStatus.textContent);
 assert.deepEqual(Object.keys(previews[0].mappings),['I',name]);
 assert.deepEqual(JSON.parse(JSON.stringify(previews[0].mappings[name])),{
  header_row:2,work_col:1,first_day_col:2,total_count_col:4,total_amount_col:5});
 await ui.nodes.saveDailyFolderRule.click();
 assert.deepEqual(JSON.parse(JSON.stringify(writes[0].mappings)),rule.mappings);
});

test('explicitly reopening a workbook reapplies its saved folder rule after an unsaved manual mapping',async()=>{
 for(const entry of ['picker','folder']){
  const inspected=[],source='/excel/book.xlsx',rule={format:'legacy',type:'week',sheet:'B',start:'2031-01-08'};
  const ui=mount({chooseWideXlsx:async()=>source,chooseFolder:async()=>null,list:async()=>({}),
   nativeImportFolderContext:async()=>({folder:'/excel',revision:1,name:'book.xlsx',nativeSignature:'sig',rule}),
   nativeImportSheets:async()=>[{name:'A',component:'a.xml'},{name:'B',component:'b.xml'}],
   nativeLegacyImportPreview:async(_path,options)=>{inspected.push(options);return importReview;}});
  await ui.nodes.excelFile.click();
  await changeImportField(ui,'importMappingMode','manual');
  const row=ui.nodes.importColumnMappings.children[0];
  for(const [key,value] of Object.entries({header_row:'2',work_col:'D',count_col:'E',amount_col:'F'})){
   const input=row.querySelector('[data-field="'+key+'"]');input.value=value;await input.fire('input');
  }
  await ui.nodes.inspectFile.click();
  await changeImportField(ui,'importWeek','2031-02-05');await ui.nodes.inspectFile.click();
  assert.equal(inspected.at(-1).mappings[0].work_col,4);
  ui.nodes.replaceExisting.checked=true;
  if(entry==='picker')await ui.nodes.excelFile.click();
  else await ui.window.unifiedReports.openSource(source);
  await ui.nodes.inspectFile.click();
  const reopened=inspected.at(-1);
  assert.equal(reopened.mappings,null);assert.equal(reopened.sheet,'b.xml');assert.equal(reopened.start,rule.start);
  assert.equal(ui.nodes.importMappingMode.value,'auto');assert.equal(ui.nodes.importColumnMappings.children.length,0);
  assert.equal(ui.nodes.replaceExisting.checked,false);
 }
});

test('manual blocks cannot reuse any column from a preceding set',async()=>{
 const seen=[];
 const ui=mount({chooseWideXlsx:async()=>'/xls/pay.xlsx',nativeLegacyImportPreview:async(_,o)=>{seen.push(o);return importReview;}});
 await ui.nodes.excelFile.click();await changeImportField(ui,'importMappingMode','manual');
 await ui.nodes.addImportMapping.click();
 const [first,second]=ui.nodes.importColumnMappings.children;
 for(const row of [first,second])for(const [field,value] of Object.entries({header_row:'1',work_col:'A',count_col:'B',amount_col:'C'}))
  row.querySelector('[data-field="'+field+'"]').value=value;
 await ui.nodes.inspectFile.click();assert.equal(seen.length,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/Столбец A.*нескольких наборах/);
 second.querySelector('[data-field="work_col"]').value='D';second.querySelector('[data-field="count_col"]').value='E';
 await ui.nodes.inspectFile.click();assert.equal(seen.length,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/Столбец C/);
 second.querySelector('[data-field="amount_col"]').value='F';await ui.nodes.inspectFile.click();assert.equal(seen.length,1);
});

test('refreshing a rule after an unrelated write preserves edits and rejects changes to the same source',async()=>{
 const rule={type:'day',format:'daily-wide',sheets:['Дни'],year:2025};
 const context={folder:'/excel',name:'book.xlsx',nativeSignature:'sig',revision:1,rule};
 const writes=[];
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',nativeImportFolderContext:async()=>structuredClone(context),
  nativeWideImportPreview:async()=>importReview,nativeSetRuleChecked:async(name,next,rev)=>{
   assert.equal(rev,context.revision);context.rule=next;writes.push(next);return ++context.revision;
  }});
 await ui.nodes.excelFile.click();await changeImportField(ui,'dailyYear','2026');
 ui.UI.revision=context.revision=2;
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.equal(writes.length,1);assert.equal(writes[0].year,2026);
 context.rule={...context.rule,year:2027};ui.UI.revision=context.revision=4;
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();assert.equal(writes.length,1);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/правило изменились.*Выберите файл заново/);
});

test('a newline in a worksheet name round-trips in the ordinary text editor for wide mappings',async()=>{
 const name='Апрель\nМай', mapping={headerRow:17,workCol:0,firstDayCol:1};
 const rule={type:'day',format:'daily-wide',sheets:[name],mappings:{[name]:mapping}};
 let preview,saved;
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',nativeImportSheets:async()=>[{name,component:'sheet.xml'}],
  nativeImportFolderContext:async()=>({folder:'/excel',name:'book.xlsx',nativeSignature:'sig',revision:1,rule}),
  nativeWideImportPreview:async(_,opt)=>{preview=opt;return importReview;},nativeSetRuleChecked:async(_,next)=>{saved=next;return 2;}});
 await ui.nodes.excelFile.click();
 assert.equal(ui.nodes.unifiedWideMapping.value.split('\n').length,1);
 await ui.nodes.inspectFile.click();await ui.nodes.saveDailyFolderRule.click();
 assert.equal(preview.mappings[name].header_row,18);
 assert.deepEqual(JSON.parse(JSON.stringify(saved.mappings)),{[name]:mapping});
});

test('all eligible sheets ignores a draft sequential period and restores it in selected mode',async()=>{
 const previews=[],ui=mount({chooseWideXlsx:async()=>'/tmp/days.xlsx',nativeWideImportPreview:async(_,options)=>{previews.push(options);return importReview;}});
 await ui.nodes.excelFile.click();await changeImportField(ui,'importFormat','daily-wide');
 await changeImportField(ui,'unifiedWidePeriod0','q2');
 await changeImportField(ui,'unifiedWideSheetMode','auto');await ui.nodes.inspectFile.click();
 assert.deepEqual(previews.map(o=>[Array.from(o.sheets),Object.keys(o.sequential_periods)]),[[[],[]]]);
 assert.equal(ui.nodes.saveImport.disabled,false);
 await changeImportField(ui,'unifiedWideSheetMode','selected');await ui.nodes.inspectFile.click();
 assert.equal(previews.length,1);assert.match(ui.nodes.unifiedImportStatus.textContent,/год/);
 await changeImportField(ui,'dailyYear','2024');await ui.nodes.inspectFile.click();
 assert.equal(previews[1].sequential_periods['Дни'].quarter,2);
});

test('duplicate manual daily mappings must be resolved before preview or saving',async()=>{
 const previews=[],ui=mount({chooseWideXlsx:async()=>'/tmp/days.xlsx',nativeWideImportPreview:async(_,options)=>{previews.push(options);return importReview;}});
 await ui.nodes.excelFile.click();await changeImportField(ui,'importFormat','daily-wide');
 ui.nodes.unifiedWideMapping.value='Дни; 1; A; B\nДни; 2; D; E';await ui.nodes.unifiedWideMapping.fire('input');
 await ui.nodes.inspectFile.click();assert.equal(previews.length,0);assert.equal(ui.nodes.saveImport.disabled,true);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Дни/);
 ui.nodes.unifiedWideMapping.value='Дни; 2; D; E';await ui.nodes.unifiedWideMapping.fire('input');await ui.nodes.inspectFile.click();
 assert.equal(previews[0].mappings['Дни'].work_col,4);
});

test('daily mappings must use a selected sheet and cannot silently ignore an amount total',async()=>{
 const previews=[],ui=mount({chooseWideXlsx:async()=>'/tmp/days.xlsx',
  nativeImportSheets:async()=>[{name:'A',component:'a.xml'},{name:'B',component:'b.xml'}],
  nativeWideImportPreview:async(_,options)=>{previews.push(options);return importReview;}});
 await ui.nodes.excelFile.click();await changeImportField(ui,'importFormat','daily-wide');
 ui.nodes.unifiedWideSheet0.checked=true;await ui.nodes.unifiedWideSheet0.fire('change');
 for(const [line,error] of [['B; 1; A; B',/Отметьте лист «B»/],['A; 1; A; B; ; F',/итогового количества/]]){
  ui.nodes.unifiedWideMapping.value=line;await ui.nodes.unifiedWideMapping.fire('input');await ui.nodes.inspectFile.click();
  assert.equal(previews.length,0);assert.match(ui.nodes.unifiedImportStatus.textContent,error);
 }
 ui.nodes.unifiedWideMapping.value='A; 1; A; B; E; F';await ui.nodes.unifiedWideMapping.fire('input');await ui.nodes.inspectFile.click();
 assert.equal(previews[0].mappings.A.total_amount_col,6);
});

test('manual preview of a connected ordinary workbook rechecks a changed folder rule',async()=>{
 let revision=1,previews=0;
 const ui=mount({chooseWideXlsx:async()=>'/excel/book.xlsx',nativeImportFolderContext:async()=>({
  name:'book.xlsx',folder:'/excel',nativeSignature:'one',revision,
  rule:{format:'legacy',type:'day',start:revision===1?'2024-01-01':'2024-02-01',sheet:'Дни'}}),
  nativeLegacyImportPreview:async()=>{previews++;return importReview;}});
 await ui.nodes.excelFile.click();
 revision=ui.UI.revision=2;
 await ui.nodes.inspectFile.click();
 assert.equal(previews,0);assert.equal(ui.nodes.saveImport.disabled,true);
 assert.match(ui.nodes.unifiedImportStatus.textContent,/Выберите файл заново/);
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 assert.equal(previews,1);assert.equal(ui.nodes.importWeek.value,'2024-02-01');
});

test('replacement consent belongs to the reviewed periods and is reset after a new preview',async()=>{
 const ui=mount({chooseWideXlsx:async()=>'/tmp/book.xlsx',nativeLegacyImportPreview:async()=>({...importReview,conflicts:{replacements:1}})});
 await ui.nodes.excelFile.click();await ui.nodes.inspectFile.click();
 ui.nodes.replaceExisting.checked=true;
 await changeImportField(ui,'importWeek','2024-04-21');await ui.nodes.inspectFile.click();
 assert.equal(ui.nodes.replaceExisting.checked,false);
});

test('switching databases forgets the workbook and pending sheet selection even at an equal revision',async()=>{
 const choose=deferred();let previews=0;
 const ui=mount({chooseWideXlsx:async()=>'/a/book.xlsx',nativeImportSheets:()=>choose.promise,
  nativeLegacyImportPreview:async()=>{previews++;return importReview;}});
 const selection=ui.nodes.excelFile.click();await flush();
 ui.window.sharedWorkspace={state:{mode:'shared',folder:'B'}};
 ui.snapshot(null);ui.snapshot();choose.resolve([{name:'Старый лист',component:'old.xml'}]);await selection;
 await ui.nodes.inspectFile.click();assert.equal(previews,0);assert.equal(ui.nodes.inspectFile.disabled,true);assert.equal(ui.nodes.excelFile.disabled,false);
 assert.doesNotMatch(ui.nodes.excelFileStatus.textContent||'',/book\.xlsx/);
});

test('a pending native review cannot repopulate the form after a same-revision refresh',async()=>{
 const preview=deferred();const ui=mount({chooseWideXlsx:async()=>'/a/book.xlsx',nativeLegacyImportPreview:()=>preview.promise});
 await ui.nodes.excelFile.click();const pending=ui.nodes.inspectFile.click();await flush();
 ui.snapshot(null);ui.snapshot();preview.resolve(importReview);await pending;
 assert.equal(ui.nodes.saveImport.disabled,true);assert.match(ui.nodes.unifiedImportStatus.textContent,/изменились/);
});

test('each report period supports both layouts and keeps the selected kind in preview',async()=>{
 for(const kind of ['day','week','month']){
  const calls=[];
  const ui=mount({chooseWideXlsx:async()=>'/tmp/paired.xlsx',
   nativeImportSheets:async()=>[{name:'Работы',component:'sheet.xml',layout:'daily-wide'}],
   nativeWideImportPreview:async(_,options)=>{calls.push(['shared',options]);return importReview;},
   nativeLegacyImportPreview:async(_,options)=>{calls.push(['blocks',options]);return importReview;}});
  await ui.nodes.excelFile.click();
  assert.equal(ui.nodes.importFormat.value,'daily-wide');
  await changeImportField(ui,'importType',kind);
  assert.equal(ui.nodes.importType.disabled,false);
  await changeImportField(ui,kind==='month'?'importMonth':'importWeek',kind==='month'?'2024-02':'2024-02-01');
  await ui.nodes.inspectFile.click();
  await changeImportField(ui,'importFormat','legacy');
  assert.equal(ui.nodes.saveImport.disabled,true);
  await ui.nodes.inspectFile.click();
  assert.deepEqual(calls.map(([layout,options])=>[layout,options.kind,options.start]),[
   ['shared',kind,'2024-02-01'],['blocks',kind,'2024-02-01']]);
 }
});

test('ambiguous or mixed Excel layouts require an explicit choice and allow manual override',async()=>{
 for(const layouts of [[null],['legacy','daily-wide']]){
  const ui=mount({chooseWideXlsx:async()=>'/tmp/ambiguous.xlsx',
   nativeImportSheets:async()=>layouts.map((layout,i)=>({name:'Лист '+i,component:'sheet'+i,layout}))});
  await ui.nodes.excelFile.click();
  assert.equal(ui.nodes.importFormat.value,'');assert.equal(ui.nodes.inspectFile.disabled,true);
  await changeImportField(ui,'importFormat','legacy');
  assert.equal(ui.nodes.inspectFile.disabled,false);
  await ui.nodes.inspectFile.click();assert.equal(ui.nodes.saveImport.disabled,false);
 }
});

test('a classic multi-sheet daily calendar supersedes the ordinary branch of a saved auto rule',async()=>{
 const rule={format:'auto',type:'week',start:'2031-01-08',sheet:'Second'},calls=[];
 const ui=mount({chooseWideXlsx:async()=>'/tmp/quarters.xlsx',
  nativeImportFolderContext:async()=>({folder:'/tmp',name:'quarters.xlsx',revision:1,nativeSignature:'current',rule}),nativeImportSheets:async()=>['Q1','Q2','Q3','Q4'].map(name=>({name,component:name+'.xml',layout:'daily-wide',dailyWide:true,classicDaily:true})),
  nativeWideImportPreview:async(_,options)=>{calls.push(options);return importReview;}});
 await ui.nodes.excelFile.click();
 assert.equal(ui.nodes.importFormat.value,'daily-wide');assert.equal(ui.nodes.importType.value,'day');
 await ui.nodes.inspectFile.click();
 assert.equal(calls[0].kind,'day');assert.equal(calls[0].start,null);assert.deepEqual(Array.from(calls[0].sheets),[]);
});
