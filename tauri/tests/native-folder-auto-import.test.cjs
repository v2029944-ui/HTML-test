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
 const window={confirm:options.confirm||(()=>true),nativeWorkspaceRefresh:async()=>{calls.push('refresh');return true;},desktopAPI:{
  nativeFolderImportSession:async enabled=>{calls.push(['session',enabled]);return enabled?'session-1':null;},
  nativeFolderImportPreview:async request=>{calls.push(['preview',request]);return options.preview?options.preview(request,preview):preview;},
  nativeFolderImportApply:async(request,token)=>{calls.push(['apply',request,token]);return options.apply?options.apply(request):{reports:1,revision:request.revision+1};},
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
test('duplicates, replacements, overlaps and warnings require manual review without writes',async()=>{
 for(const field of ['new_reports','replacements','cross_type_overlaps','issues']){
  const x=mount();if(field==='issues')x.preview.quality.issues=1;else x.preview.conflicts[field]=field==='new_reports'?0:1;
  await x.toggle(true);assert.equal(x.requests('apply').length,0,field);
 }
});
test('file status and last successful sync follow committed reports and the selected workspace',async()=>{
 const x=mount({apply:request=>{
  if(request.name==='failed.xlsx')throw Error('write failed');
  return {reports:1,revision:request.revision+1};
 },preview:(request,review)=>request.name==='overlap.xlsx'?{...review,conflicts:{...review.conflicts,cross_type_overlaps:1}}:review});
 x.add('failed.xlsx');x.add('overlap.xlsx');
 const result=(name,snapshot=x.snapshot)=>x.auto.fileStatus(snapshot.folder,snapshot.files.find(file=>file.name===name),snapshot.rules[name]);
 await x.toggle(true);
 assert.equal(result('one.xlsx'),'imported');
 assert.equal(result('failed.xlsx'),'manual');
 assert.equal(result('overlap.xlsx'),'manual');
 assert.deepEqual(x.requests('apply').map(request=>request.name),['one.xlsx','failed.xlsx']);
 const stamp=x.auto.lastSuccessfulImport(x.snapshot.folder);
 assert.ok(Number.isFinite(stamp)&&stamp>0);
 const changed=structuredClone(x.snapshot);
 changed.files[0].nativeSignature='different';
 assert.equal(result('one.xlsx',changed),null);
 await x.auto.updateSnapshot(null);
 assert.equal(x.auto.lastSuccessfulImport(x.snapshot.folder),stamp);
 await x.toggle(false);
 await x.auto.stopForWorkspaceChange();
 assert.equal(x.auto.lastSuccessfulImport(x.snapshot.folder),null);
 assert.equal(result('one.xlsx'),null);
});
test('a rejected import never sets a successful sync time',async()=>{
 const x=mount({apply:()=>{throw Error('write failed');}});
 await x.toggle(true);
 assert.equal(x.auto.lastSuccessfulImport(x.snapshot.folder),null);
});
test('a failed scan invalidates in-flight review without applying it',async()=>{
 const review=deferred(),started=deferred();
 const x=mount({preview:()=>{started.resolve();return review.result;}});
 const enabling=x.toggle(true);
 await started.result;
 const failedScan=x.auto.updateSnapshot(null);
 review.resolve(x.preview);
 await Promise.all([enabling,failedScan]);
 assert.deepEqual(x.requests('apply'),[]);
 assert.equal(x.auto.lastSuccessfulImport(x.snapshot.folder),null);
});
test('a late receipt from the previous workspace cannot mark the same folder as synced',async()=>{
 const apply=deferred(),started=deferred();
 const x=mount({apply:()=>{started.resolve();return apply.result;}});
 const enabling=x.toggle(true);
 await started.result;
 x.auto.forgetWorkspaceSession();
 const reloaded=x.auto.updateSnapshot(x.snapshot);
 apply.resolve({reports:1,revision:2});
 await Promise.all([enabling,reloaded]);
 assert.equal(x.auto.lastSuccessfulImport(x.snapshot.folder),null);
 assert.equal(x.auto.fileStatus(x.snapshot.folder,x.snapshot.files[0],x.snapshot.rules['one.xlsx']),null);
 await x.toggle(true);
 assert.deepEqual(x.requests('preview').map(request=>request.revision),[1,1]);
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

test('asynchronous consent keeps autoimport off until accepted, and cancellation cannot enable a stale lease',async()=>{
 const consent=deferred(),x=mount({confirm:()=>consent.result});const opening=x.toggle(true);
 assert.equal(x.calls.length,0);assert.equal(x.auto.isEnabled(),false);
 await x.toggle(false);consent.resolve(true);await opening;
 assert.deepEqual(x.requests('session'),[false]);assert.equal(x.requests('apply').length,0);assert.equal(x.auto.isEnabled(),false);
 const denied=deferred(),y=mount({confirm:()=>denied.result});const pending=y.toggle(true);denied.resolve(false);await pending;
 assert.equal(y.calls.length,0);assert.equal(y.e.sourceAutoImport.checked,false);
 const accepted=deferred(),z=mount({confirm:()=>accepted.result});const start=z.toggle(true);accepted.resolve(true);await start;
 assert.deepEqual(z.requests('session'),[true]);assert.equal(z.requests('apply').length,1);
});
test('rejected asynchronous consent leaves no enabled autoimport session',async()=>{
 const x=mount({confirm:async()=>{throw Error('dialog failed');}});await x.toggle(true);
 assert.equal(x.auto.isEnabled(),false);assert.equal(x.e.sourceAutoImport.checked,false);assert.ok(!x.requests('session').includes(true));assert.match(x.e.sourceAutoStatus.textContent,/Не удалось/);
});
test('connecting a workspace revokes an active local import and waits for its unfinished preview',async()=>{
 const preview=deferred(),started=deferred();
 const x=mount({preview:()=>{started.resolve();return preview.result;}});
 const enabling=x.toggle(true);await started.result;
 let stopped=false;const stopping=x.auto.stopForWorkspaceChange().then(()=>{stopped=true;});
 assert.equal(x.e.sourceAutoImport.checked,false);assert.equal(x.auto.isEnabled(),false);
 await Promise.resolve();assert.equal(stopped,false);
 preview.resolve(x.preview);await Promise.all([enabling,stopping]);
 assert.deepEqual(x.requests('session'),[true,false]);assert.deepEqual(x.requests('apply'),[]);
 await x.auto.updateSnapshot(x.snapshot);assert.deepEqual(x.requests('apply'),[]);
});
test('peer checkout invalidates pending consent without requesting a lease on the new database',async()=>{
 const consent=deferred(),x=mount({confirm:()=>consent.result});
 const enabling=x.toggle(true);x.auto.forgetWorkspaceSession();consent.resolve(true);await enabling;
 assert.equal(x.e.sourceAutoImport.checked,false);assert.equal(x.auto.isEnabled(),false);
 assert.deepEqual(x.requests('session'),[]);assert.deepEqual(x.requests('apply'),[]);
});
