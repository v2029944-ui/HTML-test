// Tauri SQL-only importer for ordinary "Work / Count / Amount" XLSX columns.
// XLSX is parsed by the on-disk Rust converter; the WebView sees summaries only.
(function nativeLegacyImport(){
'use strict';
const api=window.desktopAPI,$=id=>document.getElementById(id);
if(!api?.nativeLegacyImportPreview||!api?.nativeLegacyImportApply)return;
const nodes={
  choose:$('legacyChoose'),preview:$('legacyPreview'),apply:$('legacyApply'),
  type:$('legacyType'),start:$('legacyStart'),sheet:$('legacySheet'),
  file:$('legacyFile'),status:$('legacyStatus'),summary:$('legacySummary'),
  issues:$('legacyIssues'),samples:$('legacyRows'),
  replace:$('legacyReplace'),overlap:$('legacyOverlap'),quality:$('legacyQuality')
};
if(Object.values(nodes).some(x=>!x))return;
let source=null,options=null,review=null,busy=false,sheets=[];
const el=(tag,value)=>{
  const node=document.createElement(tag);
  if(value!==undefined)node.textContent=String(value);
  return node;
};
function status(message,error=false){
  nodes.status.textContent=message;
  nodes.status.className='status '+(error?'error':'muted');
}
function lock(){
  for(const key of ['choose','type','start'])nodes[key].disabled=busy;
  const mappings=$('legacyMappingText');if(mappings)mappings.disabled=busy;
  nodes.sheet.disabled=busy||!source||!sheets.length;
  nodes.preview.disabled=busy||!source||!sheets.length;
  nodes.apply.disabled=busy||!review;
  for(const key of ['replace','overlap','quality'])nodes[key].disabled=busy;
}
function reset(message){
  review=null;options=null;
  nodes.summary.replaceChildren();nodes.samples.replaceChildren();
  nodes.issues.textContent='';
  for(const key of ['replace','overlap','quality'])nodes[key].checked=false;
  if(message)status(message);
  lock();
}
function validDate(value){
  if(!/^\d{4}-\d{2}-\d{2}$/.test(value))return false;
  const date=new Date(value+'T00:00:00Z');
  return Number.isFinite(date.getTime())&&date.toISOString().slice(0,10)===value;
}
function getOptions(organizations){
  const start=nodes.start.value;
  const kind=nodes.type.value;
  const sheet=nodes.sheet.value.trim();
  if(!['day','week','month'].includes(kind)||!validDate(start)||
      (kind==='month'&&!start.endsWith('-01')))
    throw Error('Укажите корректный тип и дату первого периода (месяц — с первого числа).');
  if(!sheet||!sheets.includes(sheet))throw Error('Выберите существующий лист Excel из списка.');
  if(!Array.isArray(organizations)||organizations.length!==4||
      organizations.some(s=>typeof s!=='string'||!s.trim()))
    throw Error('В настройках должны быть четыре подразделения.');
  const text=$('legacyMappingText')?.value||'';
  const mappings=window.nativeImportMappings?.parseMappings(text);
  if(text.trim()&&!mappings)throw Error('Настройка столбцов недоступна. Перезапустите приложение.');
  return {kind,start,sheet,organizations:organizations.slice(),...(mappings?{mappings}:{})};
}
async function selectSource(chosenPath){
  if(busy)return false;
  source=null;sheets=[];nodes.sheet.replaceChildren();nodes.sheet.value='';
  nodes.file.textContent='Файл не выбран.';
  busy=true;reset();lock();
  try{
    const path=chosenPath??await api.chooseWideXlsx();
    if(typeof path==='string'&&path){
      source=path;
      nodes.file.textContent='Выбран XLSX: '+path.split(/[\\/]/).pop();
      if(typeof api.nativeLegacyImportSheets!=='function')throw Error('Выбор листа Excel недоступен.');
      const found=await api.nativeLegacyImportSheets(path);
      if(!Array.isArray(found)||found.length===0||found.length>64||
          found.some(v=>typeof v!=='string'||!v.includes('/worksheets/')))
        throw Error('В XLSX не найдены поддерживаемые листы с данными.');
      sheets=found.slice();
      nodes.sheet.replaceChildren();
      for(const [i,component] of sheets.entries()){
        const option=el('option','Лист '+(i+1)+' · '+component);
        option.value=component;nodes.sheet.append(option);
      }
      nodes.sheet.value=sheets[0];
      status('XLSX проверен. Выберите лист и первую дату, затем проверьте периоды.');
      return true;
    }else {status('Выбор файла отменён.');return false;}
  }catch(error){
    source=null;sheets=[];nodes.sheet.replaceChildren();nodes.sheet.value='';
    reset();status('Не удалось выбрать XLSX: '+String(error?.message||error),true);return false;
  }finally{busy=false;lock();}
}
nodes.choose.addEventListener('click',()=>selectSource());
window.nativeLegacyImporter=Object.freeze({selectSource,busy:()=>busy,invalidate:()=>{if(!busy){source=null;reset('Выбран другой сценарий импорта.');}}});
$('legacyMappingText')?.addEventListener('input',()=>{if(!busy)reset('Столбцы изменены. Выполните проверку заново.');});
for(const key of ['type','start','sheet']){
  nodes[key].addEventListener('change',()=>reset('Параметры изменены. Проверьте файл заново.'));
}
nodes.preview.addEventListener('click',async()=>{
  if(busy||!source)return;
  busy=true;reset();lock();
  try{
    const settings=await api.nativeSettingsBootstrap();
    options=getOptions(settings.org_names);
    const result=await api.nativeLegacyImportPreview(source,options);
    if(!result||typeof result.reviewToken!=='string'||result.reviewToken.length!==64)
      throw Error('Некорректный результат проверки Rust.');
    review=result;
    const conflicts=result.conflicts||{},quality=result.quality||{};
    for(const [title,value] of [
      ['Отчётов',result.reportCount],['Строк',result.rowCount],
      ['Новых',conflicts.new_reports||0],['Замен',conflicts.replacements||0],
      ['Пересечений',conflicts.cross_type_overlaps||0],['Ошибок',quality.issues||0]
    ]){
      const cell=el('div');cell.className='metric';
      cell.append(el('strong',value),el('span',title));nodes.summary.append(cell);
    }
    for(const report of result.sampleReports||[]){
      const tr=el('tr');
      for(const value of [report.start,report.end,report.kind,report.rows,report.issues])tr.append(el('td',value));
      nodes.samples.append(tr);
    }
    const issues=[...(result.sampleReportIssues||[]),...(result.sampleSourceIssues||[])];
    nodes.issues.textContent=issues.length?issues.map(i=>{
      const cells=Array.isArray(i.cells)?i.cells:[i.work_cell,i.count_cell,i.amount_cell].filter(Boolean);
      return (i.reason||i.type||'Замечание')+
        (cells.length?' ['+cells.join(', ')+']':'');
    }).join('; '):'В проверенной выборке замечаний не обнаружено.';
    status('Предпросмотр завершён. Проверьте периоды, пересечения и ошибки до сохранения.');
  }catch(error){
    reset();status('Проверка XLSX не завершена: '+String(error?.message||error),true);
  }finally{busy=false;lock();}
});
nodes.apply.addEventListener('click',async()=>{
  if(busy||!review||!options||!source)return;
  const conflict=review.conflicts||{},quality=review.quality||{};
  const qualityWarning=Boolean((quality.issues||0)||(quality.mismatch_reports||0)||(quality.relevant_source_issues||0));
  if((conflict.replacements>0&&!nodes.replace.checked)||
      (conflict.cross_type_overlaps>0&&!nodes.overlap.checked)||
      (qualityWarning&&!nodes.quality.checked)){
    status('Подтвердите все обнаруженные замены, пересечения и замечания.',true);
    return;
  }
  if(!window.confirm('Повторно проверить XLSX в Rust и сохранить '+review.reportCount+' отчётов?'))return;
  busy=true;lock();
  try{
    let backupDestination=null;
    if(conflict.replacements>0){
      if(typeof api.chooseSqliteBackupPath!=='function')
        throw Error('Создание резервной копии недоступно. Замена запрещена.');
      backupDestination=await api.chooseSqliteBackupPath(
        'analiz-legacy-before-import-'+new Date().toISOString().slice(0,10)+'.sqlite3'
      );
      if(!backupDestination){
        status('Импорт отменён: для замены необходима резервная копия SQLite.');
        return;
      }
    }
    const result=await api.nativeLegacyImportApply(source,options,review.reviewToken,{
      replaceExact:nodes.replace.checked,
      acceptCrossType:nodes.overlap.checked,
      acceptQuality:nodes.quality.checked,
      backupDestination
    });
    reset();
    let updated=false;
    try{
      if(typeof window.nativeWorkspaceRefresh==='function')
        updated=await window.nativeWorkspaceRefresh()===true;
    }catch{/* The SQLite commit succeeded; UI refresh is optional. */}
    status('Сохранено: '+result.reports+' отчётов. Ревизия: '+result.revision+
      (updated?'. Сводка обновлена.':'. Обновите сводку вручную.'));
  }catch(error){
    reset();status('Не сохранено: '+String(error?.message||error)+'. Проверьте файл заново.',true);
  }finally{busy=false;lock();}
});
lock();
})();
