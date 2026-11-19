// Session opt-in only. Native commands bind every write to the current folder and rule.
(function(){
'use strict';
const $=id=>document.getElementById(id),api=window.desktopAPI;
const control=$('sourceAutoImport');
const status=text=>{$('sourceAutoStatus').textContent=text;};
const batchSize=25;
let session=null,latest=null,fingerprint=null,running=null,transition=Promise.resolve(),toggleVersion=0;
const handled=new Map();
const zero=x=>Number.isSafeInteger(x)&&x===0;
function safe(review){
 const c=review?.conflicts,q=review?.quality;
 return typeof review?.reviewToken==='string'&&/^[a-f0-9]{64}$/.test(review.reviewToken)&&
  Number.isSafeInteger(review.reportCount)&&review.reportCount>0&&
  c?.new_reports===review.reportCount&&zero(c.replacements)&&zero(c.cross_type_overlaps)&&
  ['issues','invalid','skipped','mismatches','other_notices','mismatch_reports','source_issues','relevant_source_issues','deferred_source_issues'].every(k=>zero(q?.[k]));
}
function remember(snapshot){
 if(!snapshot?.folder||!Array.isArray(snapshot.files)||!Number.isSafeInteger(snapshot.revision)){
  latest=null;fingerprint=null;return;
 }
 const key=JSON.stringify([snapshot.folder,snapshot.files,snapshot.rules]);
 const sourceChanged=key!==fingerprint;
 if(sourceChanged){
  const names=new Set(snapshot.files.map(file=>file?.name));
  if(snapshot.folder!==latest?.folder)handled.clear();
  else for(const name of handled.keys())if(!names.has(name))handled.delete(name);
  fingerprint=key;
 }
 if(sourceChanged||snapshot.revision>latest.revision)latest=snapshot;
 // A listing can land after this queue's own commit. Retain the newer
 // receipt revision so that it can authorize the next file in that batch.
}
function fileKey(file,rule){
 return JSON.stringify([file.nativeSignature,rule,file.nativeTooLarge??file.tooLarge]);
}
async function drain(){
 let imported=0,refreshFailed=false;
 try{
  while(session&&latest){
   const current=latest, lease=session;
   let revision=current.revision;
   for(let offset=0;offset<current.files.length;offset+=batchSize){
    if(session!==lease||latest!==current)break;
    for(const file of current.files.slice(offset,offset+batchSize)){
     if(session!==lease||latest!==current)break;
     if(!file||typeof file.name!=='string')continue;
     const rule=Object.hasOwn(current.rules??{},file.name)?current.rules[file.name]:null;
     const key=fileKey(file,rule);
     if(handled.get(file.name)?.key===key)continue;
     let manual=true;
     if(file.nativeSignature&&!(file.nativeTooLarge??file.tooLarge)&&rule){
      const request={session:lease,folder:current.folder,revision,name:file.name,signature:file.nativeSignature};
      try{
       const preview=await api.nativeFolderImportPreview(request);
       if(session!==lease||latest!==current)break;
       if(safe(preview)){
        const receipt=await api.nativeFolderImportApply(request,preview.reviewToken);
        imported+=receipt.reports;
        revision=receipt.revision;
        manual=false;
       }
      }catch(error){
       if(session!==lease||latest!==current)break;
       if(/stale storage revision/i.test(String(error?.message??error))){
        try{remember(await api.list());}catch{/* Quarantine this attempt if metadata is unavailable. */}
        if(latest!==current)break;
       }
      }
     }
     if(latest?.folder===current.folder)handled.set(file.name,{key,manual});
    }
    if(offset+batchSize<current.files.length&&session===lease&&latest===current)
     await new Promise(resolve=>setTimeout(resolve,0));
   }
   if(latest===current){
    latest={...current,revision};
    break;
   }
   if(latest?.folder===current.folder&&latest.revision<revision)latest={...latest,revision};
  }
 }finally{
  if(imported){
   try{refreshFailed=await window.nativeWorkspaceRefresh?.()!==true;}
   catch{refreshFailed=true;}
  }
  if(session){
   const manual=[...handled].filter(([,result])=>result.manual);
   const names=manual.slice(0,5).map(([name])=>name).join(', ');
   status('Автоимпорт: добавлено отчётов '+imported+'. Требуют ручной проверки файлов: '+manual.length+
    (names?' ('+names+(manual.length>5?', …':'')+')':'')+'.'+
    ' Замены, пересечения и файлы с замечаниями требуют ручной проверки.'+
    (refreshFailed?' Нажмите «Обновить данные», чтобы увидеть сохранённые отчёты.':''));
  }else if(refreshFailed){
   status('Автоматический импорт выключен. Нажмите «Обновить данные», чтобы увидеть сохранённые отчёты.');
  }
 }
}
function pump(){
 if(!running&&session&&latest){
  const snapshot=latest;
  running=drain().finally(()=>{
   running=null;
   // The SQL refresh in drain can yield to another folder notification.
   if(session&&latest&&(latest.folder!==snapshot.folder||latest.files!==snapshot.files)&&
      latest.files.some(file=>file&&handled.get(file.name)?.key!==fileKey(file,
       Object.hasOwn(latest.rules??{},file.name)?latest.rules[file.name]:null)))return pump();
  });
 }
 return running;
}
function updateSnapshot(snapshot){
 remember(snapshot);
 return pump();
}
function stop(){
 session=null;
 toggleVersion++;
 transition=transition.catch(()=>{}).then(()=>api.nativeFolderImportSession(false));
 return transition;
}
control.checked=false;
control.addEventListener('change',async()=>{
 if(!control.checked){
  const revocation=stop(), version=toggleVersion;
  status('Автоматический импорт выключается.');
  try{
   await revocation;
   if(version===toggleVersion)status('Автоматический импорт выключен.');
  }catch{
   if(version===toggleVersion)status('Не удалось подтвердить остановку автоимпорта. Закройте программу.');
  }
  return;
 }
 const version=++toggleVersion;
 try{
  if(!await window.confirm('Разрешить в этом сеансе автоматически добавлять новые отчёты из настроенной папки? Файлы с заменами, пересечениями и замечаниями останутся для ручной проверки.')){
   if(version===toggleVersion)control.checked=false;
   return;
  }
  if(version!==toggleVersion||!control.checked)return;
  transition=transition.catch(()=>{}).then(()=>api.nativeFolderImportSession(true));
  const lease=await transition;
  if(version!==toggleVersion)return;
  if(typeof lease!=='string'||!lease)throw Error('Invalid native lease');
  session=lease;
  status('Автоимпорт включён для новых проверенных отчётов в этом сеансе.');
  const known=latest;
  const snapshot=await api.list();
  if(version!==toggleVersion)return;
  if(known===latest)await updateSnapshot(snapshot);
  else await pump();
 }catch{
  if(version!==toggleVersion)return;
  control.checked=false;
  try{await stop();}catch{/* The native lease expires when this window closes. */}
  status('Не удалось запустить автоимпорт. Автоимпорт выключен.');
 }
});
window.addEventListener?.('pagehide',()=>{if(control.checked||session)void stop().catch(()=>{});},{once:true});
window.nativeFolderAutoImport=Object.freeze({updateSnapshot,isEnabled:()=>session!==null});
})();
