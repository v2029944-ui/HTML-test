'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
function bridge(){
 const pending=[],window={__TAURI__:{core:{invoke:()=>new Promise((resolve,reject)=>pending.push({resolve,reject}))},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 return {api:window.desktopAPI,ops:window.tauriOperations,pending};
}
test('native saves are counted until completion, including parallel failures',async()=>{
 const f=bridge(),one=f.api.save({}),two=f.api.setRule('a.xlsx',{});assert.equal(f.ops.saves,2);
 f.pending[0].resolve({});await one;assert.equal(f.ops.saves,1);
 f.pending[1].reject(Error('disk'));await assert.rejects(two,/disk/);assert.equal(f.ops.saves,0);
});
test('native Excel reads block updates until their promise settles',async()=>{
 const f=bridge(),read=f.api.readExcel('a.xlsx');assert.equal(f.ops.reads,1);f.pending[0].resolve({bytes:[]});await read;assert.equal(f.ops.reads,0);
 f.ops.updating=true;await assert.rejects(f.api.readExcel('b.xlsx'),/обновления/);await assert.rejects(f.api.save({}),/обновления/);assert.equal(f.pending.length,1);
});
test('shared Excel lifetime guard restores counters on error and refuses import during update',async()=>{
 const html=fs.readFileSync(path.join(__dirname,'../../index.html'),'utf8'),start=html.indexOf('async function withExcelImport('),end=html.indexOf('\n}',start)+2;
 const context={state:{}},run=vm.runInNewContext('('+html.slice(start,end)+')',context);let resolve;
 const pending=run(()=>new Promise(r=>resolve=r));assert.equal(context.state.excelImports,1);resolve();await pending;assert.equal(context.state.excelImports,0);
 await assert.rejects(run(()=>{throw Error('parse');}),/parse/);assert.equal(context.state.excelImports,0);
 context.state.appUpdating=true;await assert.rejects(run(()=>{}),/обновления/);
});

test('native report browsing holds a read lease, forwards bounded cursor, and blocks updates',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const revision=window.desktopAPI.nativeRevision();
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[0].command,'native_read_revision');
 pending[0].resolve(7);assert.equal(await revision,7);
 const page=window.desktopAPI.nativeReportPage(7,{report_type:'day'},{start:'2026-11-01',id:'r1'},25);
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[1].command,'native_report_page');
 assert.equal(calls[1].args.revision,7);
 assert.equal(calls[1].args.filter.report_type,'day');
 assert.equal(calls[1].args.after.id,'r1');
 assert.equal(calls[1].args.limit,25);
 pending[1].reject(Error('Stale storage revision'));
 await assert.rejects(page,/Stale/);
 assert.equal(window.tauriOperations.reads,0);
 window.tauriOperations.updating=true;
 await assert.rejects(window.desktopAPI.nativeRevision(),/обновления/);
 assert.equal(calls.length,2);
});

test('native settings operations keep balanced leases and exact SQLite group IDs',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const boot=window.desktopAPI.nativeSettingsBootstrap();
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[0].command,'native_settings_bootstrap');
 pending[0].resolve({revision:3});await boot;
 const groups=window.desktopAPI.nativeGroupPage(3,'922337203685477580',50);
 assert.equal(calls[1].command,'native_group_page');
 assert.equal(calls[1].args.afterHandle,'922337203685477580');
 pending[1].resolve([]);await groups;
 const update=window.desktopAPI.nativeGroupRename(3,'922337203685477580','A');
 assert.equal(window.tauriOperations.saves,1);
 assert.equal(calls[2].command,'native_group_rename');
 assert.equal(calls[2].args.handle,'922337203685477580');
 pending[2].resolve(4);await update;
 assert.equal(window.tauriOperations.saves,0);
});

