'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
function mount(handle=async()=>({ok:true})){const calls=[],events=[];const window={__TAURI__:{core:{invoke:async(command,args)=>{calls.push({command,args});return handle(command,args);}},dialog:{open:async()=>null,save:async()=>null}},dispatchEvent:event=>events.push(event)};class CustomEvent{constructor(type,init){this.type=type;this.detail=init.detail;}}vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,CustomEvent});return{api:window.desktopAPI,calls,events};}
test('worksheet metadata discovery is a tracked read with the original source path',async()=>{
 const m=mount();await m.api.nativeImportSheets('C:/Excel/quarters.xlsx');
 assert.deepEqual(JSON.parse(JSON.stringify(m.calls[0])),{command:'native_import_sheets',args:{source:'C:/Excel/quarters.xlsx'}});
 assert.equal(m.events.at(-1).detail.reads,0);
});
test('unified reads retain bounded cursors and complete server filters',async()=>{const m=mount(),request={revision:7,source:'daily'},options={search:'строка',direction:'down',sort:'amountDesc'};await m.api.unifiedChangesPage(request,options,'opaque');await m.api.unifiedNameOptions(7,'Ёж','next',50);await m.api.managementReportPage(7,{organization:'А'},null,50);assert.deepEqual(JSON.parse(JSON.stringify(m.calls)),[{command:'unified_changes_page',args:{request,options,after:'opaque'}},{command:'unified_name_options',args:{revision:7,search:'Ёж',after:'next',limit:50}},{command:'unified_report_page',args:{revision:7,filter:{organization:'А'},after:null,limit:50}}]);assert.equal(m.events.at(-1).detail.reads,0);});
test('direct pages carry absolute indices and exact server filters without invented cursors',async()=>{
 const m=mount(),request={revision:7,source:'daily'},options={search:'Работа'},before={from:'2024-01-01',to:'2024-01-31'};
 await m.api.unifiedNameOptionsAtPage(7,'Ёж',6,10);
 await m.api.unifiedChangesPageAtPage(request,options,3);
 await m.api.unifiedPatternPageAtPage(request,'regularity',4,options,2);
 await m.api.nativeCompositionPageAtPage(request,before,'contributions','amount',4);
 await m.api.unifiedComparisonWindow(request,before,'А',6,10);
 await m.api.managementReportWindow(7,{organization:'А'},4,50);
 await m.api.managementReportOrgWindow(7,'r1',2,50);
 await m.api.nativeQualityMatrixReportsAtPage(7,'day','2024-01-01','А',3,20);
 assert.deepEqual(m.calls.map(c=>c.args.pageIndex),[6,3,2,4,6,4,2,3]);
 assert.ok(m.calls.slice(0,4).every(c=>c.args.after===null));
 assert.equal(m.calls[4].command,'unified_comparison_window');assert.equal(m.calls[5].command,'unified_report_window');assert.equal(m.calls[6].command,'unified_report_org_window');assert.equal(m.calls[7].command,'native_quality_matrix_reports_at_page');
 assert.equal(m.calls[0].args.limit,10);assert.equal(m.calls[4].args.limit,10);assert.equal(m.calls[3].args.structureLimit,null);assert.equal(m.events.at(-1).detail.saves,0);
});
test('atomic changes forward revision and review hash; deletion needs no backup destination',async()=>{const m=mount();await m.api.unifiedGroupsImport(7,'/groups.json','hash',true);await m.api.unifiedClearReports(7);await m.api.nativeDeleteReport(7,'one-report');assert.deepEqual(JSON.parse(JSON.stringify(m.calls)),[{command:'unified_groups_import',args:{revision:7,source:'/groups.json',hash:'hash',confirmed:true}},{command:'unified_clear_reports',args:{revision:7}},{command:'native_delete_report',args:{revision:7,id:'one-report'}}]);assert.equal(m.events.at(-1).detail.saves,0);});
test('comparison, regularity and structure preserve added query options',async()=>{const m=mount(),request={revision:7},before={from:'2026-01-01',to:'2026-01-02'},options={search:'работа',status:'new'};await m.api.unifiedComparisonPage(request,before,'А','cursor',50);await m.api.unifiedPatternPage(request,'regularity',6,options,null);await m.api.nativeCompositionPage(request,before,'structure','count',null,20);assert.equal(m.calls[0].args.organization,'А');assert.equal(m.calls[1].args.windowSize,6);assert.equal(m.calls[1].args.options,options);assert.equal(m.calls[2].args.structureLimit,20);});
test('quality evidence forwards bounded cells and opaque report cursors as reads',async()=>{const m=mount();await m.api.nativeQualityMatrixPage(7,'week','2024-12-30','2025-01-06',['А']);await m.api.nativeQualityMatrixReports(7,'week','2024-12-30','А','opaque',20);await m.api.nativeQualityMatrixReports(7,'day','2024-12-30',null);assert.deepEqual(JSON.parse(JSON.stringify(m.calls)),[{command:'native_quality_matrix_page',args:{revision:7,reportType:'week',from:'2024-12-30',to:'2025-01-06',organizations:['А']}},{command:'native_quality_matrix_reports',args:{revision:7,reportType:'week',start:'2024-12-30',organization:'А',after:'opaque',limit:20}},{command:'native_quality_matrix_reports',args:{revision:7,reportType:'day',start:'2024-12-30',organization:null,after:null,limit:20}}]);assert.equal(m.events.at(-1).detail.reads,0);assert.equal(m.events.at(-1).detail.saves,0);});

