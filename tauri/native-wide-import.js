// Tauri-only, reviewed daily-wide Excel import. Never load full report
// documents into the WebView. The backend reparses and revalidates before commit.
(function(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
if(!api?.nativeWideImportPreview||!api?.nativeWideImportApply)return;
let source=null,review=null,options=null,busy=false;
const make=(tag,text)=>{
 const node=document.createElement(tag);
 if(text!==undefined)node.textContent=String(text);
 return node;
};
const status=(text,error=false)=>{
 $('wideStatus').textContent=text;
 $('wideStatus').className='status '+(error?'error':'muted');
};
function updateControls(){
 $('wideChoose').disabled=busy;
 $('wideYear').disabled=busy;
 $('widePreview').disabled=busy||!source;
 $('wideApply').disabled=busy||!review;
 for(const id of ['wideReplace','wideOverlap','wideQuality'])$(id).disabled=busy;
}
function resetReview(message){
 review=null;options=null;
 $('wideSummary').replaceChildren();
 $('wideReportRows').replaceChildren();
 $('wideIssues').textContent='';
 for(const id of ['wideReplace','wideOverlap','wideQuality'])$(id).checked=false;
 if(message)status(message);
 updateControls();
}
$('wideChoose').addEventListener('click',async()=>{
 if(busy)return;
 busy=true;updateControls();
 try{
  const path=await api.chooseWideXlsx();
  if(typeof path==='string'&&path){
   source=path;resetReview();
   const name=path.split(/[\\/]/).pop();
   $('wideFile').textContent='Выбран XLSX: '+name;
   status('Файл выбран. Выполните проверку перед сохранением.');
  }else status('Выбор XLSX отменён.');
 }catch(error){
  resetReview();status('Ошибка выбора файла: '+String(error?.message||error),true);
 }finally{busy=false;updateControls();}
});
$('wideYear').addEventListener('input',()=>resetReview('Год изменён. Проверьте Excel заново.'));
$('widePreview').addEventListener('click',async()=>{
 if(busy||!source)return;
 resetReview();busy=true;updateControls();
 try{
  const yearText=$('wideYear').value.trim();
  const year=yearText===''?null:Number(yearText);
  if(year!==null&&(!Number.isInteger(year)||year<1900||year>9999))throw Error('Укажите год от 1900 до 9999.');
  const snapshot=await api.nativeSettingsBootstrap();
  const orgs=snapshot.org_names;
  if(!Array.isArray(orgs)||orgs.length!==4||orgs.some(x=>typeof x!=='string'||!x.trim()))
   throw Error('В настройках должны быть четыре подразделения.');
  options={year,sheets:[],mappings:{},organizations:orgs.slice()};
  const result=await api.nativeWideImportPreview(source,options);
  if(!result||typeof result.reviewToken!=='string'||result.reviewToken.length!==64)
   throw Error('Некорректное подтверждение нативного импорта.');
  review=result;
  const conflict=result.conflicts||{},quality=result.quality||{};
  const metrics=[
   ['Отчётов',result.reportCount],['Строк работ',result.rowCount],
   ['Новых отчётов',conflict.new_reports||0],['Замен',conflict.replacements||0],
   ['Пересечений типов',conflict.cross_type_overlaps||0],
   ['Ошибок',quality.issues||0],['Проблем качества',quality.relevant_source_issues||0]
  ];
  const boxes=$('wideSummary');boxes.replaceChildren();
  for(const [label,value] of metrics){
   const box=make('div');box.className='metric';
   box.append(make('strong',value),make('span',label));boxes.append(box);
  }
  const rows=$('wideReportRows');rows.replaceChildren();
  for(const row of result.sampleReports||[]){
   const tr=make('tr');
   for(const value of [row.id,row.kind,row.start,row.end,row.rows,row.issues])tr.append(make('td',value));
   rows.append(tr);
  }
  const issues=[...(result.sampleReportIssues||[]),...(result.sampleSourceIssues||[])];
  $('wideIssues').textContent=issues.length?
    'Первые замечания и ячейки Excel: '+issues.map(issue=>{
      const cells=Array.isArray(issue.cells)?issue.cells:
        [issue.work_cell,issue.count_cell,issue.amount_cell].filter(Boolean);
      const cell=cells.filter(Boolean).join(', ');
      const report=issue.report_start||issue.date||'';
      return (report?report+': ':'')+(issue.reason||issue.type||'Замечание')+(cell?' ['+cell+']':'');
    }).join('; '):
    'В первой проверенной выборке замечаний источника и отчётов не обнаружено.';
  status('Проверка завершена. Перед сохранением проверьте количество замен, даты и замечания.');
 }catch(error){
  resetReview();status('Нативная проверка не завершена: '+String(error?.message||error),true);
 }finally{busy=false;updateControls();}
});
$('wideApply').addEventListener('click',async()=>{
 if(busy||!review||!options||!source)return;
 const counts=review.conflicts||{},quality=review.quality||{};
 const requireQuality=Boolean((quality.issues||0)||(quality.mismatch_reports||0)||(quality.relevant_source_issues||0));
 if((counts.replacements>0&&!$('wideReplace').checked)||
    (counts.cross_type_overlaps>0&&!$('wideOverlap').checked)||
    (requireQuality&&!$('wideQuality').checked)){
   status('Отметьте подтверждения для всех найденных замен, пересечений и замечаний.',true);
   return;
 }
 if(!window.confirm('Повторно проверить XLSX и атомарно сохранить '+review.reportCount+' отчётов в SQLite?'))return;
 busy=true;updateControls();
 try{
  let backupDestination=null;
  if(counts.replacements>0){
   // The Rust apply IPC creates a verified backup before any replacement.
   // The UI only selects the path; it cannot attest a backup was made.
   if(typeof api.chooseSqliteBackupPath!=='function')
    throw Error('Выбор резервной копии SQLite недоступен. Замена отчётов запрещена.');
   status('Перед заменой существующих отчётов требуется резервная копия SQLite.');
   const backupPath=await api.chooseSqliteBackupPath(
      'analiz-before-import-'+new Date().toISOString().slice(0,10)+'.sqlite3');
   if(backupPath===null||backupPath===undefined||backupPath===''){
    status('Сохранение отменено: перед заменой отчётов необходимо создать резервную копию.');
    return;
   }
   if(typeof backupPath!=='string')throw Error('Некорректный путь резервной копии.');
   backupDestination=backupPath;
   status('Выбран путь резервной копии. Rust проверит SQLite и сохранит копию перед заменой…');
  }
  const result=await api.nativeWideImportApply(source,options,review.reviewToken,{
   replaceExact:$('wideReplace').checked,
   acceptCrossType:$('wideOverlap').checked,
   acceptQuality:$('wideQuality').checked,
   backupDestination
  });
  resetReview();
  // A verified commit has finished. Reload only bounded SQL metadata and current
  // pages; never invoke the legacy full-history load_data path.
  let reloaded=false;
  try{
    if(typeof window.nativeWorkspaceRefresh==='function')
      reloaded=await window.nativeWorkspaceRefresh()===true;
  }catch{/* The import is committed even if the view cannot refresh. */}
  status('Сохранено отчётов: '+result.reports+'. Строк: '+result.rows+
    '. Ревизия SQLite: '+result.revision+
    (reloaded?'. Сводка SQLite обновлена.':'. Нажмите «Обновить данные» для новой сводки.'));
 }catch(error){
  resetReview();
  status('Импорт не сохранён: '+String(error?.message||error)+'. Проверьте XLSX заново.',true);
 }finally{busy=false;updateControls();}
});
updateControls();
})();
