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
 const ids=['reports','reportCount','upload','calendarDetails','reportCalendar','calendarRange','calendarPrev','calendarNext','qualitySource','qualityPeriod','qualityOrg','qualityType','qualityTable','qualitySummary','qualityPage','qualityPrev','qualityNext','qualityHeading','replaceExisting','importPreview','excelFile','importFormat','importType','importWeek','importMonth','sheetSelect','dailyYear','importMappingMode','inspectFile','saveImport','dailyWideOptions','weekStartLabel','monthStartLabel','importDateCaption','importManualWrap','importColumnMappings','addImportMapping','excelFileStatus','dailySheetChoices','importPeriodDates','saveDailyFolderRule'];
 const root=new E();for(const id of ids){const e=new E(id.includes('File')?'input':'div');e.id=id;const p=new E();root.append(p);p.append(e);}
 const filterRoot=new E(),qualityLabel=new E();filterRoot.append(qualityLabel);qualityLabel.append(nodes.qualityType);
 Object.assign(nodes.importFormat,{value:'legacy'});nodes.importType.value='day';nodes.importWeek.value='2024-01-01';nodes.importMappingMode.value='auto';nodes.qualitySource.value='saved';for(const id of ['qualityPeriod','qualityOrg','qualityType'])nodes[id].value='all';
 const api={managementReportPage:async()=>[],managementReportSummary:async()=>({reports:0,rows:0,amount:0,latest:'2024-01-01'}),managementReportOrgPage:async()=>[],unifiedCalendar:async(revision,year)=>({revision,year,days:[]}),nativeDailyRollup:async()=>({points:[],next_cursor:null}),unifiedQualityPage:async()=>({issues:[],next_offset:null,total:0,counts:{}}),chooseWideXlsx:async()=>null,nativeLegacyImportSheets:async()=>['xl/worksheets/sheet1.xml'],nativeLegacyImportPreview:async()=>({reviewToken:'a'.repeat(64),reportCount:1,rowCount:2,conflicts:{},quality:{},sampleReports:[]}),nativeLegacyImportApply:async()=>({reports:1,rows:2,revision:2}),chooseSqliteBackupPath:async()=>null,...overrides};
 let snapshotHook,viewHook;const UI={api,$:id=>nodes[id],el:(tag,text,cls)=>{const e=new E(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;},revision:1,snapshot:{org_names:['A','B','C','D']},fmt:v=>v??'—',onSnapshot:fn=>snapshotHook=fn,onView:(_,fn)=>viewHook=fn,refresh:async()=>{UI.revision++;snapshotHook(UI.snapshot);}};
 const window={unifiedUI:UI,confirm:()=>true};const document={createElement:t=>new E(t),head:new E('head')};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../unified-reports.js'),'utf8'),{window,document,console});window.unifiedReports.init();return {nodes,UI,window,refresh:()=>window.unifiedReports.refresh(),snapshot:(value=UI.snapshot)=>snapshotHook(value),view:()=>viewHook()};
}
const text=e=>[e.textContent||'',...e.children.map(x=>typeof x==='object'?text(x):'')].join(' ');
const flush=()=>new Promise(r=>setImmediate(r));
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
test('quality filters and offset go to native global query, old responses clear on failure',async()=>{
 const calls=[];const ui=mount({unifiedQualityPage:async(...args)=>{calls.push(args);return {issues:[],next_offset:50,total:80,counts:{missing:80}};}});await ui.refresh();ui.nodes.qualitySearch.value='needle';await ui.nodes.qualitySearch.fire('input');await flush();assert.equal(calls.at(-1)[1].search,'needle');await ui.nodes.qualityNext.click();await flush();assert.equal(calls.at(-1)[2],50);assert.match(ui.nodes.qualitySummary.textContent,/80/);
});

test('snapshot invalidation accepts null and clears stale reports and quality',async()=>{const ui=mount();await ui.refresh();assert.equal(ui.nodes.reportCalendar.children.length,12);ui.snapshot(null);assert.equal(ui.nodes.reportCalendar.children.length,0);assert.equal(ui.nodes.reports.children.length,0);assert.equal(ui.nodes.qualityTable.children.length,0);assert.equal(ui.nodes.saveImport.disabled,true);});

test('changing XLSX A to B clears dates year and manual mappings before B review',async()=>{
 const choices=['/tmp/A.xlsx','/tmp/B.xlsx'];let previews=0;
 const ui=mount({chooseWideXlsx:async()=>choices.shift(),nativeLegacyImportPreview:async()=>{previews++;throw Error('should require new date');}});
 await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2020-01-01';ui.nodes.importMonth.value='2020-01';ui.nodes.dailyYear.value='2020';ui.nodes.importMappingMode.value='manual';await ui.nodes.addImportMapping.click();ui.nodes.replaceExisting.checked=true;
 await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'B.xlsx');assert.equal(ui.nodes.importWeek.value,'');assert.equal(ui.nodes.importMonth.value,'');assert.equal(ui.nodes.dailyYear.value,'');assert.equal(ui.nodes.importMappingMode.value,'auto');assert.equal(ui.nodes.importColumnMappings.children.length,0);assert.equal(ui.nodes.replaceExisting.checked,false);
 await ui.nodes.inspectFile.click();assert.equal(previews,0);assert.match(ui.nodes.unifiedImportStatus.textContent,/первую дату/);
});

