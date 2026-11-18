'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');

test('SQL workspace is independently built without legacy app or full-history load',()=>{
 const root=path.resolve(__dirname,'../..'),out=fs.mkdtempSync(path.join(os.tmpdir(),'sql-workspace-'));
 try{
   prepare({base:root,output:out});
   const html=fs.readFileSync(path.join(out,'native-workspace.html'),'utf8');
   const js=fs.readFileSync(path.join(out,'tauri-native-workspace.js'),'utf8');
   assert.match(html,/id="legacyWorkspaceLink" href="index.html"/);
   assert.match(html,/tauri-native-workspace.js/);
   assert.ok(!html.includes('tauri-folder-importer.js'));
   assert.ok(!html.includes('window.appReady=main()'));
   assert.ok(!js.includes('desktopAPI.load('));
   assert.ok(!js.includes('desktopAPI.save('));
   assert.ok(!js.includes("load_data"));
   assert.match(js,/nativeSettingsBootstrap/);
   assert.match(js,/nativeGroupRename/);
   assert.match(js,/nativeReportPage/);
 }finally{fs.rmSync(out,{recursive:true,force:true});}
});
class Element{
 constructor(tag){this.tag=tag;this.children=[];this.events={};this.disabled=false;this.value='';this.textContent='';this.style={};}
 append(...els){this.children.push(...els);}
 replaceChildren(...els){this.children=[...els];}
 addEventListener(name,callback){this.events[name]=callback;}
 setAttribute(name,value){this[name]=value;}
}
function mount(){
 const ids=['workspaceRefresh','nativeReportType','reportBack','reportNext','groupBack','groupNext','workspaceStatus','reportStatus','groupStatus','reportRows','groupRows','metricReports','metricIndexed','metricGroups','metricCatalog','analysisType','analysisFrom','analysisTo','analysisOrganization','analysisPrepare','analysisCancel','analysisRun','analysisStatus','analysisValues','divisionStatus','divisionRows','workRows','workStatus','workBack','workNext','analysisExport','exportStatus','backupSqlite','backupStatus','restoreChoose','restoreCopy','restoreSelected','restoreStatus'];
 const e=Object.fromEntries(ids.map(id=>[id,new Element('div')]));
 e.analysisType.value='day';
 e.analysisOrganization.value='*';
 const calls=[];
 const report=n=>({id:'r'+n,period_start:'2024-02-'+String(n%28+1).padStart(2,'0'),period_end:'2024-02-29',report_type:'day',source_name:'<b>untrusted</b>'});
 const api={
   nativeSettingsBootstrap:async()=>({revision:12,reports:366,indexed_reports:366,groups:1,catalog:0,org_names:['СМУ16','АВТО']}),
   nativeReportPage:async(rev,filter,after,limit)=>{calls.push({kind:'report',rev,filter,after,limit});return !after?Array.from({length:100},(_,n)=>report(n)):[report(101)];},
   nativeGroupPage:async(rev,after,limit)=>{calls.push({kind:'group',rev,after,limit});return [{handle:'922337203685477580',id:'g',name:'Old',members:4}];},
   nativeGroupRename:async(rev,handle,name)=>{calls.push({kind:'rename',rev,handle,name});return 13;},
   nativeAnalysisIndexPage:async(rev,kind,after,limit)=>{calls.push({kind:'index',rev,reportType:kind,after,limit});return {processed:2,next:null,done:true};},
   nativeAnalysisIndexSettings:async rev=>{calls.push({kind:'settings_index',rev});},
   chooseSqliteBackupPath:async defaultPath=>{calls.push({kind:'choose_backup',defaultPath});return 'C:/Backups/new.sqlite3';},
   nativeBackupSqlite:async destination=>{calls.push({kind:'sqlite_backup',destination});return 12;},
   chooseRestoreBackupPath:async()=>{calls.push({kind:'restore_picker'});return 'C:/Backups/snapshot.sqlite3';},
   nativeRestorePreview:async source=>{calls.push({kind:'restore_preview',source});return {reportCount:366,bytes:8192,verification:'a'.repeat(64)};},
   chooseRestoreDestination:async name=>{calls.push({kind:'restore_dest',name});return 'C:/Backups/recovered.sqlite3';},
   nativeRestoreCopy:async(source,verification,destination)=>{
     calls.push({kind:'restore_copy',source,verification,destination});
     return {reportCount:366,bytes:8192,verification:'a'.repeat(64)};
   },
   chooseCsvPath:async defaultPath=>{calls.push({kind:'save_csv',defaultPath});return 'C:/Exports/new.csv';},
   nativeExportRangeCsv:async(rev,kind,from,to,organizations,organization,destination)=>{
     calls.push({kind:'export_csv',rev,reportType:kind,from,to,organizations,organization,destination});
   },
   nativeAnalysisRange:async(rev,kind,from,to,organizations)=>{calls.push({kind:'range',rev,reportType:kind,from,to,organizations});return {complete:false,periods:366,missing:1,total:{count:null,amount:null,average:null},invalid_reports:1,duplicate_issues:0,overlap_issues:0};},
   nativeAnalysisRangePage:async(rev,kind,from,to,organizations,organization,after,limit)=>{
     calls.push({kind:'work',rev,reportType:kind,from,to,organizations,organization,after,limit});
     const row=n=>({key:'work:'+String(n).padStart(4,'0'),name:n===0?'<b>Работа</b>':'Работа '+n,kind:'work',count:null,amount:null,average:null,active:true});
     return after?[row(50)]:Array.from({length:50},(_,i)=>row(i));
   },
   nativeAnalysisOrganizations:async(rev,kind,from,to,organizations)=>{calls.push({kind:'divisions',rev,reportType:kind,from,to,organizations});return [
     {org:'СМУ16',complete:true,missing:0,total:{count:8,amount:1200,average:150}},
     {org:'АВТО',complete:false,missing:2,total:{count:null,amount:null,average:null}}
   ];}
 };
 const window={desktopAPI:api,confirm:()=>true};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-workspace.js'),'utf8'),{window,document:{getElementById:id=>e[id],createElement:tag=>new Element(tag)}});
 return {e,calls,window};
}
test('native workspace displays only one keyset page and no HTML from source names',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 assert.equal(e.metricReports.textContent,366);
 assert.equal(e.metricGroups.textContent,1);
 assert.equal(e.reportRows.children.length,100);
 assert.equal(e.reportRows.children[0].children[3].textContent,'<b>untrusted</b>');
 assert.equal(e.groupRows.children.length,1);
 assert.equal(calls[0].kind,'report');
 assert.equal(calls[0].limit,100);
 assert.equal(calls[1].kind,'group');
 assert.equal(calls[1].limit,50);
 await e.reportNext.events.click();
 assert.equal(e.reportRows.children.length,1);
 assert.equal(calls[2].after.id,'r99');
 await e.reportBack.events.click();
 assert.equal(e.reportRows.children.length,100);
});
test('renaming a group needs explicit action, uses a string handle, and refreshes revision',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 const tr=e.groupRows.children[0];
 const input=tr.children[1].children[0],button=tr.children[3].children[0];
 assert.equal(button.disabled,true);
 input.value='New';input.events.input();
 assert.equal(button.disabled,false);
 await button.events.click();
 const action=calls.find(v=>v.kind==='rename');
 assert.equal(action.rev,12);
 assert.equal(action.handle,'922337203685477580');
 assert.equal(action.name,'New');
 assert.equal(e.reportRows.children.length,100);
});

