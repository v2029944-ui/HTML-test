'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
class E {
 constructor(tag){this.tag=tag;this.children=[];this.events={};this.checked=false;this.disabled=false;this.value='';this.textContent='';this.className='';}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=[...nodes];}
 addEventListener(event,fn){this.events[event]=fn;}
}
const goodPreview={
  reviewToken:'a'.repeat(64),revision:12,reportCount:2,rowCount:8,
  conflicts:{new_reports:0,replacements:2,cross_type_overlaps:1},
  quality:{issues:1,relevant_source_issues:0},
  sampleReports:[{start:'2026-11-16',end:'2026-11-22',kind:'week',rows:4,issues:1}],
  sampleReportIssues:[{reason:'bad quantity',count_cell:'E29'}],
  sampleSourceIssues:[]
};
function mount(options={}){
 const ids=['legacyChoose','legacyPreview','legacyApply','legacyType','legacyStart','legacySheet','legacyFile','legacyStatus','legacySummary','legacyIssues','legacyRows','legacyReplace','legacyOverlap','legacyQuality'];
 const e=Object.fromEntries(ids.map(id=>[id,new E('div')]));
 e.legacyType.value='week';
 const calls=[];
 const api={
  chooseWideXlsx:async()=>options.file===undefined?'C:/Excel/report.xlsx':options.file,
  nativeSettingsBootstrap:async()=>({org_names:['СМУ16','АВТО','ТРАК','ЦЕНТР']}),
  nativeLegacyImportPreview:async(source,args)=>{
    calls.push({kind:'preview',source,args});
    return options.preview===undefined?goodPreview:options.preview;
  },
  chooseSqliteBackupPath:async name=>{calls.push({kind:'backup_path',name});return options.backup===undefined?'C:/Backup/previous.sqlite3':options.backup;},
  nativeLegacyImportApply:async(source,args,token,flags)=>{
    calls.push({kind:'apply',source,args,token,flags});
    if(options.fail)throw Error('Stale SQLite revision');
    return {reports:2,rows:8,revision:13};
  }
 };
 const window={
  desktopAPI:api,
  confirm:()=>options.confirm!==false,
  nativeWorkspaceRefresh:async()=>{calls.push({kind:'refresh'});return options.refresh!==false;}
 };
 const doc={getElementById:id=>e[id],createElement:tag=>new E(tag)};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-legacy-import.js'),'utf8'),{window,document:doc});
 return {e,calls};
}
async function review(ui){
 const {e}=ui;
 await e.legacyChoose.events.click();
 e.legacySheet.value='Лист1';
 e.legacyStart.value='2026-11-16';
 await e.legacyPreview.events.click();
}
test('SQL-only legacy importer is packaged, never embedded in standalone HTML',()=>{
 const root=path.resolve(__dirname,'../..');
 const out=fs.mkdtempSync(path.join(os.tmpdir(),'legacy-native-'));
 try{
  prepare({base:root,output:out});
  const html=fs.readFileSync(path.join(out,'native-workspace.html'),'utf8');
  assert.match(html,/tauri-native-legacy-import.js/);
  assert.match(html,/id="legacyApply"/);
  assert.equal(fs.readFileSync(path.join(out,'tauri-native-legacy-import.js'),'utf8'),
    fs.readFileSync(path.join(root,'tauri/native-legacy-import.js'),'utf8'));
  assert.ok(!fs.readFileSync(path.join(root,'index.html'),'utf8').includes('tauri-native-legacy-import.js'));
 }finally{fs.rmSync(out,{recursive:true,force:true});}
});
test('legacy preview passes exact sheet/date and only bounded metadata, not Excel bytes',async()=>{
 const ui=mount();await review(ui);
 const {e,calls}=ui;
 assert.equal(calls[0].kind,'preview');
 assert.equal(calls[0].args.kind,'week');
 assert.equal(calls[0].args.start,'2026-11-16');
 assert.equal(calls[0].args.sheet,'Лист1');
 assert.equal(calls[0].args.organizations.length,4);
 assert.equal(e.legacyRows.children.length,1);
 assert.equal(e.legacyRows.children[0].children[0].textContent,'2026-11-16');
 assert.match(e.legacyIssues.textContent,/E29/);
 assert.equal(e.legacyApply.disabled,false);
 assert.equal(calls.some(x=>x.kind==='read_excel'||x.kind==='load_data'),false);
});
test('destructive replacement requires individual acknowledgments and Rust-owned backup',async()=>{
 const ui=mount();await review(ui);
 const {e,calls}=ui;
 await e.legacyApply.events.click();
 assert.equal(calls.some(x=>x.kind==='apply'),false);
 e.legacyReplace.checked=true;
 await e.legacyApply.events.click();
 assert.equal(calls.some(x=>x.kind==='apply'),false);
 e.legacyOverlap.checked=true;e.legacyQuality.checked=true;
 await e.legacyApply.events.click();
 const backup=calls.findIndex(x=>x.kind==='backup_path'),apply=calls.findIndex(x=>x.kind==='apply'),refresh=calls.findIndex(x=>x.kind==='refresh');
 assert.ok(backup>=0&&backup<apply&&apply<refresh);
 assert.equal(calls[apply].flags.backupDestination,'C:/Backup/previous.sqlite3');
 assert.equal(calls[apply].flags.acceptQuality,true);
 assert.equal(calls[apply].flags.replaceExact,true);
 assert.equal(calls[apply].token,'a'.repeat(64));
 assert.match(e.legacyStatus.textContent,/Сводка обновлена/);
 assert.equal(e.legacyApply.disabled,true);
});
test('cancelled backup, declining final approval or invalid date never writes',async()=>{
 const cancelled=mount({backup:null});await review(cancelled);
 const e=cancelled.e;
 e.legacyReplace.checked=true;e.legacyOverlap.checked=true;e.legacyQuality.checked=true;
 await e.legacyApply.events.click();
 assert.equal(cancelled.calls.some(c=>c.kind==='apply'),false);
 assert.match(e.legacyStatus.textContent,/отменён/);
 const denied=mount({confirm:false});await review(denied);
 denied.e.legacyReplace.checked=true;denied.e.legacyOverlap.checked=true;denied.e.legacyQuality.checked=true;
 await denied.e.legacyApply.events.click();
 assert.equal(denied.calls.some(c=>c.kind==='apply'),false);
 const invalid=mount();await invalid.e.legacyChoose.events.click();
 invalid.e.legacySheet.value='';invalid.e.legacyStart.value='invalid';
 await invalid.e.legacyPreview.events.click();
 assert.equal(invalid.calls.some(c=>c.kind==='preview'),false);
 assert.equal(invalid.e.legacyApply.disabled,true);
});
test('failure at apply invalidates old reviewed token; no false success reported',async()=>{
 const ui=mount({fail:true});await review(ui);
 const e=ui.e;e.legacyReplace.checked=true;e.legacyOverlap.checked=true;e.legacyQuality.checked=true;
 await e.legacyApply.events.click();
 assert.equal(e.legacyApply.disabled,true);
 assert.match(e.legacyStatus.textContent,/Не сохранено/);
 assert.equal(ui.calls.some(x=>x.kind==='refresh'),false);
});
test('successful commit with failed view refresh remains successful',async()=>{
 const ui=mount({refresh:false});await review(ui);
 const e=ui.e;e.legacyReplace.checked=true;e.legacyOverlap.checked=true;e.legacyQuality.checked=true;
 await e.legacyApply.events.click();
 assert.match(e.legacyStatus.textContent,/Обновите сводку вручную/);
 assert.doesNotMatch(e.legacyStatus.textContent,/Не сохранено/);
});
test('new reports without replacement do not request an unrelated backup',async()=>{
 const ui=mount({preview:{...goodPreview,conflicts:{new_reports:2,replacements:0,cross_type_overlaps:0},quality:{issues:0}}});
 await review(ui);
 await ui.e.legacyApply.events.click();
 const call=ui.calls.find(x=>x.kind==='apply');
 assert.equal(call.flags.backupDestination,null);
 assert.equal(ui.calls.some(x=>x.kind==='backup_path'),false);
});