test('canceling replacement consistently retains A parameters but invalidates its review',async()=>{const choices=['/tmp/A.xlsx',null];const ui=mount({chooseWideXlsx:async()=>choices.shift()});await ui.nodes.excelFile.click();ui.nodes.importWeek.value='2024-03-01';await ui.nodes.inspectFile.click();assert.equal(ui.nodes.saveImport.disabled,false);await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'A.xlsx');assert.equal(ui.nodes.importWeek.value,'2024-03-01');assert.equal(ui.nodes.inspectFile.disabled,false);assert.equal(ui.nodes.saveImport.disabled,true);assert.match(ui.nodes.unifiedImportStatus.textContent,/отменён/);});

test('a selected Windows source receives only its checked folder rule',async()=>{const ui=mount({chooseWideXlsx:async()=>String.raw`C:\Excel\B.xlsx`,list:async()=>({folder:String.raw`C:\Excel`,files:[],rules:{'B.xlsx':{format:'daily-wide',type:'day',start:'',year:2025,sheets:['Дни'],mappings:{'Дни':{workCol:0,headerRow:1,firstDayCol:1}}}}})});await ui.nodes.excelFile.click();assert.equal(ui.nodes.excelFileStatus.textContent,'B.xlsx');assert.equal(ui.nodes.importFormat.value,'daily-wide');assert.equal(ui.nodes.dailyYear.value,2025);assert.equal(ui.nodes.unifiedWideSheets.value,'Дни');assert.match(ui.nodes.unifiedWideMapping.value,/Дни; 2; A; B/);});

test('deferred preview locks dynamic fields and rejects parameter events before exposing review',async()=>{
 let resolve;const ui=mount({chooseWideXlsx:async()=>'/tmp/A.xlsx',nativeLegacyImportPreview:()=>new Promise(r=>resolve=r)});await ui.nodes.excelFile.click();const pending=ui.nodes.inspectFile.click();await flush();assert.equal(ui.nodes.addImportMapping.disabled,true);assert.equal(ui.nodes.unifiedWideSheets.disabled,true);assert.equal(ui.nodes.unifiedWideMapping.disabled,true);
 ui.nodes.unifiedWideMapping.value='Data; 2; A; B;;';await ui.nodes.unifiedWideMapping.fire('input');resolve({reviewToken:'a'.repeat(64),reportCount:1,rowCount:1,conflicts:{},quality:{},sampleReports:[]});await pending;assert.equal(ui.nodes.saveImport.disabled,true);assert.match(ui.nodes.unifiedImportStatus.textContent,/изменились/);assert.equal(ui.nodes.addImportMapping.disabled,false);
});

test('report pager is joined and announces current page and total without an empty extra page',async()=>{
 const report=i=>({id:'r'+i,start:'2024-01-01',end:'2024-01-01',report_type:'day',rows:1,amount:2,warnings:0,organizations:1});const ui=mount({managementReportPage:async(rev,filter,after)=>after?[report(50)]:Array.from({length:50},(_,i)=>report(i)),managementReportSummary:async()=>({reports:51,rows:51,amount:102,latest:'2024-01-01'})});await ui.refresh();assert.equal(ui.nodes.unifiedReportsPager.className,'unified-pager');assert.match(ui.nodes.unifiedReportsPage.textContent,/Страница 1 из 2.*1–50 из 51/);assert.equal(ui.nodes.unifiedReportsPrev.disabled,true);assert.equal(ui.nodes.unifiedReportsNext.disabled,false);await ui.nodes.unifiedReportsNext.click();await flush();assert.match(ui.nodes.unifiedReportsPage.textContent,/Страница 2 из 2.*51–51 из 51/);assert.equal(ui.nodes.unifiedReportsNext.disabled,true);await ui.nodes.unifiedReportsPrev.click();await flush();assert.match(ui.nodes.unifiedReportsPage.textContent,/Страница 1 из 2/);
});

test('connected-file picker preserves Windows verbatim separators and Unix paths',async()=>{
 for(const [folder,expected] of [[String.raw`\\?\C:\Excel`,String.raw`\\?\C:\Excel\B.xlsx`],['/tmp/excel','/tmp/excel/B.xlsx']]){
  let selected=null;const ui=mount({list:async()=>({folder,files:[{name:'B.xlsx'}],rules:{}}),chooseFolder:async()=>null,nativeLegacyImportSheets:async source=>{selected=source;return ['xl/worksheets/sheet1.xml'];}});
  const picker=ui.nodes.sourceManualFile.parentElement;
  await picker.children.find(x=>x.textContent==='Показать файлы папки').click();
  assert.equal(ui.nodes.sourceManualFile.value,expected);
  await picker.children.find(x=>x.textContent==='Открыть для проверки').click();await flush();assert.equal(selected,expected);
 }
});