test('D2 analysis indexes in bounded batches, cleans ranges, and never fabricates incomplete totals',async()=>{
 const {e,calls,window}=mount();await window.nativeWorkspaceReady;
 assert.equal(e.analysisRun.disabled,true);
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 await e.analysisPrepare.events.click();
 assert.equal(e.analysisRun.disabled,false);
 const batch=calls.find(c=>c.kind==='index');
 assert.equal(batch.limit,20);
 assert.equal(batch.rev,12);
 assert.equal(batch.reportType,'day');
 assert.equal(calls.filter(c=>c.kind==='settings_index').length,1);
 await e.analysisRun.events.click();
 const q=calls.find(c=>c.kind==='range');
 assert.equal(q.from,'2024-01-01');
 assert.equal(q.to,'2024-12-31');
 assert.equal(q.organizations.length,2);
 assert.equal(q.organizations[0],'СМУ16');
 assert.match(e.analysisStatus.textContent,/не все периоды/);
 const metrics=e.analysisValues.children;
 assert.equal(metrics.length,9);
 assert.equal(metrics[3].children[0].textContent,'—');
 assert.equal(metrics[4].children[0].textContent,'—');
 e.analysisType.value='week';e.analysisType.events.change();
 assert.equal(e.analysisRun.disabled,true);
});

test('D2 per-division comparison keeps partial periods unknown and plots complete magnitude',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 const divisions=calls.find(x=>x.kind==='divisions');
 assert.equal(divisions.rev,12);
 assert.equal(divisions.reportType,'day');
 assert.equal(divisions.organizations.length,2);
 assert.equal(e.divisionRows.children.length,2);
 const complete=e.divisionRows.children[0].children;
 assert.equal(complete[0].textContent,'СМУ16');
 assert.equal(complete[4].textContent,'1 200');
 assert.equal(complete[5].children[0].children[0].style.width,'100%');
 const partial=e.divisionRows.children[1].children;
 assert.equal(partial[0].textContent,'АВТО');
 assert.equal(partial[2].textContent,'2');
 assert.equal(partial[3].textContent,'—');
 assert.equal(partial[4].textContent,'—');
 assert.equal(partial[5].children[0].textContent,'Нет полного ряда');
 // Changing the selection must not leave a misleading chart visible.
 e.analysisOrganization.value='СМУ16';
 e.analysisOrganization.events.change();
 assert.equal(e.divisionRows.children.length,0);
});