// The UI no longer offers these actions; isolated native recovery checks still
// use the same revision-independent source verification token and IPC routes.
test('recovery IPC remains available without exposing desktop restoration controls',async()=>{
 const m=mount(),hash='a'.repeat(64);
 await m.api.nativeRestoreCopy('/snapshot.sqlite3',hash,'/copy.sqlite3');
 await m.api.nativeRestoreSchedule('/snapshot.sqlite3',hash);
 await m.api.nativeRestoreCancel();
 assert.deepEqual(JSON.parse(JSON.stringify(m.calls)),[
  {command:'native_restore_copy',args:{source:'/snapshot.sqlite3',verification:hash,destination:'/copy.sqlite3'}},
  {command:'native_restore_schedule',args:{source:'/snapshot.sqlite3',verification:hash}},
  {command:'native_restore_cancel',args:{}}
 ]);
 assert.equal(m.events.at(-1).detail.saves,0);
});

test('worksheet auto-rule metadata forwards only the selected native rule',async()=>{
 const m=mount(),rule={format:'auto',type:'week',start:'2024-02-05',sheet:'Second'};
 await m.api.nativeImportSheets('C:/Excel/two.xlsx',rule);
 assert.deepEqual(JSON.parse(JSON.stringify(m.calls[0])),{command:'native_import_sheets',args:{source:'C:/Excel/two.xlsx',rule}});
});

test('all-work series paging distinguishes keyed rows from selectable names',async()=>{
 const m=mount();
 await m.api.unifiedNameOptionsAtPage(7,'',1,8,true);
 await m.api.unifiedNameOptionsAtPage(7,'',1,8);
 assert.equal(m.calls[0].args.allSeries,true);
 assert.equal(m.calls[1].args.allSeries,undefined);
});

test('organization work pages carry the same scope, revision and absolute page as the detailed table',async()=>{
 const m=mount(),request={revision:7,source:'daily',displayType:'day',organizations:['B','A'],selectedNames:[],query:{from:'2025-03-01',to:'2025-03-02'}};
 await m.api.unifiedOrganizationWorkWindow(request,1,50);
 assert.deepEqual(JSON.parse(JSON.stringify(m.calls[0])),{command:'unified_organization_work_window',args:{request,pageIndex:1,limit:50}});
 assert.equal(m.events.at(-1).detail.reads,0);
});

test('critical IPC forwards reviewed requests, optional fields and native errors', async () => {
 const nativeError=Error('Stale SQLite revision');
 const m=mount(async command=>{if(command==='unified_delete_reports')throw nativeError;return {ok:true};});
 const importRequest={revision:7,session:'session-token',folder:'C:/Excel',name:'day.xlsx',signature:'file-signature'};
 const deletion={kind:'period',from:'2024-01-01',to:'2024-01-02'};
 const summaryRequest={revision:7,reportType:'day',organizations:['A'],from:'2024-01-02',to:'2024-01-02',
  baseFrom:'2024-01-01',baseTo:'2024-01-01',metric:'amount',percent:0,minCount:0,minAmount:0,kind:'all',search:'',sort:'name',page:0};
 const request={revision:7,source:'daily',displayType:'day',organizations:['A'],selectedNames:[],
  query:{method:'range',from:'2024-01-02',to:'2024-01-02',percent:0,min_count:0,min_amount:0}};
 const detail={request,section:'organizations',metric:'amount',before:null,baseOrg:'A',limit:null};
 await m.api.nativeFolderImportPreview(importRequest);
 await m.api.nativeFolderImportApply(importRequest,'review-token');
 await m.api.unifiedReportDeletionCount(7,deletion);
 await assert.rejects(m.api.unifiedDeleteReports(7,deletion,2),error=>error===nativeError);
 await m.api.referenceSummaryCsv(summaryRequest,'C:/export/summary.csv');
 await m.api.referenceDetailCsv(detail,'C:/export/detail.csv');
 await m.api.nativeRestoreSchedule('C:/backups/profile.sqlite3','verified-hash',7);
 assert.deepEqual(JSON.parse(JSON.stringify(m.calls)),[
  {command:'native_folder_import_preview',args:{request:importRequest}},
  {command:'native_folder_import_apply',args:{request:importRequest,reviewToken:'review-token'}},
  {command:'unified_report_deletion_count',args:{revision:7,scope:deletion}},
  {command:'unified_delete_reports',args:{revision:7,scope:deletion,confirmedCount:2}},
  {command:'reference_summary_csv',args:{request:summaryRequest,destination:'C:/export/summary.csv'}},
  {command:'reference_detail_csv',args:{payload:detail,destination:'C:/export/detail.csv'}},
  {command:'native_restore_schedule',args:{source:'C:/backups/profile.sqlite3',verification:'verified-hash',revision:7}}
 ]);
 assert.deepEqual(m.events.map(event=>[event.detail.reads,event.detail.saves]),[
  [1,0],[0,0],[0,1],[0,0],[1,0],[0,0],[0,1],[0,0],[0,1],[0,0],[0,1],[0,0],[0,1],[0,0]
 ]);
});
