'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
const deferred=()=>{
 const promise={};
 promise.result=new Promise(resolve=>{promise.resolve=resolve;});
 return promise;
};
function mount(options={}){
 const e={sourceAutoImport:{checked:false,events:{},addEventListener(k,v){this.events[k]=v;}},sourceAutoStatus:{}};
 const calls=[];
 const preview={reviewToken:'a'.repeat(64),reportCount:1,conflicts:{new_reports:1,replacements:0,cross_type_overlaps:0},quality:{issues:0,invalid:0,skipped:0,mismatches:0,other_notices:0,mismatch_reports:0,source_issues:0,relevant_source_issues:0,deferred_source_issues:0}};
 const snapshot={folder:'\\\\?\\C:\\Excel',revision:1,files:[],rules:{}};
 const add=(name,target=snapshot)=>{
  target.files.push({name,nativeSignature:'sig-'+name,tooLarge:false,nativeTooLarge:false});
  target.rules[name]={format:'legacy',type:'day',start:'2024-01-01',sheet:''};
 };
 add('one.xlsx');
 const window={confirm:()=>true,nativeWorkspaceRefresh:async()=>{calls.push('refresh');return true;},desktopAPI:{
  nativeFolderImportSession:async enabled=>{calls.push(['session',enabled]);return enabled?'session-1':null;},
  nativeFolderImportPreview:async request=>{calls.push(['preview',request]);return options.preview?options.preview(request,preview):preview;},
  nativeFolderImportApply:async(request,token)=>{calls.push(['apply',request,token]);return {reports:1,revision:request.revision+1};},
  list:async()=>snapshot
 }};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-folder-auto-import.js'),'utf8'),{
  window,document:{getElementById:k=>e[k]},setTimeout
 });
 const auto=window.nativeFolderAutoImport;
 return {e,auto,calls,snapshot,preview,add,
  requests:kind=>calls.filter(c=>c[0]===kind).map(c=>c[1]),
  toggle:enabled=>{e.sourceAutoImport.checked=enabled;return e.sourceAutoImport.events.change();}
 };
}
test('background import defaults off and passes only selected-folder metadata to native IPC',async()=>{
 const x=mount();
 await x.auto.updateSnapshot(x.snapshot);assert.equal(x.calls.length,0);
 await x.toggle(true);
 const [request]=x.requests('apply');
 assert.equal(request.name,'one.xlsx');
 assert.equal(request.folder,x.snapshot.folder);
 assert.equal(request.revision,1);
 assert.equal(request.signature,'sig-one.xlsx');
 assert.equal(request.session,'session-1');
 assert.equal(x.calls.find(c=>c[0]==='apply')[2],x.preview.reviewToken);
 await x.auto.updateSnapshot(x.snapshot);assert.equal(x.requests('apply').length,1);
});
test('replacements, overlaps and warnings require manual review without writes',async()=>{
 for(const field of ['replacements','cross_type_overlaps','issues']){
  const x=mount();if(field==='issues')x.preview.quality.issues=1;else x.preview.conflicts[field]=1;
  await x.toggle(true);assert.equal(x.requests('apply').length,0,field);
 }
});
test('disabling during preview revokes the native lease and prevents a new apply',async()=>{
 const pending=deferred(), started=deferred();
 const x=mount({preview:()=>{started.resolve();return pending.result;}});
 const running=x.toggle(true);
 await started.result;
 await x.toggle(false);pending.resolve(x.preview);await running;
 assert.deepEqual(x.requests('session'),[true,false]);
 assert.equal(x.requests('apply').length,0);
});
test('a changed snapshot arriving during preview is drained without a second notification',async()=>{
 const pending=deferred(), started=deferred();
 let previews=0;
 const x=mount({preview:(_,review)=>{
  if(++previews!==1)return review;
  started.resolve();return pending.result;
 }});
 const running=x.toggle(true);
 await started.result;
 const changed=structuredClone(x.snapshot);x.add('two.xlsx',changed);
 const queued=x.auto.updateSnapshot(changed);
 pending.resolve(x.preview);
 await Promise.all([running,queued]);
 assert.deepEqual(x.requests('apply').map(r=>[r.name,r.revision]),[['one.xlsx',1],['two.xlsx',2]]);
});
test('bounded passes drain more than 100 files and quarantine a corrupt file without losing committed refresh',async()=>{
 const x=mount({preview:(request,review)=>{
  if(request.name==='bad.xlsx')throw Error('corrupt zip');
  return review;
 }});
 x.add('bad.xlsx');
 for(let i=0;i<105;i++)x.add('report-'+i+'.xlsx');
 await x.toggle(true);
 assert.equal(x.requests('apply').length,106);
 assert.match(x.e.sourceAutoStatus.textContent,/проверки файлов: 1/);
 assert.ok(x.calls.includes('refresh'));
 await x.auto.updateSnapshot(x.snapshot);
 assert.equal(x.requests('preview').filter(r=>r.name==='bad.xlsx').length,1);
});