test('bounded work/group SQL pages preserve unknown totals, keyset cursors and selected scope',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 const first=calls.find(c=>c.kind==='work');
 assert.equal(first.rev,12);
 assert.equal(first.reportType,'day');
 assert.equal(first.limit,50);
 assert.equal(first.after,null);
 assert.equal(first.organization,null);
 assert.equal(first.organizations.length,2);
 assert.equal(e.workRows.children.length,50);
 assert.equal(e.workRows.children[0].children[0].textContent,'<b>Работа</b>');
 assert.equal(e.workRows.children[0].children[2].textContent,'—');
 assert.equal(e.workRows.children[0].children[3].textContent,'—');
 assert.equal(e.workNext.disabled,false);
 await e.workNext.events.click();
 const requests=calls.filter(c=>c.kind==='work');
 assert.equal(requests[1].after,'work:0049');
 assert.equal(e.workRows.children.length,1);
 assert.equal(e.workNext.disabled,true);
 await e.workBack.events.click();
 assert.equal(calls.filter(c=>c.kind==='work').at(-1).after,null);
 assert.equal(e.workRows.children.length,50);
 e.analysisOrganization.value='СМУ16';
 e.analysisOrganization.events.change();
 assert.equal(e.workRows.children.length,0,'changing scope clears old data');
 assert.equal(e.workNext.disabled,true);
 await e.analysisRun.events.click();
 assert.equal(calls.filter(c=>c.kind==='work').at(-1).organization,'СМУ16');
});

test('native CSV export waits for explicit Save selection and preserves selected revision',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 assert.equal(e.analysisExport.disabled,true);
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 await e.analysisPrepare.events.click();
 assert.equal(e.analysisExport.disabled,true,'indexing does not authorize exporting a stale scope');
 await e.analysisRun.events.click();
 assert.equal(e.analysisExport.disabled,false);
 await e.analysisExport.events.click();
 const save=calls.find(c=>c.kind==='save_csv');
 const exportCall=calls.find(c=>c.kind==='export_csv');
 assert.match(save.defaultPath,/\.csv$/);
 assert.equal(exportCall.rev,12);
 assert.equal(exportCall.from,'2024-01-01');
 assert.equal(exportCall.to,'2024-12-31');
 assert.equal(JSON.stringify(exportCall.organizations),JSON.stringify(['СМУ16','АВТО']));
 assert.equal(exportCall.organization,null);
 assert.equal(exportCall.destination,'C:/Exports/new.csv');
 assert.match(e.exportStatus.textContent,/CSV сохранён/);
 e.analysisOrganization.value='СМУ16';
 e.analysisOrganization.events.change();
 assert.equal(e.analysisExport.disabled,true,'scope changes invalidate previous export');
});

test('native SQLite backup is a user-initiated copy, not a report rewrite',async()=>{
 const {e,calls,window}=mount();await window.nativeWorkspaceReady;
 await e.backupSqlite.events.click();
 assert.match(calls.find(x=>x.kind==='choose_backup').defaultPath,/\.sqlite3$/);
 assert.equal(calls.find(x=>x.kind==='sqlite_backup').destination,'C:/Backups/new.sqlite3');
 assert.match(e.backupStatus.textContent,/Копия SQLite сохранена/);
 assert.equal(calls.some(x=>x.kind==='rename'),false);
 assert.equal(e.reportRows.children.length,100);
});

test('recovery UI validates a chosen backup before offering a separate copy',async()=>{
 const {e,calls,window}=mount();await window.nativeWorkspaceReady;
 assert.equal(e.restoreCopy.disabled,true);
 await e.restoreChoose.events.click();
 assert.equal(calls.some(x=>x.kind==='restore_preview'),true);
 assert.equal(e.restoreCopy.disabled,false);
 assert.match(e.restoreStatus.textContent,/Копия проверена/);
 assert.match(e.restoreSelected.textContent,/snapshot\.sqlite3/);
 assert.equal(calls.some(x=>x.kind==='restore_copy'),false);
 await e.restoreCopy.events.click();
 const restore=calls.find(x=>x.kind==='restore_copy');
 assert.equal(restore.source,'C:/Backups/snapshot.sqlite3');
 assert.equal(restore.verification,'a'.repeat(64));
 assert.equal(restore.destination,'C:/Backups/recovered.sqlite3');
 assert.match(e.restoreStatus.textContent,/Рабочая база не изменена/);
 assert.equal(calls.some(x=>x.kind==='sqlite_backup'),false);
 assert.equal(calls.some(x=>x.kind==='rename'),false);
});
