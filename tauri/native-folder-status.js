// SQL Excel source manager with version-checked rule edits and session auto-import status.
(function nativeFolderStatus(){
'use strict';
const api=window.desktopAPI;
if(!api?.list||!api?.chooseFolder)return;
const $=id=>document.getElementById(id);
const choose=$('sourceFolderChoose'),refresh=$('sourceFolderRefresh');
const location=$('sourceFolderLocation'),status=$('sourceFolderStatus');
const rows=$('sourceFolderRows'),count=$('sourceFolderCount');
const fields={
  file:$('sourceRuleFile'),format:$('sourceRuleFormat'),type:$('sourceRuleType'),
  start:$('sourceRuleStart'),sheet:$('sourceRuleSheet'),year:$('sourceRuleYear'),
  save:$('sourceRuleSave'),status:$('sourceRuleStatus')
};
if(!choose||!refresh||!location||!status||!rows||!count||
   Object.values(fields).some(item=>!item))return;
const maxVisible=100;
let pageIndex=0,listedFiles=[],listedRules={},pagerView=null;
const pager=document.createElement('div');pager.id='sourceFolderPager';rows.parentElement?.after(pager);
const autoEnabled=()=>window.nativeFolderAutoImport?.isEnabled()===true;
const ruleEffect=()=>autoEnabled()
  ?'Автоимпорт включён. После сохранения правила новые отчёты без замечаний и пересечений могут быть добавлены автоматически.'
  :'При включении автоимпорта новое правило будет применено при проверке файла.';
const make=(tag,value)=>{
  const node=document.createElement(tag);
  if(value!==undefined)node.textContent=String(value);
  return node;
};
let folder=null,revision=null,selected=null;
let busy=false,pending=false,changing=false,closed=false;
const report=(message,error=false)=>{
  status.textContent=message;
  status.className='status '+(error?'error':'muted');
};
const ruleMessage=(message,error=false)=>{
  fields.status.textContent=message;
  fields.status.className='status '+(error?'error':'muted');
};
function lock(){
  choose.disabled=busy||changing;
  refresh.disabled=busy||changing;
  const enabled=!!selected&&!busy&&!changing;
  fields.format.disabled=!enabled;
  fields.type.disabled=!enabled||fields.format.value==='daily-wide';
  fields.start.disabled=!enabled||fields.format.value==='daily-wide';
  fields.sheet.disabled=!enabled||fields.format.value==='daily-wide';
  fields.year.disabled=!enabled||fields.format.value!=='daily-wide';
  fields.save.disabled=!enabled;
}
function invalidate(message='Выберите файл из списка, чтобы настроить правило.'){
  selected=null;
  fields.file.textContent='Файл не выбран.';
  fields.format.value='legacy';
  fields.type.value='week';
  fields.start.value='';
  fields.sheet.value='';
  fields.year.value='';
  ruleMessage(message);
  lock();
}
function openEditor(file,rule){
  if(busy||changing)return;
  const editor=$('sourceRuleEditor');
  if(editor){editor.open=true;editor.querySelector('summary')?.focus();}
  if(!file.nativeSignature||(file.nativeTooLarge??file.tooLarge)){
    invalidate();
    ruleMessage('Невозможно настроить правило: файл недоступен или слишком велик.',true);
    return;
  }
  const advanced=rule&&(Array.isArray(rule.sheets)&&rule.sheets.length>0||
    rule.mappings&&typeof rule.mappings==='object'&&Object.keys(rule.mappings).length>0);
  if(advanced){
    invalidate();
    ruleMessage('Этот файл использует расширенную настройку листов и столбцов. Откройте файл для проверки в разделе «Загрузка отчётов», измените настройки и нажмите «Запомнить настройки файла для автоимпорта».',true);
    return;
  }
  selected={name:file.name,signature:file.nativeSignature,folder,revision};
  fields.file.textContent='Файл: '+file.name;
  fields.format.value=['auto','legacy','daily-wide'].includes(rule?.format)?rule.format:'legacy';
  fields.type.value=['day','week','month'].includes(rule?.type)?rule.type:'week';
  fields.start.value=rule?.start||'';
  fields.sheet.value=rule?.sheet||'';
  fields.year.value=rule?.year||'';
  if(fields.format.value==='daily-wide')fields.type.value='day';
  ruleMessage('Проверьте формат, даты и параметры. '+ruleEffect());
  lock();
}
function isoDate(value){
  if(!/^\d{4}-\d{2}-\d{2}$/.test(value))return false;
  const date=new Date(value+'T00:00:00Z');
  return Number.isFinite(date.getTime())&&date.toISOString().slice(0,10)===value;
}
function makeRule(){
  const format=fields.format.value,wide=format==='daily-wide';
  const type=wide?'day':fields.type.value;
  const start=wide?'':fields.start.value;
  const sheet=wide?'':fields.sheet.value.trim();
  if(!['auto','legacy','daily-wide'].includes(format)||!['day','week','month'].includes(type))
    throw Error('Выбран неизвестный формат или тип отчёта.');
  if(!wide&&(!isoDate(start)||(type==='month'&&!start.endsWith('-01'))))
    throw Error('Укажите корректную дату начала. Для месяцев выбирайте первое число месяца.');
  if(sheet.length>120)throw Error('Название листа слишком длинное.');
  const result={type,start,sheet,format};
  if(wide){
    const value=fields.year.value.trim();
    if(value){
      if(!/^\d{4}$/.test(value)||Number(value)<1900||Number(value)>9999)
        throw Error('Год должен быть от 1900 до 9999.');
      result.year=Number(value);
    }
  }
  return result;
}
function displayFolder(path){
  const prefix='\\\\?\\';
  if(!path.startsWith(prefix))return path;
  const rest=path.slice(prefix.length);
  if(rest.startsWith('UNC\\'))return '\\\\'+rest.slice(4);
  return /^[a-z]:/i.test(rest)?rest:path;
}
function renderFiles(){
 rows.replaceChildren();count.textContent='Найдено XLSX: '+listedFiles.length;

    for(const file of listedFiles.slice(pageIndex*maxVisible,(pageIndex+1)*maxVisible)){
      if(!file||typeof file.name!=='string'||!Number.isFinite(file.size)||
         file.size<0||typeof file.tooLarge!=='boolean')
        throw Error('Некорректные метаданные XLSX.');
      const hasRule=Object.prototype.hasOwnProperty.call(listedRules,file.name);
      const tr=make('tr');
      tr.append(
        make('td',file.name),
        make('td',Math.ceil(file.size/1024)+' КБ'),
        make('td',(file.nativeTooLarge??file.tooLarge)?'Превышен предел нативного импорта (100 МиБ)':hasRule?'Есть правило':'Правило не задано')
      );
      const action=make('td'),button=make('button','Настроить правило'),inspect=make('button','Открыть для проверки');
      inspect.type='button';inspect.className=button.className='button secondary';action.className='source-file-actions';inspect.disabled=(file.nativeTooLarge??file.tooLarge)||!file.nativeSignature;inspect.setAttribute('aria-label','Открыть для проверки '+file.name);inspect.addEventListener('click',()=>{if(busy||changing)return;const path=folder.replace(/[\\/]$/,'')+(folder.includes('\\')?'\\':'/')+file.name;void window.unifiedReports?.openSource(path);});
      button.type='button';button.disabled=(file.nativeTooLarge??file.tooLarge)||!file.nativeSignature;
      button.setAttribute('aria-label','Настроить правило для '+file.name);
      button.addEventListener('click',()=>openEditor(file,hasRule?listedRules[file.name]:null));
      action.append(inspect,button);tr.append(action);rows.append(tr);
    }
 // A filesystem notification can arrive while the user is choosing a page.
 // Update row metadata on every scan, keeping the open chooser when its pages still apply.
 if(!pagerView||pagerView.folder!==folder||pagerView.index!==pageIndex||pagerView.total!==listedFiles.length){
   pagerView={folder,index:pageIndex,total:listedFiles.length};
   window.sqlRowScroll.attach(rows,{index:pageIndex,size:maxVisible,count:rows.children.length,total:listedFiles.length,controlsOnly:true,footer:pager,label:'Страницы файлов Excel',go:index=>{if(busy||changing)return false;pageIndex=index;renderFiles();}});
 }
 lock();
}
async function load(){
  if(closed)return;
  if(busy||changing){pending=true;return;}
  busy=true;lock();
  try{
    const result=await api.list();
    if(!result||!Array.isArray(result.files)||
       (result.folder!==null&&typeof result.folder!=='string')||
       !Number.isSafeInteger(result.revision)||result.revision<0)
      throw Error('Некорректный снимок источников SQLite.');
    const sameFolder=folder===result.folder;
    folder=result.folder;revision=result.revision;
    invalidate('Список обновлён. Выберите файл для настройки правила.');
    const files=result.files;
    location.textContent=folder?'Выбрана папка: '+displayFolder(folder):'Папка Excel ещё не выбрана.';
    listedFiles=files;listedRules=result.rules&&typeof result.rules==='object'?result.rules:{};
    pageIndex=sameFolder?Math.min(pageIndex,Math.max(0,Math.ceil(files.length/maxVisible)-1)):0;renderFiles();
    void window.nativeFolderAutoImport?.updateSnapshot(result);
    report(folder?'Папка проверена. Состояние автоимпорта указано рядом с переключателем.':
      'Выберите папку Excel. Импорт отчётов выполняется отдельно с проверкой.');
  }catch(error){
    void window.nativeFolderAutoImport?.updateSnapshot(null);
    listedFiles=[];renderFiles();
    invalidate();
    count.textContent='Список XLSX недоступен.';
    report('Не удалось проверить папку Excel: '+String(error?.message||error),true);
  }finally{
    busy=false;lock();
    if(pending&&!closed){pending=false;void load();}
  }
}
for(const id of ['format','type']){
  fields[id].addEventListener('change',()=>{
    if(fields.format.value==='daily-wide')fields.type.value='day';
    lock();
    ruleMessage('Параметры изменены. Сохраните правило после проверки.');
  });
}
fields.save.addEventListener('click',async()=>{
  if(busy||changing||!selected)return;
  let rule;
  try{rule=makeRule();}catch(error){ruleMessage(String(error.message),true);return;}
  const choice={...selected};
  changing=true;lock();
  let saved=false,reloaded=true,error=null,attempted=false;
  try{
    if(!await window.confirm('Сохранить правило для «'+choice.name+'»? '+ruleEffect())||closed)return;
    attempted=true;
    if(typeof api.nativeSetRuleChecked!=='function')throw Error('Проверяемое сохранение правил недоступно.');
    await api.nativeSetRuleChecked(choice.name,rule,choice.revision,choice.folder,choice.signature);
    saved=true;
    reloaded=false;
    try{
      if(typeof window.nativeWorkspaceRefresh==='function')
        reloaded=await window.nativeWorkspaceRefresh()===true;
    }catch{/* The rule was committed; the UI snapshot needs a manual refresh. */}
  }catch(exception){error=exception;}
  finally{
    changing=false;lock();if(attempted||error||pending)await load();
    if(saved)ruleMessage(reloaded?'Правило сохранено. Данные SQLite обновлены.':
      'Правило сохранено. Нажмите «Обновить данные», чтобы обновить ревизию SQLite.',!reloaded);
    else if(error)ruleMessage('Правило не сохранено: '+String(error?.message||error)+'. Обновите список файлов.',true);
  }
});
choose.addEventListener('click',async()=>{
  if(busy||changing)return;
  changing=true;lock();
  let changed=false,reloaded=true,attempted=false;
  try{
    if(folder&&!await window.confirm(
      'При выборе другой папки правила файлов прежней папки будут сброшены. Сохранённые отчёты в SQLite останутся. '+
      (autoEnabled()?'Автоимпорт продолжится для настроенной папки. ':'')+'Продолжить?'
    )){report('Выбор папки отменён.');return;}
    if(closed)return;
    attempted=true;
    const picked=await api.chooseFolder();
    if(picked){
      changed=true;
      reloaded=false;
      try{
        if(typeof window.nativeWorkspaceRefresh==='function')
          reloaded=await window.nativeWorkspaceRefresh()===true;
      }catch{/* Folder choice succeeded; refresh the SQL snapshot manually. */}
    }else report('Выбор папки отменён.');
  }catch(error){report('Не удалось выбрать папку: '+String(error?.message||error),true);}
  finally{
    changing=false;lock();
    if(attempted||pending)await load();
    if(changed&&!reloaded)
      report('Папка выбрана. Нажмите «Обновить данные» для актуальной ревизии SQLite.',true);
  }
});
refresh.addEventListener('click',()=>{void load();});
if(typeof api.onFilesChanged==='function'){
  const stop=api.onFilesChanged(()=>{
    if(!closed){report('Состав папки изменился. Проверьте файлы перед импортом.');void load();}
  });
  if(typeof stop==='function'&&typeof window.addEventListener==='function'){
    window.addEventListener('pagehide',()=>{closed=true;stop();},{once:true});
  }
}
invalidate();
if(window.unifiedReports)window.unifiedReports.refreshFolder=load;
window.nativeFolderReady=load();
})();
