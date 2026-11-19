'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const {userError}=require('../native-errors.js');

test('SQL workspace is independently built without legacy app or full-history load',()=>{
 const root=path.resolve(__dirname,'../..'),out=fs.mkdtempSync(path.join(os.tmpdir(),'sql-workspace-'));
 try{
   prepare({base:root,output:out});
   const html=fs.readFileSync(path.join(out,'native-workspace.html'),'utf8');
   const js=fs.readFileSync(path.join(out,'tauri-native-workspace.js'),'utf8');
   assert.match(html,/id="legacyWorkspaceLink" href="index.html"/);
   assert.match(html,/tauri-native-workspace.js/);
   assert.match(html,/tauri-native-updates.js/);
   assert.match(html,/id="sqlUpdateCheck"/);
   assert.match(html,/id="sqlUpdateInstall"/);
   assert.match(html,/tauri-native-legacy-import.js/);
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
function mount(options={}){
 // Derive the mock DOM from the shipped page so missing controls cannot be
 // silently supplied by a hand-maintained test fixture.
 const html=fs.readFileSync(path.join(__dirname,'../native-workspace.html'),'utf8');
 const ids=[...html.matchAll(/\bid="([^"]+)"/g)].map(match=>match[1]);
 const e=Object.fromEntries(ids.map(id=>[id,new Element('div')]));
 e.analysisType.value='day';
 e.analysisOrganization.value='*';
 e.rankingMetric.value='amount';e.rankingCategory.value='all';e.workTrendMetric.value='amount';e.comparisonRankMetric.value='amount';
 const calls=[];
 const divisionAmounts=Array.isArray(options.divisionAmounts)?options.divisionAmounts:[1200,600];
 const report=n=>({id:'r'+n,period_start:'2024-02-'+String(n%28+1).padStart(2,'0'),period_end:'2024-02-29',report_type:'day',source_name:'<b>untrusted</b>'});
 const api={
   nativeSettingsBootstrap:async()=>({revision:12,reports:366,indexed_reports:366,groups:1,catalog:0,org_names:['СМУ16','АВТО']}),
   nativeReportPage:async(rev,filter,after,limit)=>{calls.push({kind:'report',rev,filter,after,limit});if(options.failReports)throw Error('No active organization');return !after?Array.from({length:100},(_,n)=>report(n)):[report(101)];},
   nativeGroupPage:async(rev,after,limit)=>{calls.push({kind:'group',rev,after,limit});return [{handle:'922337203685477580',id:'g',name:'Old',members:4}];},
   nativeGroupRename:async(rev,handle,name)=>{calls.push({kind:'rename',rev,handle,name});return 13;},
   nativeAnalysisIndexPage:async(rev,kind,after,limit)=>{calls.push({kind:'index',rev,reportType:kind,after,limit});return {processed:2,next:null,done:true};},
   nativeAnalysisIndexSettings:async rev=>{calls.push({kind:'settings_index',rev});},
   nativeAnalysisNameOptions:async(rev,search,limit)=>{
     calls.push({kind:'names',rev,search,limit});
     return {has_more:false,rows:[
       {name:'Работа',kind:'work'},{name:'Группа',kind:'group'}
     ]};
   },
   chooseSqliteBackupPath:async defaultPath=>{calls.push({kind:'choose_backup',defaultPath});return 'C:/Backups/new.sqlite3';},
   nativeBackupSqlite:async destination=>{calls.push({kind:'sqlite_backup',destination});return 12;},
   chooseRestoreBackupPath:async()=>{calls.push({kind:'restore_picker'});return 'C:/Backups/snapshot.sqlite3';},
   nativeRestorePreview:async source=>{calls.push({kind:'restore_preview',source});return {reportCount:366,bytes:8192,verification:'a'.repeat(64)};},
   chooseRestoreDestination:async name=>{calls.push({kind:'restore_dest',name});return 'C:/Backups/recovered.sqlite3';},
   nativeRestoreCopy:async(source,verification,destination)=>{
     calls.push({kind:'restore_copy',source,verification,destination});
     return {reportCount:366,bytes:8192,verification:'a'.repeat(64)};
   },
   nativeRestoreSchedule:async(source,verification)=>{
     calls.push({kind:'restore_schedule',source,verification});
     return {reportCount:366,restartRequired:true};
   },
   nativeRestoreLaunchInfo:async()=>null,
   nativeRestoreCancel:async()=>{calls.push({kind:'restore_cancel'});return true;},
   chooseCsvPath:async defaultPath=>{calls.push({kind:'save_csv',defaultPath});return 'C:/Exports/new.csv';},
   nativeExportRangeCsv:async(rev,kind,from,to,organizations,organization,destination,selectedName,selectedNames)=>{
     calls.push({kind:'export_csv',rev,reportType:kind,from,to,organizations,organization,destination,selectedName,selectedNames});
   },
   nativeAnalysisRange:async(rev,kind,from,to,organizations,selectedName,selectedNames)=>{calls.push({kind:'range',rev,reportType:kind,from,to,organizations,selectedName,selectedNames});return options.complete
    ?{complete:true,periods:12,missing:0,total:{count:12,amount:options.mismatchedTotal?1900:divisionAmounts[0]+divisionAmounts[1],average:150},invalid_reports:0,duplicate_issues:0,overlap_issues:0}
    :{complete:false,periods:366,missing:1,total:{count:null,amount:null,average:null},
      available_reports:options.observed?8:0,
      available_total:options.observed?{count:7,amount:350,average:50}:{count:null,amount:null,average:null},
      invalid_reports:1,duplicate_issues:0,overlap_issues:0,import_warning_reports:0};},
   nativeAnalysisTimeline:async(rev,kind,from,to,organizations,selectedName,grouping,selectedNames)=>{
     calls.push({kind:'timeline',rev,reportType:kind,from,to,organizations,selectedName,selectedNames});
     return {granularity:'period',points:[{start:from,periods:1,missing:options.complete?0:1,
       total:options.complete?{count:2,amount:100,average:50}:{count:null,amount:null,average:null}}]};
   },
   nativeOrganizationTrends:async(rev,kind,from,to,organizations,selectedName,grouping,selectedNames)=>{
     calls.push({kind:'org_trends',rev,reportType:kind,from,to,organizations,selectedName,selectedNames});
     return {granularity:'period',selected_organizations:organizations.length,series:[
       {organization:organizations[0],points:[{start:from,periods:1,missing:options.complete?0:1,
         total:options.complete?{count:2,amount:100,average:50}:{count:null,amount:null,average:null}}]}
     ]};
   },
   nativeAnalysisRanking:async(rev,kind,from,to,organizations,organization,metric,category,selectedName,selectedNames)=>{
     calls.push({kind:'ranking',rev,reportType:kind,from,to,organizations,organization,metric,category,selectedName,selectedNames});
     return options.complete
       ?{complete:true,rows:[{key:'work:x',name:'Работа',kind:'work',count:-2,amount:300,active:true}]}
       :{complete:false,rows:[]};
   },
   nativeAnalysisCompare:async(rev,kind,beforeFrom,beforeTo,from,to,organizations,selectedName,selectedNames)=>{
     calls.push({kind:'comparison',rev,reportType:kind,beforeFrom,beforeTo,from,to,organizations,selectedName,selectedNames});
     const measure=(base,current)=>({base,current,delta:current-base,percent:base===0?null:(current-base)/Math.abs(base)*100});
     const comparison={
       summary:{before:{complete:true},after:{complete:!!options.complete},
         count:options.complete?measure(0,2):null,
         amount:options.complete?measure(10,20):null},
       divisions:[{organization:organizations[0],before:{count:2,amount:10},
         after:{count:options.complete?4:null,amount:options.complete?20:null},
         count:options.complete?measure(2,4):null,amount:options.complete?measure(10,20):null}],
       top_amount:options.complete?[{key:'work:a',name:'A',kind:'work',amount:measure(10,20),count:measure(2,4)}]:[],
       top_count:options.complete?[{key:'work:a',name:'A',kind:'work',amount:measure(10,20),count:measure(2,4)}]:[]
     };
     if(options.delayedComparison)return new Promise(resolve=>{options.resolveComparison=()=>resolve(comparison);});
     return comparison;
   },
   nativeWorkTimeline:async(rev,kind,from,to,organizations,organization,key,selectedName,grouping,selectedNames)=>{
     calls.push({kind:'work_timeline',rev,reportType:kind,from,to,organizations,organization,key,selectedName,selectedNames});
     const data={key,name:'Работа '+key,kind:key.startsWith('group:')?'group':'work',
       granularity:'period',points:[{start:from,periods:1,missing:options.complete?0:1,
         total:options.complete?{count:-2,amount:20}:{count:null,amount:null}}]};
     if(options.delayedWorkTrend)return new Promise(resolve=>{options.resolveWorkTrend=()=>resolve(data);});
     return data;
   },
   nativeAnalysisRangePage:async(rev,kind,from,to,organizations,organization,after,limit,selectedName,selectedNames)=>{
     calls.push({kind:'work',rev,reportType:kind,from,to,organizations,organization,after,limit,selectedName,selectedNames});
     const row=n=>({key:'work:'+String(n).padStart(4,'0'),name:n===0?'<b>Работа</b>':'Работа '+n,kind:'work',count:null,amount:null,average:null,active:true});
     return after?[row(50)]:Array.from({length:50},(_,i)=>row(i));
   },
   nativeAnalysisOrganizations:async(rev,kind,from,to,organizations,selectedName,selectedNames)=>{calls.push({kind:'divisions',rev,reportType:kind,from,to,organizations,selectedName,selectedNames});return [
     {org:'СМУ16',complete:true,missing:0,total:{count:8,amount:divisionAmounts[0],average:divisionAmounts[0]/8}},
     options.complete
      ?{org:'АВТО',complete:true,missing:0,total:{count:4,amount:divisionAmounts[1],average:divisionAmounts[1]/4}}
      :{org:'АВТО',complete:false,missing:2,total:{count:null,amount:null,average:null}}
   ];}
 };
 const chartCalls=[],rankingCalls=[],orgTrendsCalls=[],workTrendCalls=[],comparisonCalls=[];
 const persistent=options.persistent||new Map();
 const window={desktopAPI:api,confirm:()=>true,analizUserError:userError,
   localStorage:{getItem:key=>persistent.get(key)||null,setItem:(key,value)=>persistent.set(key,value)},
   nativeTimeline:{
     show:data=>chartCalls.push({action:'show',data}),
     clear:message=>chartCalls.push({action:'clear',message}),
     error:message=>chartCalls.push({action:'error',message})
   },
   nativeRanking:{
     show:data=>rankingCalls.push({action:'show',data}),
     clear:message=>rankingCalls.push({action:'clear',message}),
     error:message=>rankingCalls.push({action:'error',message})
   },
   nativeOrganizationTrends:{
     show:data=>orgTrendsCalls.push({action:'show',data}),
     clear:message=>orgTrendsCalls.push({action:'clear',message}),
     error:message=>orgTrendsCalls.push({action:'error',message})
   },
   nativeWorkTrends:{
     show:data=>workTrendCalls.push({action:'show',data}),
     clear:message=>workTrendCalls.push({action:'clear',message}),
     error:message=>workTrendCalls.push({action:'error',message})
   },
   nativeComparison:{
     show:data=>comparisonCalls.push({action:'show',data}),
     clear:message=>comparisonCalls.push({action:'clear',message}),
     error:message=>comparisonCalls.push({action:'error',message})
   }};

 const document={getElementById:id=>e[id],createElement:tag=>new Element(tag)};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-period-selection.js'),'utf8'),{window,document});
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-name-filter.js'),'utf8'),{window,document,TextEncoder});
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-workspace.js'),'utf8'),{window,document});
 return {e,calls,window,chartCalls,rankingCalls,orgTrendsCalls,workTrendCalls,comparisonCalls};
}
test('native workspace displays only one keyset page and no HTML from source names',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 assert.equal(e.metricReports.textContent,366);
 assert.equal(e.metricGroups.textContent,1);
 assert.equal(e.reportRows.children.length,100);
 assert.equal(e.reportPageInfo.textContent,'Страница 1');
 assert.equal(e.reportRows.children[0].children[3].textContent,'<b>untrusted</b>');
 assert.equal(e.groupRows.children.length,1);
 assert.equal(e.groupPageInfo.textContent,'Страница 1');
 assert.equal(e.workPageInfo.textContent,'Страница 1');
 assert.equal(calls[0].kind,'report');
 assert.equal(calls[0].limit,100);
 assert.equal(calls[1].kind,'group');
 assert.equal(calls[1].limit,50);
 await e.reportNext.events.click();
 assert.equal(e.reportPageInfo.textContent,'Страница 2');
 assert.equal(e.reportRows.children.length,1);
 assert.equal(calls[2].after.id,'r99');
 await e.reportBack.events.click();
 assert.equal(e.reportPageInfo.textContent,'Страница 1');
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

test('SQL filter context shows scope, preserves selection and clears stale results',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 assert.match(e.analysisFilterScope.textContent,/все из настроек \(2\)/);
 assert.match(e.analysisFilterScope.textContent,/детализация и CSV/);
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 e.analysisOrganization.value='СМУ16';e.analysisOrganization.events.change();
 assert.match(e.analysisFilterScope.textContent,/СМУ16 \(1 из 2\)/);
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 assert.equal(calls.filter(x=>x.kind==='range').at(-1).organizations.length,1);
 assert.match(e.analysisResultScope.textContent,/СМУ16 \(1 из 2\)/);
 assert.match(e.analysisResultScope.textContent,/не только показанные 50 строк/);
 await window.nativeWorkspaceRefresh();
 assert.equal(e.analysisOrganization.value,'СМУ16','refresh must preserve a valid selected organization');
 assert.match(e.analysisFilterScope.textContent,/СМУ16/);
 assert.match(e.analysisResultScope.textContent,/повторите расчёт/i);
 e.analysisType.value='week';e.analysisType.events.change();
 assert.equal(e.analysisOrganization.value,'СМУ16','period type change keeps organization selection');
 assert.match(e.analysisResultScope.textContent,/Тип периода изменён/);
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

test('division shares use only complete matching SQL totals; incomplete periods show unknown shares',async()=>{
 const complete=mount({complete:true});
 await complete.window.nativeWorkspaceReady;
 complete.e.analysisFrom.value='2024-01-01';
 complete.e.analysisTo.value='2024-12-31';
 await complete.e.analysisPrepare.events.click();
 await complete.e.analysisRun.events.click();
 assert.match(complete.e.divisionShareStatus.textContent,/Доли от подтверждённой суммы/);
 assert.equal(complete.e.divisionRows.children[0].children[6].textContent,'66,7 %');
 assert.equal(complete.e.divisionRows.children[1].children[6].textContent,'33,3 %');
 const a=complete.e.divisionRows.children[0].children[6],b=complete.e.divisionRows.children[1].children[6];
 assert.equal(a.className,'division-share');
 assert.equal(a.children[0].className,'division-share-track');
 assert.equal(a.children[0].children[0].className,'division-share-fill');
 assert.equal(a.children[0]['aria-hidden'],'true');
 assert.ok(Math.abs(parseFloat(a.children[0].children[0].style.width)-66.6666666667)<0.01);
 assert.ok(Math.abs(parseFloat(b.children[0].children[0].style.width)-33.3333333333)<0.01);
 complete.e.analysisOrganization.value='СМУ16';
 complete.e.analysisOrganization.events.change();
 assert.equal(complete.e.divisionRows.children.length,0);
 assert.match(complete.e.divisionShareStatus.textContent,/сброшены/);
 const incomplete=mount();
 await incomplete.window.nativeWorkspaceReady;
 incomplete.e.analysisFrom.value='2024-01-01';
 incomplete.e.analysisTo.value='2024-12-31';
 await incomplete.e.analysisPrepare.events.click();
 await incomplete.e.analysisRun.events.click();
 assert.equal(incomplete.e.divisionRows.children[0].children[6].textContent,'—');
 assert.equal(incomplete.e.divisionRows.children[1].children[6].textContent,'—');
 assert.match(incomplete.e.divisionShareStatus.textContent,/Доли скрыты/);
 assert.equal(incomplete.e.divisionRows.children[0].children[6].children.length,0);
 assert.equal(incomplete.e.divisionRows.children[1].children[6].children.length,0);
 const mismatch=mount({complete:true,mismatchedTotal:true});
 await mismatch.window.nativeWorkspaceReady;
 mismatch.e.analysisFrom.value='2024-01-01';
 mismatch.e.analysisTo.value='2024-12-31';
 await mismatch.e.analysisPrepare.events.click();
 await mismatch.e.analysisRun.events.click();
 assert.equal(mismatch.e.divisionRows.children[0].children[6].textContent,'—');
 assert.equal(mismatch.e.divisionRows.children[1].children[6].textContent,'—');
 assert.match(mismatch.e.divisionShareStatus.textContent,/не совпадают/);
 assert.equal(mismatch.e.divisionRows.children[0].children[6].children.length,0);
});

test('zero and negative SQL totals never render misleading shares or mini-bars',async()=>{
 for(const amounts of [[0,0],[-120,-60]]){
  const {e,window}=mount({complete:true,divisionAmounts:amounts});
  await window.nativeWorkspaceReady;
  e.analysisFrom.value='2024-01-01';
  e.analysisTo.value='2024-12-31';
  await e.analysisPrepare.events.click();
  await e.analysisRun.events.click();
  assert.equal(e.divisionRows.children.length,2);
  for(const row of e.divisionRows.children){
   const share=row.children[6];
   assert.equal(share.textContent,'—');
   assert.equal(share.children.length,0);
  }
  assert.match(e.divisionShareStatus.textContent,/сумма не положительна/);
 }
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

test('SQL read failures show a Russian action rather than a Rust exception',async()=>{
 const {e,window}=mount({failReports:true});
 await window.nativeWorkspaceReady;
 assert.match(e.reportStatus.textContent,/Не выбрано подразделение/);
 assert.ok(!e.reportStatus.textContent.includes('No active organization'));
 assert.equal(e.reportRows.children.length,0);
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

test('requesting live SQLite restore requires explicit review, confirmation and restart',async()=>{
 const {e,calls,window}=mount();
 await window.nativeWorkspaceReady;
 assert.equal(e.restoreSchedule.disabled,true);
 await e.restoreChoose.events.click();
 assert.equal(e.restoreSchedule.disabled,false);
 assert.equal(calls.some(c=>c.kind==='restore_schedule'),false);
 await e.restoreSchedule.events.click();
 const request=calls.find(c=>c.kind==='restore_schedule');
 assert.ok(request,'a verified source can be scheduled after confirmation');
 assert.equal(request.source,'C:/Backups/snapshot.sqlite3');
 assert.equal(request.verification,'a'.repeat(64));
 assert.match(e.restoreStatus.textContent,/Полностью закройте приложение/);
 assert.equal(e.restoreSchedule.disabled,true);
 assert.equal(calls.some(c=>c.kind==='sqlite_backup'),false,'do not overwrite or back up live SQLite in the running WebView');
 await e.restoreCancel.events.click();
 assert.equal(calls.filter(c=>c.kind==='restore_cancel').length,1);
 assert.match(e.restoreStatus.textContent,/отменено/);
});

test('SQL timeline uses identical date and organization filters and clears stale graphics',async()=>{
 const {e,calls,window,chartCalls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisOrganization.value='АВТО';e.analysisOrganization.events.change();
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 const kpi=calls.find(x=>x.kind==='range');
 const timeline=calls.find(x=>x.kind==='timeline');
 assert.equal(timeline.reportType,kpi.reportType);
 assert.equal(timeline.from,kpi.from);
 assert.equal(timeline.to,kpi.to);
 assert.equal(timeline.rev,kpi.rev);
 assert.equal(timeline.organizations.join(','),kpi.organizations.join(','));
 assert.ok(chartCalls.some(x=>x.action==='show'));
 e.analysisTo.value='2024-03-31';e.analysisTo.events.change();
 assert.equal(chartCalls.at(-1).action,'clear');
 assert.match(chartCalls.at(-1).message,/Фильтры изменены/);
});

test('SQL work/group ranking uses exact KPI filter, reorders per metric and invalidates stale values',async()=>{
 const {e,calls,window,rankingCalls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisOrganization.value='СМУ16';
 e.analysisOrganization.events.change();
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 const q=calls.find(x=>x.kind==='range');
 const r=calls.find(x=>x.kind==='ranking');
 assert.equal(r.rev,q.rev);assert.equal(r.from,q.from);assert.equal(r.to,q.to);
 assert.equal(r.reportType,q.reportType);
 assert.equal(r.organizations.join(','),q.organizations.join(','));
 assert.equal(r.organization,'СМУ16');
 assert.equal(r.metric,'amount');assert.equal(r.category,'all');
 assert.ok(rankingCalls.some(x=>x.action==='show'));
 e.rankingCategory.value='work';
 e.rankingMetric.value='count';
 await e.rankingMetric.events.change();
 const latest=calls.filter(x=>x.kind==='ranking').at(-1);
 assert.equal(latest.category,'work');
 assert.equal(latest.metric,'count');
 e.analysisFrom.value='2024-03-01';e.analysisFrom.events.change();
 assert.equal(rankingCalls.at(-1).action,'clear');
 assert.match(rankingCalls.at(-1).message,/Фильтры изменены/);
});
test('SQL ranking refuses incomplete aggregates without hiding still-valid KPI statuses',async()=>{
 const {e,window,rankingCalls}=mount();
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-01-01';e.analysisTo.value='2024-12-31';
 await e.analysisPrepare.events.click();
 await e.analysisRun.events.click();
 const last=rankingCalls.find(x=>x.action==='show');
 assert.equal(last.data.complete,false);
 assert.equal(last.data.rows.length,0);
 assert.match(e.analysisStatus.textContent,/не все периоды/);
});

test('organization trends use the exact KPI scope and invalidate on every filter change',async()=>{
 const {e,calls,window,orgTrendsCalls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisOrganization.value='АВТО';e.analysisOrganization.events.change();
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 const kpi=calls.find(x=>x.kind==='range');
 const trends=calls.find(x=>x.kind==='org_trends');
 assert.equal(trends.rev,kpi.rev);
 assert.equal(trends.reportType,kpi.reportType);
 assert.equal(trends.from,kpi.from);
 assert.equal(trends.to,kpi.to);
 assert.equal(trends.organizations.join(','),kpi.organizations.join(','));
 assert.equal(trends.organizations.length,1);
 assert.ok(orgTrendsCalls.some(x=>x.action==='show'));
 e.analysisTo.value='2024-03-31';e.analysisTo.events.change();
 assert.equal(orgTrendsCalls.at(-1).action,'clear');
 assert.match(orgTrendsCalls.at(-1).message,/Фильтры изменены/);
});
test('a missing organization trend stays unknown without hiding verified KPI rows',async()=>{
 const {e,window,orgTrendsCalls}=mount();
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.match(e.analysisStatus.textContent,/не все периоды/);
 const plotted=orgTrendsCalls.find(x=>x.action==='show');
 assert.equal(plotted.data.series[0].points[0].missing,1);
 assert.equal(plotted.data.series[0].points[0].total.amount,null);
});

test('work detail chart requests the exact selected key and report scope',async()=>{
 const {e,calls,window,workTrendCalls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisOrganization.value='СМУ16';e.analysisOrganization.events.change();
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(e.workRows.children.length,50);
 const button=e.workRows.children[0].children[6].children[0];
 assert.equal(button.textContent,'График');
 button.events.click();
 await new Promise(resolve=>setImmediate(resolve));
 const summary=calls.find(x=>x.kind==='range');
 const selected=calls.find(x=>x.kind==='work_timeline');
 assert.equal(selected.rev,summary.rev);
 assert.equal(selected.reportType,summary.reportType);
 assert.equal(selected.from,summary.from);
 assert.equal(selected.to,summary.to);
 assert.equal(selected.organizations.join(','),summary.organizations.join(','));
 assert.equal(selected.organization,'СМУ16');
 assert.equal(selected.key,'work:0000');
 assert.equal(workTrendCalls.at(-1).action,'show');
 assert.equal(workTrendCalls.at(-1).data.key,'work:0000');
 assert.equal(e.workTrendMetric.disabled,false);
 e.analysisTo.value='2024-03-31';e.analysisTo.events.change();
 assert.equal(workTrendCalls.at(-1).action,'clear');
 assert.equal(e.workTrendMetric.disabled,true);
});
test('late SQLite trend response cannot resurrect a graph after filters change',async()=>{
 const opts={complete:true,delayedWorkTrend:true};
 const {e,window,workTrendCalls}=mount(opts);
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-01';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 e.workRows.children[0].children[6].children[0].events.click();
 assert.equal(typeof opts.resolveWorkTrend,'function');
 e.analysisFrom.value='2024-03-01';e.analysisFrom.events.change();
 opts.resolveWorkTrend();
 await new Promise(resolve=>setImmediate(resolve));
 assert.ok(!workTrendCalls.some(x=>x.action==='show'));
 assert.equal(workTrendCalls.at(-1).action,'clear');
});

test('SQL comparison uses the exact current KPI scope and explicit baseline dates',async()=>{
 const {e,calls,window,comparisonCalls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisOrganization.value='СМУ16';e.analysisOrganization.events.change();
 e.analysisFrom.value='2024-02-29';e.analysisTo.value='2024-03-01';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.match(e.comparisonCurrentContext.textContent,/2024-02-29/);
 assert.equal(e.comparisonRun.disabled,true);
 e.comparisonBeforeFrom.value='2024-02-27';e.comparisonBeforeFrom.events.change();
 e.comparisonBeforeTo.value='2024-02-28';e.comparisonBeforeTo.events.change();
 assert.equal(e.comparisonRun.disabled,false);
 await e.comparisonRun.events.click();
 const c=calls.find(x=>x.kind==='comparison'),kpi=calls.find(x=>x.kind==='range');
 assert.equal(c.rev,kpi.rev);
 assert.equal(c.reportType,kpi.reportType);
 assert.equal(c.from,kpi.from);assert.equal(c.to,kpi.to);
 assert.equal(c.organizations.join(','),kpi.organizations.join(','));
 assert.equal(c.beforeFrom,'2024-02-27');
 assert.equal(c.beforeTo,'2024-02-28');
 assert.equal(comparisonCalls.at(-1).action,'show');
 e.comparisonBeforeFrom.value='2024-02-26';e.comparisonBeforeFrom.events.change();
 assert.equal(comparisonCalls.at(-1).action,'clear');
 e.analysisFrom.value='2024-03-02';e.analysisFrom.events.change();
 assert.equal(e.comparisonRun.disabled,true);
 assert.match(e.comparisonCurrentContext.textContent,/после расчёта/);
});
test('SQL comparison can return incomplete summary and blocks stale responses',async()=>{
 const opts={complete:false,delayedComparison:true};
 const {e,window,comparisonCalls}=mount(opts);
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-29';e.analysisTo.value='2024-03-01';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 e.comparisonBeforeFrom.value='2024-02-28';e.comparisonBeforeFrom.events.change();
 e.comparisonBeforeTo.value='2024-02-28';e.comparisonBeforeTo.events.change();
 e.comparisonRun.events.click();
 assert.equal(typeof opts.resolveComparison,'function');
 e.analysisOrganization.value='АВТО';e.analysisOrganization.events.change();
 opts.resolveComparison();
 await new Promise(resolve=>setImmediate(resolve));
 assert.ok(!comparisonCalls.some(x=>x.action==='show'));
 assert.equal(comparisonCalls.at(-1).action,'clear');
});

test('partial SQL totals stay labelled observed and acknowledgement survives refresh only for identical evidence',async()=>{
 const persistent=new Map();
 const {e,window}=mount({observed:true,persistent});
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-03-01';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(e.analysisCoverageNotice.hidden,false);
 assert.match(e.analysisCoverageText.textContent,/только к имеющимся отчётам/);
 assert.equal(e.analysisValues.children[3].children[0].textContent,'7');
 assert.match(e.analysisValues.children[3].children[1].textContent,/имеющиеся данные/);
 assert.equal(e.analysisValues.children[4].children[0].textContent,'350');
 assert.match(e.analysisStatus.textContent,/Полный итог .* неизвестен/);
 await e.analysisAcknowledge.events.click();
 assert.equal(e.analysisCoverageNotice.hidden,true);
 assert.match(e.analysisStatus.textContent,/подтверждено/);
 assert.match(e.analysisValues.children[0].children[0].textContent,/известно/);
 assert.equal(persistent.size,1);
 await window.nativeWorkspaceRefresh();
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(e.analysisCoverageNotice.hidden,true,'acknowledgement survives same revision and filters');
 e.analysisFrom.value='2024-02-29';e.analysisFrom.events.change();
 assert.equal(e.analysisCoverageNotice.hidden,true,'old result is cleared');
 await e.analysisRun.events.click();
 assert.equal(e.analysisCoverageNotice.hidden,false,'new date needs new acknowledgement');
 assert.equal(persistent.size,1,'new acknowledgement is not implicit');
});

test('missing all reports does not turn SQL unknown into observed zero',async()=>{
 const {e,window}=mount({observed:false});
 await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-03-01';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(e.analysisValues.children[3].children[0].textContent,'—');
 assert.equal(e.analysisValues.children[4].children[0].textContent,'—');
 assert.equal(e.analysisCoverageNotice.hidden,false);
});

test('SQL calendar selector normalizes weekly and monthly values before calling SQLite',async()=>{
 const {window,e,calls}=mount({complete:true});
 await window.nativeWorkspaceReady;
 e.analysisType.value='week';e.analysisType.events.change();
 assert.equal(e.analysisFrom.type,'week');
 e.analysisFrom.value='2024-W09';e.analysisTo.value='2024-W10';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 const week=calls.filter(x=>x.kind==='range').at(-1);
 assert.equal(week.reportType,'week');
 assert.equal(week.from,'2024-02-26');
 assert.equal(week.to,'2024-03-04');
 e.comparisonBeforeFrom.value='2024-W07';e.comparisonBeforeFrom.events.change();
 e.comparisonBeforeTo.value='2024-W08';e.comparisonBeforeTo.events.change();
 await e.comparisonRun.events.click();
 const baseline=calls.filter(x=>x.kind==='comparison').at(-1);
 assert.equal(baseline.beforeFrom,'2024-02-12');
 assert.equal(baseline.beforeTo,'2024-02-19');
 e.analysisType.value='month';e.analysisType.events.change();
 assert.equal(e.analysisFrom.type,'month');
 assert.equal(e.comparisonBeforeFrom.type,'month');
 e.analysisFrom.value='2024-02';e.analysisTo.value='2024-03';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 const month=calls.filter(x=>x.kind==='range').at(-1);
 assert.equal(month.reportType,'month');
 assert.equal(month.from,'2024-02-01');
 assert.equal(month.to,'2024-03-01');
});


test('selected work name reaches all SQL KPI, chart, division, ranking, comparison and CSV calls',async()=>{
 const {e,calls,window}=mount({complete:true});
 await window.nativeWorkspaceReady;
 await window.nativeNameFilter.search();
 assert.equal(e.analysisName.children.length,3);
 e.analysisName.value='Работа';e.analysisName.events.change();e.analysisNameAdd.events.click();
 assert.equal(window.nativeNameFilter.selectedName(),'Работа');
 assert.match(e.analysisFilterScope.textContent,/Работа/);
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 for(const kind of ['range','timeline','org_trends','divisions','ranking','work']){
   const call=calls.filter(x=>x.kind===kind).at(-1);
   assert.ok(call,kind+' must be queried');
   assert.equal(call.selectedName,'Работа',kind+' uses exact selected work');
 }
 assert.match(e.analysisResultScope.textContent,/Работа/);
 e.comparisonBeforeFrom.value='2024-02-26';e.comparisonBeforeTo.value='2024-02-27';
 await e.comparisonRun.events.click();
 assert.equal(calls.filter(x=>x.kind==='comparison').at(-1).selectedName,'Работа');
 await e.analysisExport.events.click();
 assert.equal(calls.filter(x=>x.kind==='export_csv').at(-1).selectedName,'Работа');
 e.analysisNameReset.events.click();
 assert.equal(window.nativeNameFilter.selectedName(),null);
 assert.equal(e.analysisValues.children.length,0,'changing selection invalidates old KPI');
 assert.equal(e.workRows.children.length,0,'changing selection invalidates detail');
 assert.match(e.analysisFilterScope.textContent,/все/);
});
test('work name selection cannot change while SQL computations are running',async()=>{
 const opt={complete:true};
 const {e,window}=mount(opt);await window.nativeWorkspaceReady;
 await window.nativeNameFilter.search();
 e.analysisName.value='Группа';e.analysisName.events.change();e.analysisNameAdd.events.click();
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();
 assert.equal(e.analysisName.disabled,false);
 await e.analysisRun.events.click();
 assert.equal(e.analysisName.disabled,false);
 assert.equal(window.nativeNameFilter.selectedName(),'Группа');
});

test('daily source uses identical full-month day bounds for every SQL analysis block and CSV',async()=>{
 const {e,calls,window}=mount({complete:true});await window.nativeWorkspaceReady;
 e.analysisSource.value='daily';e.analysisSource.events.change();
 e.analysisType.value='month';e.analysisType.events.change();
 e.analysisFrom.value='2024-02';e.analysisTo.value='2024-02';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(calls.filter(c=>c.kind==='index').at(-1).reportType,'day');
 for(const kind of ['range','timeline','org_trends','divisions','ranking','work']){
  const call=calls.filter(c=>c.kind===kind).at(-1);
  assert.ok(call,kind);assert.equal(call.reportType,'day',kind);
  assert.equal(call.from,'2024-02-01',kind);assert.equal(call.to,'2024-02-29',kind);
 }
 e.comparisonBeforeFrom.value='2024-01';e.comparisonBeforeTo.value='2024-01';
 await e.comparisonRun.events.click();
 const comparison=calls.filter(c=>c.kind==='comparison').at(-1);
 assert.equal(comparison.reportType,'day');assert.equal(comparison.beforeFrom,'2024-01-01');assert.equal(comparison.beforeTo,'2024-01-31');
 await e.analysisExport.events.click();
 const csv=calls.filter(c=>c.kind==='export_csv').at(-1);
 assert.equal(csv.reportType,'day');assert.equal(csv.to,'2024-02-29');
 assert.match(e.analysisResultScope.textContent,/только дневные/);
});

test('multiple exact names reach each SQLite analysis block and CSV',async()=>{
 const {e,calls,window}=mount({complete:true});
 await window.nativeWorkspaceReady;
 await window.nativeNameFilter.search();
 for(const name of ['Работа','Группа']){
  e.analysisName.value=name;
  e.analysisName.events.change();
  e.analysisNameAdd.events.click();
 }
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 e.comparisonBeforeFrom.value='2024-02-26';e.comparisonBeforeTo.value='2024-02-27';
 await e.comparisonRun.events.click();
 await e.analysisExport.events.click();
 e.workRows.children[0].children[6].children[0].events.click();
 await new Promise(resolve=>setImmediate(resolve));
 for(const kind of ['range','timeline','org_trends','divisions','ranking','work','work_timeline','comparison','export_csv']){
  const call=calls.filter(x=>x.kind===kind).at(-1);
  assert.equal(call.selectedName,null,kind);
  assert.deepEqual(Array.from(call.selectedNames),['Группа','Работа'],kind);
 }
});

test('verified mutations queue a mandatory refresh behind active SQL analysis',async()=>{
 const {e,window}=mount({complete:true});await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();
 const original=window.desktopAPI.nativeAnalysisRange;
 let release,entered=false;
 window.desktopAPI.nativeAnalysisRange=async(...args)=>{entered=true;await new Promise(r=>{release=r;});return original(...args);};
 const analysis=e.analysisRun.events.click();while(!entered)await Promise.resolve();
 window.desktopAPI.nativeSettingsBootstrap=async()=>({revision:13,reports:367,indexed_reports:366,groups:1,catalog:0,org_names:['СМУ16','АВТО']});
 const refresh=window.nativeWorkspaceRefresh();release();await analysis;
 assert.equal(await refresh,true);
 assert.equal(e.metricReports.textContent,367);
 assert.equal(e.analysisValues.children.length,0,'old-revision KPI must be cleared by mandatory refresh');
});

test('queued refresh cannot bootstrap a newer snapshot before older report and group pages finish',async()=>{
 const {e,window}=mount();await window.nativeWorkspaceReady;
 let bootstraps=0,releaseReport,releaseGroup,groupStarted=false;
 const report=window.desktopAPI.nativeReportPage,group=window.desktopAPI.nativeGroupPage;
 let reportCalls=0,groupCalls=0;
 window.desktopAPI.nativeSettingsBootstrap=async()=>({revision:12+(++bootstraps),reports:366+bootstraps,indexed_reports:366,groups:1,catalog:0,org_names:['СМУ16','АВТО']});
 window.desktopAPI.nativeReportPage=async(...args)=>{if(++reportCalls===1)await new Promise(r=>{releaseReport=r;});return report(...args);};
 window.desktopAPI.nativeGroupPage=async(...args)=>{if(++groupCalls===1){groupStarted=true;await new Promise(r=>{releaseGroup=r;});}return group(...args);};
 const first=window.nativeWorkspaceRefresh();while(!releaseReport)await Promise.resolve();
 const second=window.nativeWorkspaceRefresh();releaseReport();while(!groupStarted)await Promise.resolve();
 const beforeRelease=bootstraps;
 releaseGroup();await first;assert.equal(await second,true);
 assert.equal(beforeRelease,1,'new bootstrap must wait until both old pages complete');
 assert.equal(e.metricReports.textContent,368);
});

test('printing requires a calculated current scope and does not save report data',async()=>{
 const {e,window}=mount({complete:true});await window.nativeWorkspaceReady;
 let prints=0;window.print=()=>{prints++;};
 await e.analysisPrint.events.click();assert.equal(prints,0);
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 assert.equal(e.analysisPrint.disabled,false);e.analysisPrint.events.click();assert.equal(prints,1);
 e.analysisFrom.events.change();assert.equal(e.analysisPrint.disabled,true);
});

test('failed metadata refresh clears prior analysis and disables printing',async()=>{
 const {e,window}=mount({complete:true});await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 let prints=0;window.print=()=>{prints++;};
 window.desktopAPI.nativeSettingsBootstrap=async()=>{throw Error('Unavailable');};
 assert.equal(await window.nativeWorkspaceRefresh(),false);
 assert.equal(e.analysisValues.children.length,0);assert.equal(e.analysisPrint.disabled,true);
 e.analysisPrint.events.click();assert.equal(prints,0);
});
test('completed comparison context includes the baseline dates for print output',async()=>{
 const {e,window}=mount({complete:true});await window.nativeWorkspaceReady;
 e.analysisFrom.value='2024-02-28';e.analysisTo.value='2024-02-29';
 await e.analysisPrepare.events.click();await e.analysisRun.events.click();
 e.comparisonBeforeFrom.value='2024-02-26';e.comparisonBeforeTo.value='2024-02-27';
 await e.comparisonRun.events.click();
 assert.match(e.comparisonCurrentContext.textContent,/2024-02-26.*2024-02-27/);
});
