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
 constructor(tag){this.tag=tag;this.children=[];this.events={};this.disabled=false;this.value='';this.textContent='';}
 append(...els){this.children.push(...els);}
 replaceChildren(...els){this.children=[...els];}
 addEventListener(name,callback){this.events[name]=callback;}
 setAttribute(name,value){this[name]=value;}
}
function mount(){
 const ids=['workspaceRefresh','nativeReportType','reportBack','reportNext','groupBack','groupNext','workspaceStatus','reportStatus','groupStatus','reportRows','groupRows','metricReports','metricIndexed','metricGroups','metricCatalog'];
 const e=Object.fromEntries(ids.map(id=>[id,new Element('div')]));
 const calls=[];
 const report=n=>({id:'r'+n,period_start:'2024-02-'+String(n%28+1).padStart(2,'0'),period_end:'2024-02-29',report_type:'day',source_name:'<b>untrusted</b>'});
 const api={
   nativeSettingsBootstrap:async()=>({revision:12,reports:366,indexed_reports:366,groups:1,catalog:0,org_names:['СМУ16']}),
   nativeReportPage:async(rev,filter,after,limit)=>{calls.push({kind:'report',rev,filter,after,limit});return !after?Array.from({length:100},(_,n)=>report(n)):[report(101)];},
   nativeGroupPage:async(rev,after,limit)=>{calls.push({kind:'group',rev,after,limit});return [{handle:'922337203685477580',id:'g',name:'Old',members:4}];},
   nativeGroupRename:async(rev,handle,name)=>{calls.push({kind:'rename',rev,handle,name});return 13;}
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
