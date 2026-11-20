// SQL-only settings controls. Keep record handles opaque across IPC.
(function(){
'use strict';
const api=window.desktopAPI;
const $=suffix=>document.getElementById('management'+suffix);
const limit=50;
const userError=window.analizUserError||(()=> 'Проверьте данные и повторите действие.');
let revision=null,writing=false,selectedGroup=null,catalogEdit=null,retainGroup=null;
const members={name:'Member',request:0,after:null,back:[],next:null,busy:false};
const catalog={name:'Catalog',request:0,after:null,back:[],next:null,busy:false};

function make(tag,text){
 const node=document.createElement(tag);
 if(text!==undefined)node.textContent=String(text);
 return node;
}
function button(label,run,name=label){
 const action=make('button',label);
 action.type='button';
 action.setAttribute('aria-label',name);
 action.addEventListener('click',run);
 return action;
}
function status(suffix,message,error=false){
 $(suffix).textContent=message;
 $(suffix).className='status '+(error?'error':'muted');
}
function controls(){
 const locked=writing||revision===null;
 $('GroupCreate').disabled=locked;
 $('MemberAdd').disabled=locked||!selectedGroup||members.busy;
 $('MemberClose').disabled=writing;
 $('CatalogSave').disabled=locked||catalog.busy;
 $('CatalogCancel').disabled=writing;
 for(const page of [members,catalog]){
  $(page.name+'Back').disabled=locked||page.busy||!page.back.length;
  $(page.name+'Next').disabled=locked||page.busy||page.next===null;
  $(page.name+'Page').textContent='Страница '+(page.back.length+1);
 }
}
function resetPage(page){
 page.request++;
 page.after=null;
 page.back=[];
 page.next=null;
 page.busy=false;
 $(page.name+'Rows').replaceChildren();
 status(page.name+'Status','');
}
function resetCatalogForm(){
 catalogEdit=null;
 $('CatalogWork').value='';
 $('CatalogOrganization').value='';
 $('CatalogSave').textContent='Добавить в справочник';
 $('CatalogCancel').hidden=true;
}
function showMembers(group){
 selectedGroup=group;
 $('Members').hidden=!group;
 $('MemberTitle').textContent=group?'Состав группы «'+group.name+'»':'Состав группы';
 $('MemberName').value='';
 resetPage(members);
 controls();
}
function memberRow(group,row,expected){
 const tr=make('tr');
 const actions=make('td');
 actions.append(button('Исключить',()=>save(expected,
  {kind:'remove_member',group:group.handle,handle:row.handle},
  'Исключить работу «'+row.work+'» из группы «'+group.name+'»?',group
 ),'Исключить '+row.work));
 tr.append(make('td',row.work),actions);
 return tr;
}
function catalogRow(row,expected){
 const tr=make('tr');
 const actions=make('td');
 actions.append(button('Изменить',()=>{
  if(writing||expected!==revision)return;
  catalogEdit=row;
  $('CatalogWork').value=row.work;
  $('CatalogOrganization').value=row.organization??'';
  $('CatalogSave').textContent='Сохранить изменение';
  $('CatalogCancel').hidden=false;
  $('CatalogWork').focus();
 },'Изменить '+row.work));
 actions.append(button('Удалить',()=>save(expected,{kind:'delete_catalog',handle:row.handle},
  'Удалить «'+row.work+'» из справочника?'
 ),'Удалить '+row.work+' из справочника'));
 tr.append(make('td',row.work),make('td',row.organization??'—'),actions);
 return tr;
}
async function load(page,after=null,back=[]){
 const group=selectedGroup,expected=revision;
 if(expected===null||page.busy||(page===members&&!group))return;
 const request=++page.request;
 page.busy=true;
 controls();
 try{
  const rows=page===members?
   await api.nativeMemberPage(expected,group.handle,after,limit):
   await api.nativeCatalogPage(expected,after,limit);
  if(page.request!==request)return;
  if(!Array.isArray(rows)||rows.length>limit||rows.some(row=>typeof row.handle!=='string')){
   throw Error('Получена некорректная страница.');
  }
  if(!rows.length&&after!==null&&back.length>page.back.length){
   page.next=null;
   return;
  }
  page.after=after;
  page.back=back;
  page.next=rows.length===limit?rows.at(-1).handle:null;
  $(page.name+'Rows').replaceChildren(...rows.map(row=>
   page===members?memberRow(group,row,expected):catalogRow(row,expected)));
  status(page.name+'Status',rows.length?'Записей на странице: '+rows.length+'.':'Записей нет.');
 }catch(error){
  if(page.request===request)status(page.name+'Status',managementError(error),true);
 }finally{
  if(page.request===request){
   page.busy=false;
   controls();
  }
 }
}
function managementError(error){
 const detail=error?.message??String(error);
 if(/^Shared source diagnostics\b/.test(detail)){
  return 'Этот отчёт связан с другими отчётами из одного источника. Удаление отдельного отчёта недоступно.';
 }
 if(/^Stale storage revision\b/.test(detail)){
  return 'База изменилась. Обновите данные перед повтором.';
 }
 if(/^Backup destination already exists\b/.test(detail)){
  return 'Файл резервной копии уже существует. Выберите новое имя.';
 }
 return userError(error);
}
function setContext(snapshot){
 revision=snapshot?.revision??null;
 const group=revision===retainGroup?.revision?retainGroup.group:null;
 showMembers(group);
 resetPage(catalog);
 resetCatalogForm();
 controls();
 return Promise.all([
  group&&load(members),
  $('Catalog').open&&load(catalog)
 ]);
}
async function save(expected,action,question,group=null){
 if(writing||expected===null||expected!==revision||!window.confirm(question))return;
 writing=true;
 controls();
 status('Status','Сохранение изменений…');
 let receipt;
 try{
  receipt=await api.nativeSettingsEdit(expected,action);
 }catch(error){
  status('Status','Изменение не сохранено. '+managementError(error),true);
  return;
 }finally{
  if(!receipt){
   writing=false;
   controls();
  }
 }
 try{
  retainGroup=group?{group,revision:receipt.revision}:null;
  if(action.kind==='create_group')$('GroupName').value='';
  const refreshed=await window.nativeWorkspaceRefresh();
  const message=receipt.cleanup_warning?
   'Изменение сохранено. Не удалось удалить временные данные редактора.':'Изменение сохранено.';
  status('Status',refreshed?message:message+' Обновите данные рабочей области.');
 }catch{
  status('Status','Изменение сохранено. Обновите данные рабочей области.');
 }finally{
  if(revision===expected)void setContext(null);
  retainGroup=null;
  writing=false;
  controls();
 }
}
function addReportAction(cell,report,expected){
 cell.append(button('Удалить отчёт',async()=>{
  if(writing||expected!==revision)return;
  const period=report.period_start+' — '+report.period_end;
  writing=true;
  controls();
  let deleted=false;
  try{
   if(await window.confirm('Удалить отчёт за '+period+' («'+report.id+'»)?')!==true)return;
   if(expected!==revision){
    status('ReportStatus','База изменилась. Выберите отчёт заново.',true);
    return;
   }
   status('ReportStatus','Удаляется отчёт…');
   await api.nativeDeleteReport(expected,report.id);
   deleted=true;
   const refreshed=await window.nativeWorkspaceRefresh();
   status('ReportStatus',refreshed?'Отчёт удалён.':
    'Отчёт удалён. Обновите данные рабочей области.');
  }catch(error){
   status('ReportStatus',deleted?'Отчёт удалён. Обновите данные рабочей области.':
    'Отчёт не удалён. '+managementError(error),!deleted);
  }finally{
   if(deleted&&revision===expected)void setContext(null);
   writing=false;
   controls();
  }
 },'Удалить отчёт '+report.id));
}
function addGroupActions(cell,group,expected){
 cell.append(button('Состав',()=>{
  if(writing||expected!==revision)return;
  showMembers(group);
  $('MemberTitle').focus();
  return load(members);
 },'Открыть состав группы '+group.name));
 cell.append(button('Удалить группу',()=>save(expected,{kind:'delete_group',handle:group.handle},
  'Удалить группу «'+group.name+'» и все связи с её работами?'
 ),'Удалить группу '+group.name));
}
$('GroupForm').addEventListener('submit',event=>{
 event.preventDefault();
 const name=$('GroupName').value.trim();
 if(!name)return;
 const id=window.crypto?.randomUUID?.()??(String(Date.now())+'-'+Math.random().toString(36).slice(2));
 return save(revision,{kind:'create_group',id,name},'Создать группу «'+name+'»?');
});
$('MemberForm').addEventListener('submit',event=>{
 event.preventDefault();
 const group=selectedGroup,work=$('MemberName').value;
 if(!group||!work.trim())return;
 return save(revision,{kind:'append_member',group:group.handle,work},
  'Добавить работу «'+work+'» в группу «'+group.name+'»?',group);
});
$('CatalogForm').addEventListener('submit',event=>{
 event.preventDefault();
 const work=$('CatalogWork').value,organization=$('CatalogOrganization').value||null;
 if(!work.trim())return;
 const action=catalogEdit?
  {kind:'update_catalog',handle:catalogEdit.handle,work,organization}:
  {kind:'create_catalog',work,organization};
 const destination=organization?' для подразделения «'+organization+'»':' без подразделения';
 const question=catalogEdit?'Заменить запись «'+catalogEdit.work+'» на «'+work+'»'+destination+'?':
  'Добавить «'+work+'» в справочник'+destination+'?';
 return save(revision,action,question);
});
$('MemberClose').addEventListener('click',()=>{if(!writing)showMembers(null);});
$('CatalogCancel').addEventListener('click',resetCatalogForm);
$('Catalog').addEventListener('toggle',()=>{
 resetPage(catalog);
 resetCatalogForm();
 controls();
 if($('Catalog').open)return load(catalog);
});
for(const page of [members,catalog]){
 $(page.name+'Back').addEventListener('click',()=>{
  if(!writing&&page.back.length)return load(page,page.back.at(-1),page.back.slice(0,-1));
 });
 $(page.name+'Next').addEventListener('click',()=>{
  if(!writing&&page.next!==null)return load(page,page.next,[...page.back,page.after]);
 });
}
controls();
window.nativeManagement={setContext,addGroupActions,addReportAction,isBusy:()=>writing};
})();
