'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const filename=path.join(__dirname,'../native-wide-import.js');
class Node{
 constructor(tag){this.tag=tag;this.children=[];this.events={};this.textContent='';this.value='';this.checked=false;this.disabled=false;this.className='';}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=[...children];}
 addEventListener(name,handler){this.events[name]=handler;}
}
function setup(options={}){
 const ids=['wideChoose','wideYear','widePreview','wideApply','wideFile','wideStatus','wideSummary','wideReportRows','wideIssues','wideReplace','wideOverlap','wideQuality'];
 const elements=Object.fromEntries(ids.map(id=>[id,new Node('div')]));
 const calls=[];let confirmCount=0;
 const preview=options.preview||{
   reviewToken:'a'.repeat(64),sourceName:'annual.xlsx',sourceHash:'b'.repeat(64),
   reportCount:366,rowCount:12000,revision:7,
   conflicts:{new_reports:2,replacements:364,cross_type_overlaps:1},
   quality:{issues:3,mismatch_reports:1,relevant_source_issues:0},
   sampleReports:[{id:'day-2024-02-29',kind:'day',start:'2024-02-29',end:'2024-02-29',rows:14,issues:1}],
   sampleSourceIssues:[{reason:'Source control mismatch',count_cell:'Q43'}]
 };
 const api={
   chooseWideXlsx:async()=>options.path===undefined?'C:/Excel/annual.xlsx':options.path,
   nativeSettingsBootstrap:async()=>({org_names:['СМУ16','АВТО','ТРАК','ЦЕНТР']}),
   nativeWideImportPreview:async(source,config)=>{calls.push({kind:'preview',source,config});return preview;},
   nativeWideImportApply:async(source,config,reviewToken,confirmation)=>{
     calls.push({kind:'apply',source,config,reviewToken,confirmation});
     return {reports:366,rows:12000,revision:8,replaced:364};
   }
 };
 const window={desktopAPI:api,confirm:()=>{confirmCount++;return true;}};
 const document={getElementById:id=>elements[id],createElement:tag=>new Node(tag)};
 vm.runInNewContext(fs.readFileSync(filename,'utf8'),{window,document});
 return {elements,calls,window,confirmCount:()=>confirmCount};
}
test('Tauri-only wide importer is packaged without adding it to browser index.html',()=>{
 const root=path.resolve(__dirname,'../..');
 const destination=fs.mkdtempSync(path.join(os.tmpdir(),'wide-import-ui-'));
 try{
  prepare({base:root,output:destination});
  const native=fs.readFileSync(path.join(destination,'native-workspace.html'),'utf8');
  assert.match(native,/tauri-native-wide-import.js/);
  assert.match(native,/id="wideApply"/);
  assert.ok(!fs.readFileSync(path.join(root,'index.html'),'utf8').includes('tauri-native-wide-import.js'));
  assert.equal(fs.readFileSync(path.join(destination,'tauri-native-wide-import.js'),'utf8'),fs.readFileSync(filename,'utf8'));
 }finally{fs.rmSync(destination,{recursive:true,force:true});}
});
test('native wide import requires review and independent acknowledgements',async()=>{
 const {elements:e,calls,confirmCount}=setup();
 assert.equal(e.widePreview.disabled,true);
 assert.equal(e.wideApply.disabled,true);
 await e.wideChoose.events.click();
 assert.equal(e.wideFile.textContent,'Выбран XLSX: annual.xlsx');
 e.wideYear.value='2024';
 e.wideYear.events.input();
 await e.widePreview.events.click();
 assert.equal(calls[0].kind,'preview');
 assert.equal(calls[0].config.year,2024);
 assert.equal(calls[0].config.organizations.length,4);
 assert.equal(e.wideSummary.children.length,7);
 assert.equal(e.wideReportRows.children[0].children[0].textContent,'day-2024-02-29');
 assert.match(e.wideIssues.textContent,/Q43/);
 assert.equal(e.wideApply.disabled,false);
 await e.wideApply.events.click();
 assert.equal(calls.length,1,'required confirmations missing');
 e.wideReplace.checked=true;
 await e.wideApply.events.click();
 assert.equal(calls.length,1,'cross-type and quality notices need consent');
 e.wideOverlap.checked=true;
 e.wideQuality.checked=true;
 await e.wideApply.events.click();
 assert.equal(calls.filter(c=>c.kind==='apply').length,1);
 assert.equal(calls.at(-1).reviewToken,'a'.repeat(64));
 assert.equal(calls.at(-1).confirmation.acceptQuality,true);
 assert.equal(confirmCount(),1);
 assert.equal(e.wideApply.disabled,true,'successful apply clears old preview');
 assert.match(e.wideStatus.textContent,/Сохранено отчётов: 366/);
});
test('changing the year invalidates reviewed import, and cancelling the picker does not write',async()=>{
 const {elements:e,calls}=setup({path:null});
 await e.wideChoose.events.click();
 assert.equal(e.wideApply.disabled,true);
 assert.equal(calls.length,0);
 const x=setup();
 await x.elements.wideChoose.events.click();
 await x.elements.widePreview.events.click();
 x.elements.wideYear.value='2025';x.elements.wideYear.events.input();
 assert.equal(x.elements.wideApply.disabled,true);
 await x.elements.wideApply.events.click();
 assert.equal(x.calls.filter(c=>c.kind==='apply').length,0);
});