test('native D2 analytics leases and bounded page parameters are forwarded without report JSON',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const cursor={start:'2024-02-28',id:'r2'};
 const index=window.desktopAPI.nativeAnalysisIndexPage(6,'day',cursor,20);
 assert.equal(calls[0].command,'native_analysis_index_page');
 assert.equal(calls[0].args.reportType,'day');
 assert.equal(calls[0].args.after.id,'r2');
 assert.equal(calls[0].args.limit,20);
 pending[0].resolve({processed:1,next:null,done:true});await index;
 const settings=window.desktopAPI.nativeAnalysisIndexSettings(6);
 assert.equal(calls[1].command,'native_analysis_index_settings');
 pending[1].resolve(null);await settings;
 const summary=window.desktopAPI.nativeAnalysisRange(6,'day','2024-02-01','2024-02-29',['СМУ16']);
 assert.equal(calls[2].command,'native_analysis_range');
 assert.equal(calls[2].args.organizations[0],'СМУ16');
 assert.equal(window.tauriOperations.reads,1);
 pending[2].resolve({complete:false,total:{amount:null}});
 assert.equal((await summary).complete,false);
 assert.equal(window.tauriOperations.reads,0);
});

test('native division comparison forwards only a bounded SQL scope and balances reads',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const rows=window.desktopAPI.nativeAnalysisOrganizations(13,'month','2024-01-01','2024-12-01',['СМУ16','АВТО']);
 assert.equal(window.tauriOperations.reads,1);
 assert.equal(calls[0].command,'native_analysis_organizations');
 assert.equal(calls[0].args.revision,13);
 assert.equal(calls[0].args.reportType,'month');
 assert.equal(calls[0].args.organizations.length,2);
 pending[0].resolve([{org:'СМУ16',complete:false,missing:2,total:{amount:null}}]);
 assert.equal((await rows)[0].total.amount,null);
 assert.equal(window.tauriOperations.reads,0);
});

test('native bounded work/group page IPC includes exact key cursor and selected org',async()=>{
 const calls=[],pending=[],window={__TAURI__:{core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const req=window.desktopAPI.nativeAnalysisRangePage(13,'day','2024-02-01','2024-02-29',['СМУ16'],'СМУ16','work:0002',50);
 assert.equal(calls[0].command,'native_analysis_range_page');
 assert.equal(calls[0].args.revision,13);
 assert.equal(calls[0].args.reportType,'day');
 assert.equal(calls[0].args.organization,'СМУ16');
 assert.equal(calls[0].args.after,'work:0002');
 assert.equal(calls[0].args.limit,50);
 assert.equal(window.tauriOperations.reads,1);
 pending[0].resolve([{key:'work:0003',count:null,amount:null}]);
 assert.equal((await req)[0].amount,null);
 assert.equal(window.tauriOperations.reads,0);
});

test('CSV Save dialog only opts into tracked native streaming export',async()=>{
 const commands=[],pending=[],saves=[];
 const window={__TAURI__:{core:{invoke:(name,args)=>{commands.push({name,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},dialog:{
  open:async()=>null,save:async options=>{saves.push(options);return 'C:/Reports/new.csv';}
 }}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const filename=await window.desktopAPI.chooseCsvPath('analiz-day.csv');
 assert.equal(filename,'C:/Reports/new.csv');
 assert.equal(saves[0].filters[0].extensions[0],'csv');
 assert.equal(commands.length,0,'dialog must not export by itself');
 const result=window.desktopAPI.nativeExportRangeCsv(12,'day','2024-01-01','2024-12-31',['СМУ16'],null,filename);
 assert.equal(window.tauriOperations.saves,1);
 assert.equal(commands[0].name,'native_export_range_csv');
 assert.equal(commands[0].args.revision,12);
 assert.equal(commands[0].args.destination,filename);
 pending[0].resolve(null);
 await result;
 assert.equal(window.tauriOperations.saves,0);
});

test('native online backup is only launched after Save dialog',async()=>{
 const calls=[],pending=[],window={__TAURI__:{
  core:{invoke:(command,args)=>{calls.push({command,args});return new Promise((resolve,reject)=>pending.push({resolve,reject}));}},
  dialog:{open:async()=>null,save:async()=> 'C:/Backups/new.sqlite3'}
 }};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,setInterval(){},clearInterval(){}});
 const path=await window.desktopAPI.chooseSqliteBackupPath('new.sqlite3');
 assert.equal(path,'C:/Backups/new.sqlite3');
 assert.equal(calls.length,0);
 const result=window.desktopAPI.nativeBackupSqlite(path);
 assert.equal(window.tauriOperations.saves,1);
 assert.equal(calls[0].command,'native_backup_sqlite');
 assert.equal(calls[0].args.destination,path);
 pending[0].resolve(12);
 assert.equal(await result,12);
 assert.equal(window.tauriOperations.saves,0);
});
