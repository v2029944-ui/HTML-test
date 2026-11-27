/* Original groups and settings controls; all report history stays in SQLite. */
(function(){
'use strict';
let initialized=false;
function init(){
 if(initialized)return;initialized=true;
 const U=window.unifiedUI,{api,$,el}=U,LIMIT=50;
 let revision=null,busy=false,active=false,editing=null,selected=new Set(),editTicket=0,workMode='edit';
 const controls=new Set(),pages=new Set();
 const valid=r=>Number.isSafeInteger(r)&&r===revision&&r===U.revision;
 async function confirmAction(expected,message){const accepted=await window.confirm(message);if(!valid(expected))throw Error('База изменилась. Повторите действие.');return accepted===true;}
 function requestGroupName(anchor,group,create=false){
  const dialog=el('dialog'),form=el('form'),heading=el('h2',create?'Создать подгруппу в «'+group.name+'»':'Переименовать группу');
  dialog.id='groupRenameDialog';heading.id='groupRenameTitle';dialog.setAttribute('aria-labelledby',heading.id);
  const label=el('label',create?'Название подгруппы':'Новое название группы'),input=el('input');
  input.id='groupRenameName';input.type='text';input.required=true;input.value=create?'':group.name;label.htmlFor=input.id;
  const actions=el('div',null,'actions'),cancel=el('button','Отмена','button secondary'),save=el('button','Сохранить','button');
  cancel.id='groupRenameCancel';cancel.type='button';save.id='groupRenameSubmit';save.type='submit';
  actions.append(cancel,save);form.append(heading,label,input,actions);dialog.append(form);
  const host=anchor.closest('dialog[open]')||anchor.closest('#refNative')||document.body;
  return new Promise(resolve=>{
   let answer=null;
   form.addEventListener('submit',event=>{event.preventDefault();const name=input.value.trim();if(!name){input.focus();return;}answer=name;dialog.close();});
   cancel.addEventListener('click',()=>dialog.close());
   dialog.addEventListener('cancel',event=>{event.preventDefault();dialog.close();});
   dialog.addEventListener('close',()=>{dialog.remove();resolve(answer);},{once:true});
   host.append(dialog);dialog.showModal();input.focus();input.select();
  });
 }
 const fail=e=>U.notice(window.analizUserError?window.analizUserError(e):(e?.message||String(e)),true);
 function button(text,run){const n=el('button',text,'button secondary');n.type='button';n.addEventListener('click',run);controls.add(n);return n;}
 function lock(){for(const n of controls){if(n.isConnected===false){controls.delete(n);continue;}n.disabled=busy||revision===null;}for(const p of pages){p.prev.disabled=busy||revision===null||p.loading||!p.offset;p.next.disabled=busy||revision===null||p.loading||p.offset+LIMIT>=p.total;}updateRemovalControls();}
 function bind(id,run,event='click'){const n=$(id);if(!n)return;controls.add(n);n.addEventListener(event,run);}
 function guard(run){return async()=>{if(busy||!valid(revision))return;busy=true;lock();try{await run(revision);}catch(e){fail(e);}finally{busy=false;lock();}};}
 async function mutate(expected,run,message){if(!valid(expected))throw Error('База изменилась. Обновите данные.');const result=await run();resetEditor(false);revision=null;for(const p of pages)clear(p);try{await U.refresh();}catch(e){U.notice(message+' Не удалось обновить рабочую область. '+(e?.message||String(e)),true);return result;}U.notice(message+((result?.cleanup_warning||result?.cleanup_error)?' Не удалось удалить временные данные редактора.':''));return result;}
 function pager(root,prefix){const bar=el('div',null,'change-pagination'),prev=button('← Предыдущая',()=>load(p,p.offset-LIMIT)),label=el('span','','muted small'),next=button('Следующая →',()=>load(p,p.offset+LIMIT));bar.append(prev,label,next);window.sqlRowScroll?.retire(prev,next);root.append(bar);const p={prefix,root:null,prev,next,label,offset:0,total:0,ticket:0,loading:false,query:()=>'',kind:'works',group:()=>null,render:()=>{}};pages.add(p);return p;}
 function existingPager(prefix,root){const p={prefix,root,prev:$(prefix+'Prev'),next:$(prefix+'Next'),label:$(prefix+'Page'),offset:0,total:0,ticket:0,loading:false,query:()=>'',kind:'works',group:()=>null,render:()=>{}};window.sqlRowScroll?.retire(p.prev,p.next);p.prev.addEventListener('click',()=>load(p,p.offset-LIMIT));p.next.addEventListener('click',()=>load(p,p.offset+LIMIT));pages.add(p);return p;}
 function clear(p){p.actions?.replaceChildren();window.sqlRowScroll?.reset(p.root);++p.ticket;p.loading=false;p.total=0;p.root?.replaceChildren();p.label.textContent='';lock();}
 async function load(p,offset=0){if(p.ready&&!p.ready()){clear(p);$('groupAuditCoverage').textContent=auditEmptyPeriod;return;}const expected=revision;if(!valid(expected))return;const ticket=++p.ticket;p.actions?.replaceChildren();p.loading=true;p.offset=Math.max(0,offset);p.root.replaceChildren();p.label.textContent='Загрузка…';lock();try{
  const result=p.fetch?await p.fetch(expected,p.offset):await api.unifiedManagementPage(expected,p.kind,p.query(),p.group(),p.offset,LIMIT);
  if(ticket!==p.ticket||!valid(expected))return;
  if(!result||!Array.isArray(result.rows)||result.rows.length>LIMIT||!Number.isSafeInteger(result.total)||result.total<0)throw Error('Получена некорректная страница.');
  p.total=result.total;p.render(result,expected);p.label.textContent='';
  const label={unifiedGroup:'Группы работ',work:'Работы',catalog:'Справочник',groupAudit:'Проверка групп'}[p.prefix];
  p.root.dataset.sqlScrollHost=p.prefix==='groupAudit'?'groupAuditResults':p.root.id||p.prefix;
  window.sqlRowScroll.attach(p.root,{index:Math.floor(p.offset/LIMIT),size:LIMIT,count:result.rows.length,total:p.total,label,controlsOnly:p.prefix!=='catalog',footer:p.label.parentNode,go:i=>{
   if(ticket!==p.ticket||!valid(expected)||busy)return false;
   return load(p,i*LIMIT);
  }});
 }catch(e){if(ticket===p.ticket&&valid(expected)){window.sqlRowScroll?.reset(p.root);p.root.replaceChildren();p.total=0;p.label.textContent='Не удалось загрузить данные.';fail(e);}}finally{if(ticket===p.ticket){p.loading=false;lock();}}}
 const groupPagination=el('div');$('groupList').parentNode.insertBefore(groupPagination,$('groupList').nextSibling);
 const groups=pager(groupPagination,'unifiedGroup');groups.root=$('groupList');groups.kind='groups';groups.query=()=>$('groupSearch').value;
 groups.fetch=(expected,offset,query=groups.query())=>api.unifiedManagementPage(expected,U.snapshot?.work_group_hierarchy?'group_tree':'groups',query,null,offset,LIMIT);
 const markedGroups=new Set(),maxGroupSelection=10000;
 let selectingGroups=false;
 const removalBar=el('div',null,'actions group-removal');
 const selectionStatus=el('span','','muted small');
 selectionStatus.setAttribute('role','status');
 const selectionMode=button('Выбрать группы',()=>{
  selectingGroups=!selectingGroups;markedGroups.clear();updateRemovalControls();
 });
 const selectVisible=button('Выбрать на странице',()=>{
  if(busy||groups.loading||!selectingGroups)return;
  addMarkedGroups(groupsOnPage());
 });
 const selectFound=button('Выбрать все найденные',guard(async expected=>{
  if(groups.loading||!selectingGroups)return;
  const query=groups.query(),token=window.sharedWorkspace?.state?.workspace_token;
  const allTree=Boolean(U.snapshot?.work_group_hierarchy&&!query.trim());
  const expectedTotal=matchingGroupCount();
  const handles=[];
  for(let offset=0;offset<expectedTotal;){
   const page=allTree?await api.unifiedManagementPage(expected,'groups','',null,offset,LIMIT):await groups.fetch(expected,offset,query);
   if(!valid(expected)||token!==window.sharedWorkspace?.state?.workspace_token||groups.query()!==query)
    throw Error('База или условия поиска изменились. Повторите выбор.');
   if(!validManagementPage(page,offset)||page.total!==expectedTotal)throw Error('Список групп изменился. Повторите выбор.');
   handles.push(...page.rows.map(row=>row.handle));offset+=page.rows.length;
  }
  addMarkedGroups(handles);
 }));
 const unmark=button('Снять выделение',()=>{markedGroups.clear();updateRemovalControls();});
 const removeSelected=button('Удалить выбранные',guard(expected=>removeGroups(expected,[...markedGroups])));
 const clearGroups=button('Очистить список групп',guard(expected=>removeGroups(expected,null)));
 removalBar.append(selectionMode,selectVisible,selectFound,unmark,removeSelected,clearGroups,selectionStatus);
 groups.root.parentNode.insertBefore(removalBar,groups.root);
 function addMarkedGroups(handles){
  const next=new Set([...markedGroups,...handles]);
  if(next.size>maxGroupSelection){fail(Error('Выберите не более 10 000 групп.'));return;}
  for(const handle of handles)markedGroups.add(handle);
  updateRemovalControls();
 }
 function groupsOnPage(){
  return [...groups.root.querySelectorAll('input')].filter(check=>{
   if(!check.dataset.groupHandle)return false;
   // A row is present even when its own details are closed, while its
   // descendants are visible only through open ancestor groups.
   let parent=check.closest('details')?.parentNode?.closest('details');
   while(parent){if(!parent.open)return false;parent=parent.parentNode?.closest('details');}
   return true;
  }).map(check=>check.dataset.groupHandle);
 }
 function matchingGroupCount(){
  const total=U.snapshot?.groups;
  return U.snapshot?.work_group_hierarchy&&!groups.query().trim()&&Number.isSafeInteger(total)?total:groups.total;
 }
 function updateRemovalControls(){
  selectionMode.setAttribute('aria-pressed',String(selectingGroups));
  selectionMode.textContent=selectingGroups?'Завершить выбор':'Выбрать группы';
  selectionStatus.hidden=!selectingGroups;
  selectionStatus.textContent='Выбрано групп: '+markedGroups.size;
  for(const control of [selectVisible,selectFound,unmark,removeSelected])control.hidden=!selectingGroups;
  selectVisible.disabled=busy||revision===null||groups.loading||!groupsOnPage().length;
  const matching=matchingGroupCount(),allTree=U.snapshot?.work_group_hierarchy&&!groups.query().trim();
  selectFound.disabled=busy||revision===null||groups.loading||!matching||matching>maxGroupSelection;
  selectFound.textContent=(allTree?'Выбрать все группы':'Выбрать все найденные')+' ('+matching+')';
  unmark.disabled=removeSelected.disabled=busy||revision===null||!markedGroups.size;
  for(const check of groups.root.querySelectorAll('input')){
   check.parentNode.hidden=!selectingGroups;
   check.checked=markedGroups.has(check.dataset.groupHandle);
   check.disabled=busy||revision===null;
  }
 }
 async function removeGroups(expected,handles){
  if(handles&&!handles.length)return;
  const workspace=window.sharedWorkspace?.state;
  const current=()=>valid(expected)&&workspace?.workspace_token===window.sharedWorkspace?.state?.workspace_token;
  if(!workspace?.available||workspace.pending||!workspace.workspace_token)throw Error('Активная база недоступна.');
  const page=await api.unifiedManagementPage(expected,'groups','',null,0,1);
  const total=U.snapshot?.groups;
  if(!current()||!validManagementPage(page,0)||(Number.isSafeInteger(total)&&total!==page.total))
   throw Error('Не удалось проверить активную базу и список групп.');
  const count=handles?.length??page.total;
  if(!count){U.notice('В активной базе нет групп.');return;}
  const all=count===page.total;
  const location=workspace.mode==='shared'?'Общая база · '+workspace.folder+'\nИзменение увидят все её пользователи.':'Личная база';
  const treeNote=U.snapshot?.work_group_hierarchy&&!all
   ? ' Оставшиеся подгруппы перейдут к ближайшей сохранённой группе выше или на верхний уровень.' : '';
  if(!await confirmAction(expected,location+'\nУдаляемых групп: '+count+'.'+treeNote+' В аналитике работы удаляемых групп снова будут видны под исходными названиями. Отчёты, справочник, количества и суммы сохранятся. Продолжить?'))return;
  if(!current())throw Error('Активная база изменилась. Повторите действие.');
  let backup=null;
  if(all){
   if(!await confirmAction(expected,'Очистить весь список групп? Перед удалением будет создана проверенная резервная копия SQLite.'))return;
   if(!current())throw Error('Активная база изменилась. Повторите действие.');
   backup=await api.chooseSqliteBackupPath('analiz-before-clear-groups.sqlite3');
   if(!backup)return;
  }
  if(!current())throw Error('Активная база изменилась. Повторите действие.');
  await mutate(expected,()=>api.unifiedGroupsDelete(expected,handles,count,backup),
   all?'Список групп очищен. Резервная копия создана.':'Группы удалены: '+count+'.');
 }
 let groupReload;
 function reloadGroups(offset){groupReload=load(groups,offset);return groupReload;}
 const works=existingPager('work',$('worksList')),workSearch=$('workSearch'),workLabel=workSearch.parentNode;
 const workSearchHome=workLabel.parentNode,workListHome=works.root.parentNode,workListNext=works.root.nextSibling;
 const workSearchCaption=el('span','Поиск работ');workLabel.replaceChildren(workSearchCaption,workSearch);
 const editor=$('groupEditor'),auditPanel=$('groupAuditPanel');
 let editorOpener=null;
 editor.hidden=true;
 function openEditor(opener){editorOpener=opener;editor.hidden=false;editor.open=true;}
 const auditSearchLabel=$('groupAuditSearch').parentNode,auditSearchHome=auditSearchLabel.parentNode;
 const auditListHome=$('groupAuditResults');
 auditSearchLabel.remove();
 const found=$('groupSearchCount'),coverage=$('groupCoverage'),counts=el('div',null,'group-result-counts');
 found.parentNode.insertBefore(counts,found);
 coverage.className=found.className='muted small';counts.append(found,coverage);
 works.query=()=>workSearch.value;
 function count(){return el('p','Выбрано работ: '+selected.size,'muted small');}
 function showWorkTotals(result){if(Number.isSafeInteger(result?.grouped)&&Number.isSafeInteger(result.total))coverage.textContent='Работ в группах: '+result.grouped+' из '+result.total;}
 works.render=result=>{works.root.append(count());for(const row of result.rows){const label=el('label',null,'work-row'),check=el('input');check.type='checkbox';check.checked=selected.has(row.name);check.disabled=busy;controls.add(check);check.addEventListener('change',()=>{if(busy)return;if(check.checked)selected.add(row.name);else selected.delete(row.name);works.root.children[0].textContent='Выбрано работ: '+selected.size;});label.append(check,el('span',row.name));works.root.append(label);}if(!workSearch.value.trim())showWorkTotals(result);};
 function resetEditor(reload=true){editTicket++;editing=null;selected=new Set();$('groupName').value='';if(workMode==='edit')workSearch.value='';works.offset=0;if(reload&&active&&workMode==='edit')void load(works);}
 async function editGroup(group,expected,opener){if(busy||!valid(expected))return;const ticket=++editTicket;busy=true;lock();try{
  const chosen=new Set();let offset=0,total=1;
  while(offset<total){const page=await api.unifiedManagementPage(expected,'members','',group.handle,offset,LIMIT);if(ticket!==editTicket||!valid(expected))return;if(!Array.isArray(page?.rows)||page.rows.length>LIMIT||!Number.isSafeInteger(page.total)||page.total>5000||page.total<0||(!page.rows.length&&offset<page.total))throw Error('Не удалось загрузить состав группы (лимит 5000 работ).');total=page.total;for(const row of page.rows)chosen.add(row.name);offset+=page.rows.length;}
  if(ticket!==editTicket||!valid(expected))return;editing=group;selected=chosen;showWorkMode('edit');openEditor(opener);$('groupName').value=group.name;workSearch.value='';await load(works);$('groupName').focus();
 }catch(e){if(ticket===editTicket)fail(e);}finally{busy=false;lock();}}
 function requestGroupSelection(anchor,row,expected,current,movingGroup=false){
  const dialog=el('dialog',null,'group-picker'),heading=el('h2',movingGroup?'Переместить группу':'Перенести работу'),work=el('p',row.name,'muted small');
  const label=el('label','Поиск групп'),search=el('input'),status=el('p','','muted small');
  const choices=el('div',null,'group-picker-choices'),more=el('button','Показать ещё','button secondary');
  const cancel=el('button','Отмена','button secondary');
  dialog.id='groupPickerDialog';heading.id='groupPickerTitle';search.id='groupPickerSearch';choices.id='groupPickerChoices';
  dialog.setAttribute('aria-labelledby',heading.id);status.setAttribute('role','status');search.type='search';
  more.type=cancel.type='button';more.hidden=true;label.append(search);
  dialog.append(heading,work,label,status,choices,more,cancel);
  const host=anchor.closest('dialog[open]')||anchor.closest('#refNative')||document.body;
  return new Promise(resolve=>{
   let answer,ticket=0,nextOffset=0;
   function choice(handle,name){
    const option=el('button',name,'button secondary');option.type='button';
    if(handle===(row.group||null))option.setAttribute('aria-current','true');
    option.addEventListener('click',()=>{if(current())answer=handle;dialog.close();});
    return option;
   }
   async function loadChoices(offset){
    if(!dialog.open||!current())return;
    const request=++ticket,term=search.value.trim();
    const live=()=>dialog.open&&current()&&request===ticket;
    if(offset===0){choices.replaceChildren(choice(null,movingGroup?'Верхний уровень':'Без группы'));more.hidden=true;}
    more.disabled=true;status.textContent='Загрузка групп…';
    try{
     const result=await api.unifiedManagementPage(expected,'groups',term,null,offset,LIMIT);
     if(!live())return;
     if(!validManagementPage(result,offset))throw Error('Получен некорректный список групп.');
     choices.append(...result.rows.filter(group=>!movingGroup||group.handle!==row.handle).map(group=>choice(group.handle,group.name)));
     nextOffset=offset+result.rows.length;more.hidden=nextOffset>=result.total;
     status.textContent=result.total?'Найдено групп: '+result.total:'Группы не найдены.';
    }catch(error){
     if(live())status.textContent='Не удалось загрузить группы. '+(error?.message||String(error));
    }finally{if(live())more.disabled=false;}
   }
   search.addEventListener('input',()=>loadChoices(0));more.addEventListener('click',()=>loadChoices(nextOffset));
   cancel.addEventListener('click',()=>dialog.close());
   dialog.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();dialog.close();}});
   dialog.addEventListener('cancel',event=>{event.preventDefault();dialog.close();});
   dialog.addEventListener('close',()=>{++ticket;dialog.remove();resolve(answer);},{once:true});
   host.append(dialog);
   try{dialog.showModal();}catch(error){dialog.remove();throw error;}
   search.focus();void loadChoices(0);
  });
 }
 function validManagementPage(page,offset){
  return Array.isArray(page?.rows)&&page.rows.length<=LIMIT&&Number.isSafeInteger(page.total)&&page.total>=0&&
   (page.rows.length>0||offset>=page.total)&&(!page.rows.length||offset+page.rows.length<=page.total);
 }
 function moveControl(row,expected,sourceGroup){
  const box=el('div',null,'group-member');
  const pick=button('Перенести…',async()=>{
   if(!valid(expected))return;
   let moved=false;
   await guard(async()=>{
    const opened=U.generation,workspace=window.sharedWorkspace?.state?.workspace_token;
    const current=()=>valid(expected)&&opened===U.generation&&workspace===window.sharedWorkspace?.state?.workspace_token&&box.isConnected!==false;
    const target=await requestGroupSelection(pick,row,expected,current);
    if(target===undefined||target===(row.group||null))return;
    if(!current())throw Error('База изменилась. Повторите действие.');
    await mutate(expected,()=>api.unifiedWorkAssign(expected,row.name,target),'Работа перенесена.');
    moved=true;
   })();
   if(!moved){if(pick.isConnected!==false&&!pick.disabled)pick.focus({preventScroll:true});return;}
   await groupReload;
   const desktop=$('refNative');
   if(!valid(revision)||(desktop&&desktop.dataset.panel!=='groups'))return;
   if(sourceGroup===undefined){workSearch.focus({preventScroll:true});return;}
   const replacement=Array.from(groups.root.querySelectorAll('details')).find(details=>details.dataset.groupId===sourceGroup);
   if(replacement){replacement.open=true;replacement.children[0].focus({preventScroll:true});}
   else $('groupSearch').focus({preventScroll:true});
  });
  pick.className='button secondary group-assign';pick.setAttribute('aria-haspopup','dialog');
  pick.setAttribute('aria-label','Перенести работу «'+row.name+'» в другую группу');
  box.append(el('span',row.name),pick);return box;
 }
 function expandMembers(details,root,group,expected,openChildren){
  const pageTicket=groups.ticket,status=el('p','','muted small');
  let offset=0,total=Number.isSafeInteger(group.members)&&group.members>=0?group.members:1,loading=false;
  const more=button('Показать ещё работы',loadNext);
  more.hidden=true;status.hidden=true;status.setAttribute('role','status');root.append(status,more);
  const current=()=>pageTicket===groups.ticket&&valid(expected)&&details.open&&details.isConnected!==false;
  async function loadNext(){
   if(loading||offset>=total||!current())return;
   loading=true;status.hidden=false;status.textContent='Загрузка состава группы…';
   try{
    const result=await api.unifiedManagementPage(expected,'members','',group.handle,offset,LIMIT);
    if(!current())return;
    if(!validManagementPage(result,offset))throw Error('Получен некорректный состав группы.');
    const rows=result.rows.map(row=>moveControl(row,expected,group.id));
    for(const row of rows)root.insertBefore(row,more);
    offset+=rows.length;total=result.total;status.hidden=true;
    if(document.activeElement===more)rows[0]?.querySelectorAll('button')[0]?.focus();
    more.hidden=offset>=total;
   }catch(error){
    if(current()){status.textContent='Не удалось загрузить состав группы. '+(error?.message||String(error));more.hidden=false;}
   }finally{loading=false;}
  }
  details.addEventListener('toggle',()=>{
   if(selectingGroups)updateRemovalControls();
   if(!current())return;
   openChildren?.();
   if(!offset)return loadNext();
  });
 }
 function confirmGroupTreeDeletion(anchor,group){
  const dialog=el('dialog'),heading=el('h2','Удалить группу «'+group.name+'»?');
  dialog.id='groupTreeDeletion';heading.id='groupTreeDeletionTitle';dialog.setAttribute('aria-labelledby',heading.id);
  const actions=el('div',null,'actions');
  const choices=[
   ['Отмена',null,'button secondary'],
   ['Перенести подгруппы на уровень выше',false,'button secondary'],
   ['Удалить группу и подгруппы',true,'button danger-button']
  ];
  const host=anchor.closest('dialog[open]')||anchor.closest('#refNative')||document.body;
  return new Promise((resolve,reject)=>{
   let answer=null;
   for(const [label,choice,style] of choices){
    const action=el('button',label,style);action.type='button';
    action.addEventListener('click',()=>{answer=choice;dialog.close();});actions.append(action);
   }
   dialog.append(heading,el('p','Непосредственных подгрупп: '+group.children+'. Исходные работы и отчёты сохранятся.'),actions);
   dialog.addEventListener('cancel',event=>{event.preventDefault();dialog.close();});
   dialog.addEventListener('close',()=>{dialog.remove();resolve(answer);},{once:true});
   host.append(dialog);
   try{dialog.showModal();actions.children[0].focus();}catch(error){dialog.remove();reject(error);}
  });
 }
 function renderGroupRow(root,group,expected,hierarchy,depth=0){
  const details=el('details',null,'group-details'),summary=el('summary',group.name);
  const actions=el('div',null,'actions'),memberRoot=el('div',null,'group-members');
  details.dataset.groupId=group.id;
  summary.append(el('small',(group.children?'Работ напрямую: ':'Работ: ')+group.members+(group.children?' · Подгрупп: '+group.children:'')));
  details.append(summary,actions,memberRoot);
  const rename=button('Переименовать',async()=>{
   if(busy||!valid(expected))return;
   await guard(async()=>{
    const opened=U.generation;
    const name=await requestGroupName(rename,group);
    if(name===null)return;
    if(opened!==U.generation||!valid(expected))throw Error('База изменилась. Повторите действие.');
    await mutate(expected,()=>api.nativeGroupRename(expected,group.handle,name),'Группа переименована.');
    if(active&&valid(revision))await load(groups,groups.offset);
   })();
   if(rename.isConnected!==false&&!rename.disabled){rename.focus({preventScroll:true});return;}
   const current=Array.from(groups.root.querySelectorAll('button')).find(item=>item.dataset.groupMore===group.id);
   if(current){current.closest('details').open=true;current.focus({preventScroll:true});}else $('groupSearch').focus({preventScroll:true});
  });
  rename.dataset.groupRename=group.id;
  const edit=button('Изменить состав',()=>editGroup(group,expected,edit));
  const activeScope=()=>{
   const opened=U.generation,token=window.sharedWorkspace?.state?.workspace_token;
   return ()=>valid(expected)&&opened===U.generation&&token===window.sharedWorkspace?.state?.workspace_token;
  };
  const create=button('Создать подгруппу',guard(async ()=>{
   const current=activeScope(),name=await requestGroupName(create,group,true);
   if(name===null)return;
   if(!current())throw Error('База изменилась. Повторите действие.');
   await mutate(expected,()=>api.unifiedGroupTreeEdit(expected,{
    kind:'create',id:window.crypto.randomUUID(),name,parent:group.handle
   }),'Подгруппа создана.');
  }));
  const move=button('Переместить',guard(async ()=>{
   const current=activeScope();
   const parent=await requestGroupSelection(move,group,expected,current,true);
   if(parent===undefined||parent===(group.group||null))return;
   if(!current())throw Error('База изменилась. Повторите действие.');
   await mutate(expected,()=>api.unifiedGroupTreeEdit(expected,{kind:'move',group:group.handle,parent}),'Группа перемещена.');
  }));
  const remove=button('Удалить',guard(async ()=>{
   if(!valid(expected))return;
   const current=activeScope();
   let deleteDescendants=false;
   if(group.children){
    deleteDescendants=await confirmGroupTreeDeletion(remove,group);
    if(deleteDescendants===null)return;
   }else if(!await confirmAction(expected,'Удалить группу «'+group.name+'»? Исходные работы останутся.'))return;
   if(!current())throw Error('База изменилась. Повторите действие.');
   if(!hierarchy){
    await mutate(expected,()=>api.nativeSettingsEdit(expected,{kind:'delete_group',handle:group.handle}),'Группа удалена.');
    return;
   }
   let backup;
   if(!group.group&&(deleteDescendants||!group.children)){
    const roots=await api.unifiedManagementPage(expected,'group_tree','',null,0,1);
    if(!current()||!validManagementPage(roots,0)||!roots.total)throw Error('Не удалось проверить список групп.');
    if(roots.total===1){
     if(roots.rows[0].handle!==group.handle)throw Error('Список групп изменился. Повторите удаление.');
     if(!await confirmAction(expected,'Будут удалены все группы. Сначала будет создана проверенная резервная копия SQLite. Продолжить?'))return;
     if(!current())throw Error('База изменилась. Повторите действие.');
     backup=await api.chooseSqliteBackupPath('analiz-before-clear-groups.sqlite3');
     if(!backup)return;
    }
   }
   if(!current())throw Error('База изменилась. Повторите действие.');
   await mutate(expected,()=>api.unifiedGroupTreeEdit(expected,{
    kind:'delete',group:group.handle,delete_descendants:deleteDescendants,confirmed:true,...(backup?{backup}:{})
   }),'Группа удалена.');
  }));
  const extra=el('div',null,'group-extra-actions');
  const more=button('Ещё',()=>showExtra(extra.hidden));
  function showExtra(open){
   extra.hidden=!open;more.setAttribute('aria-expanded',String(open));
  }
  more.dataset.groupMore=group.id;showExtra(false);
  actions.addEventListener('keydown',event=>{
   if(event.key==='Escape'&&!extra.hidden){event.stopPropagation();showExtra(false);more.focus();}
  });
  extra.append(rename,move,remove);actions.append(edit,create,more,extra);
  const checkLabel=el('label',null,'group-selection'),check=el('input');
  check.type='checkbox';check.dataset.groupHandle=group.handle;check.setAttribute('aria-label','Выбрать «'+group.name+'»');
  check.addEventListener('change',()=>{
   if(busy||!valid(expected)||!selectingGroups)return;
   if(check.checked)addMarkedGroups([group.handle]);
   else markedGroups.delete(group.handle);
   updateRemovalControls();
  });
  checkLabel.append(check);summary.append(checkLabel);
  let openChildren;
  if(group.children){
   const subtree=el('div',null,depth<6?'group-children':null),status=el('p','','muted small');
   const pageTicket=groups.ticket;
   let offset=0,loading=false;
   const more=button('Показать ещё подгруппы',loadChildren);
   status.setAttribute('role','status');more.hidden=true;
   details.append(subtree,status,more);
   async function loadChildren(){
    const current=()=>pageTicket===groups.ticket&&valid(expected)&&details.isConnected!==false;
    if(!current()||loading||offset>=group.children)return;
    loading=true;more.hidden=true;status.textContent='Загрузка подгрупп…';
    try{
     const page=await api.unifiedManagementPage(expected,'group_tree','',group.handle,offset,LIMIT);
     if(!current())return;
     if(!validManagementPage(page,offset)||page.total!==group.children)throw Error('Не удалось проверить список подгрупп.');
     for(const row of page.rows)renderGroupRow(subtree,row,expected,hierarchy,depth+1);
     offset+=page.rows.length;status.textContent='';
    }catch(error){if(current())status.textContent=error?.message||String(error);}
    finally{loading=false;if(current()){more.hidden=offset>=group.children;updateRemovalControls();}}
   }
   openChildren=()=>{if(!offset)void loadChildren();};
  }
  expandMembers(details,memberRoot,group,expected,openChildren);
  root.append(details);
 }
 groups.render=(result,expected)=>{
  found.textContent=result.total?(result.hierarchy&&!groups.query().trim()?'Групп верхнего уровня: ':'Найдено групп: ')+result.total:
   groups.query().trim()?'По запросу групп не найдено':'Групп пока нет';
  for(const group of result.rows)renderGroupRow(groups.root,group,expected,result.hierarchy===true);
 };
 bind('createGroup',()=>{if(busy)return;showWorkMode('edit');resetEditor();openEditor($('createGroup'));$('groupName').focus();});bind('resetGroup',()=>{if(!busy)resetEditor();});bind('saveGroup',guard(async expected=>{const name=$('groupName').value.trim();if(!name||selected.size>5000)throw Error('Введите название и выберите не более 5000 работ.');const id=editing?.id||window.crypto.randomUUID();await mutate(expected,()=>api.unifiedGroupSave(expected,editing?.handle||null,id,name,[...selected]),'Группа сохранена.');}));
 bind('groupSearch',()=>load(groups),'input');bind('workSearch',()=>load(workMode==='audit'?audit:works),'input');
 const catalogRoot=el('div',null,'catalog-list'),catalogPanel=$('groupCatalog'),catalogControls=el('div',null,'catalog-controls');catalogRoot.id='catalogList';catalogControls.id='catalogControls';
 const catalogSearch=el('input'),searchLabel=el('label','Поиск в справочнике','catalog-field');catalogSearch.id='catalogSearch';catalogSearch.type='search';catalogSearch.placeholder='Поиск в справочнике';searchLabel.append(catalogSearch);catalogControls.append(searchLabel);
 const form=el('div',null,'catalog-form'),workInput=el('input'),orgInput=el('select'),catalogWorkLabel=el('label','Название услуги','catalog-field'),orgLabel=el('label','Подразделение','catalog-field');workInput.id='catalogWorkName';orgInput.id='catalogOrganization';workInput.placeholder='Название услуги';catalogWorkLabel.append(workInput);orgLabel.append(orgInput);let catalogEdit=null;
 const catalogFeedback=el('p',null,'catalog-feedback');catalogFeedback.id='catalogFeedback';catalogFeedback.hidden=true;catalogFeedback.setAttribute('role','status');catalogFeedback.setAttribute('aria-live','polite');workInput.setAttribute('aria-describedby',catalogFeedback.id);orgInput.setAttribute('aria-describedby',catalogFeedback.id);
 function clearCatalogFeedback(){catalogFeedback.hidden=true;catalogFeedback.textContent='';}
 function resetCatalogEditor(focus=false){catalogEdit=null;workInput.value='';orgInput.value='';clearCatalogFeedback();if(focus)workInput.focus();}
 const saveCatalog=button('Сохранить услугу',guard(async expected=>{
  clearCatalogFeedback();
  try{const work=workInput.value.trim();if(!work)throw Error('Введите название услуги.');const action={kind:catalogEdit?'update_catalog':'create_catalog',work,organization:orgInput.value||null};if(catalogEdit)action.handle=catalogEdit.handle;if(!await confirmAction(expected,'Сохранить услугу в справочнике?'))return;await mutate(expected,()=>api.nativeSettingsEdit(expected,action),'Справочник сохранён.');resetCatalogEditor();const refreshed=valid(revision);catalogFeedback.dataset.kind=refreshed?'success':'warning';catalogFeedback.textContent=refreshed?'Услуга сохранена.':'Услуга сохранена. Не удалось обновить список. Обновите данные.';catalogFeedback.hidden=false;}
  catch(error){catalogFeedback.textContent=window.analizUserError?window.analizUserError(error):(error?.message||String(error));catalogFeedback.dataset.kind='error';catalogFeedback.hidden=false;throw error;}
 }));saveCatalog.id='catalogSave';
 const resetCatalog=button('Сбросить',()=>{if(!busy)resetCatalogEditor(true);});resetCatalog.id='catalogReset';const formActions=el('div',null,'catalog-actions');formActions.append(saveCatalog,resetCatalog);form.append(catalogWorkLabel,orgLabel,formActions);catalogControls.append(form,catalogFeedback);catalogPanel.append(catalogControls,catalogRoot);
 for(const input of [catalogSearch,workInput,orgInput])controls.add(input);
 const catalog=pager(catalogPanel,'catalog');catalog.root=catalogRoot;catalog.kind='catalog';catalog.query=()=>catalogSearch.value;catalogSearch.addEventListener('input',()=>load(catalog));
 catalog.render=(result,expected)=>{ $('catalogStatus').textContent='Записей в справочнике: '+result.total;for(const row of result.rows){const node=el('div',null,'group-member catalog-row'),entry=el('div',null,'catalog-entry'),actions=el('div',null,'catalog-row-actions');entry.append(el('span',row.name,'catalog-name'),el('span',row.organization||'Без подразделения','catalog-organization'));actions.append(button('Изменить',()=>{if(!valid(expected)||busy)return;catalogEdit=row;clearCatalogFeedback();workInput.value=row.name;orgInput.value=row.organization||'';workInput.focus();}),button('Удалить',guard(async()=>{if(!valid(expected)||!await confirmAction(expected,'Удалить «'+row.name+'» из справочника?'))return;await mutate(expected,()=>api.nativeSettingsEdit(expected,{kind:'delete_catalog',handle:row.handle}),'Услуга удалена.');})));node.append(entry,actions);catalogRoot.append(node);}};
 catalogPanel.addEventListener('toggle',()=>{if(catalogPanel.open)void load(catalog);});
 function file(id,run){const n=$(id);if(!n)return;controls.add(n);n.addEventListener('click',event=>{event.preventDefault();return guard(run)();});}
 file('importGroups',async expected=>{const source=await api.chooseGroupsImport();if(!source)return;const review=await api.unifiedGroupsPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');const question='Заменить текущие группы? Групп: '+review.groups+', работ: '+review.works+'.'+(review.requires_backup?' Сначала будет создана проверенная копия SQLite.':'');if(!await confirmAction(expected,question))return;let backup;if(review.requires_backup){backup=await api.chooseSqliteBackupPath('analiz-before-group-import.sqlite3');if(!backup)return;}await mutate(expected,()=>review.requires_backup?api.unifiedGroupsImport(expected,source,review.hash,true,backup):api.unifiedGroupsImport(expected,source,review.hash,true),'Группы импортированы.');});
 bind('exportGroups',guard(async expected=>{const destination=await api.chooseGroupsExport();if(!destination)return;if(!valid(expected))throw Error('База изменилась. Повторите экспорт.');await api.unifiedGroupsExport(expected,destination);U.notice('Группы сохранены.');}));
 file('catalogFile',async expected=>{const source=await api.chooseXlsx();if(!source)return;const review=await api.unifiedCatalogPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');if(!await confirmAction(expected,'Добавить услуги из Excel? Найдено: '+review.catalog+'.'))return;await mutate(expected,()=>api.unifiedCatalogApply(expected,source,review.hash,true),'Справочник добавлен.');});
 function organizationFields(){const root=$('organizationFields');root.replaceChildren();orgInput.replaceChildren();const none=el('option','Без подразделения');none.value='';orgInput.append(none);for(const [i,name] of U.state.settings.orgNames.entries()){const label=el('label','Подразделение '+(i+1)),input=el('input');input.value=name;input.dataset.org=i;label.append(input);root.append(label);const option=el('option',name);option.value=name;orgInput.append(option);}}
 bind('saveOrganizations',guard(async expected=>{const names=Array.from($('organizationFields').querySelectorAll('input'),n=>n.value.trim());const normalize=s=>s.toLowerCase().replace(/[.,;:]+$/,'').replace(/\s+/g,' ');if(names.length!==4||names.some(n=>!n)||new Set(names.map(normalize)).size!==4)throw Error('Нужны четыре непустых разных названия подразделений.');if(!await confirmAction(expected,'Изменить названия подразделений во всех отчётах и справочнике? Сначала будет создана резервная копия SQLite.'))return;const destination=await api.chooseSqliteBackupPath('analiz-before-rename.sqlite3');if(!destination)return;await mutate(expected,()=>api.unifiedRenameOrganizations(expected,names,destination),'Названия сохранены. Резервная копия создана.');}));
 const clearReports=$('clearReports');
 function confirmClearReports(workspace,reports){
  const dialog=el('dialog'),heading=el('h2','Удалить все импортированные отчёты?');
  dialog.id='clearReportsDialog';heading.id='clearReportsHeading';dialog.setAttribute('aria-labelledby',heading.id);
  const header=el('div',null,'clear-reports-header'),close=el('button','×','button secondary');
  close.type='button';close.setAttribute('aria-label','Закрыть');header.append(heading,close);
  const shared=workspace.mode==='shared';
  const location=el('p',shared?'Общая база · '+workspace.folder:'Личная база на этом компьютере.');
  const details=el('p','Отчётов к удалению: '+reports+'. '+(shared?'Они будут удалены для всех пользователей общей базы. ':'')+'Группы останутся.');
  location.id='clearReportsLocation';details.id='clearReportsDetails';dialog.setAttribute('aria-describedby',location.id+' '+details.id);
  const actions=el('div',null,'actions'),cancel=el('button','Отмена','button secondary'),remove=el('button','Удалить отчёты','button danger-button');
  cancel.type='button';remove.type='button';cancel.autofocus=true;
  actions.append(cancel,remove);dialog.append(header,location,details,actions);
  const host=clearReports.closest('dialog[open]')||clearReports.closest('#refNative')||document.body;
  return new Promise((resolve,reject)=>{
   let accepted=false;
   close.addEventListener('click',()=>dialog.close());
   cancel.addEventListener('click',()=>dialog.close());
   remove.addEventListener('click',()=>{accepted=true;dialog.close();});
   dialog.addEventListener('cancel',event=>{event.preventDefault();dialog.close();});
   dialog.addEventListener('close',()=>{dialog.remove();resolve(accepted);},{once:true});
   host.append(dialog);
   try{dialog.showModal();cancel.focus();}catch(error){dialog.remove();reject(error);}
  });
 }
 bind('clearReports',async()=>{
  if(busy||!valid(revision))return;
  await guard(async expected=>{
   const snapshot=U.snapshot,workspace=window.sharedWorkspace?.state;
   if(!workspace?.available||workspace.pending||!snapshot||snapshot.revision!==expected||
      !Number.isSafeInteger(snapshot.reports)||snapshot.reports<0)throw Error('Не удалось проверить активную базу и число отчётов. Обновите данные.');
   if(snapshot.reports===0){U.notice('В этой базе нет импортированных отчётов.');return;}
   if(!await confirmClearReports(workspace,snapshot.reports))return;
   if(!valid(expected)||U.snapshot!==snapshot||window.sharedWorkspace.state?.workspace_token!==workspace.workspace_token)
    throw Error('База изменилась. Повторите действие.');
   await mutate(expected,()=>api.unifiedClearReports(expected),'Отчёты удалены.');
  })();
  if(!clearReports.disabled&&(!document.activeElement||document.activeElement===document.body))clearReports.focus({preventScroll:true});
 });
 clearReports.title='Удалить загруженные отчёты после подтверждения';
 const auditEmptyPeriod='Выберите корректный период в аналитике.';
 const audit=existingPager('groupAudit',works.root);audit.ready=()=>Boolean(U.scope);audit.query=works.query;
 audit.actions=el('div',null,'actions');$('groupAuditPanel').append(audit.actions);
 audit.render=(result,expected)=>{const unused=$('groupAuditMode').value==='unused';for(const row of result.rows){const status=row.active===true?'Есть выполнения':row.active===false?'Нет выполнений':'Недостаточно данных о выполнениях';if(unused){const node=el('div',null,'group-member');node.append(el('span',row.name+' · Работ: '+row.members),el('span',status,'muted small'),button('Открыть группу',()=>{if(!valid(expected))return;$('groupSearch').value=row.name;void load(groups); }));audit.root.append(node);}else{const node=moveControl(row,expected);node.append(el('span',(row.group_name?'Группа: '+row.group_name+' · ':'')+status,'muted small'));audit.root.append(node);}}if(result.total){
  const ticket=audit.ticket,save=button('Скачать CSV',async()=>{
   await guard(async()=>{
    if(!valid(expected)||ticket!==audit.ticket)return;
    const destination=await api.chooseCsvPath('analiz-group-audit.csv');
    if(!destination||!valid(expected)||ticket!==audit.ticket)return;
    await api.unifiedGroupAuditCsv(result.request,result.mode,result.search,destination);
    U.notice('Результаты проверки групп сохранены в CSV.');
   })();
   if(save.isConnected&&valid(expected)&&ticket===audit.ticket)save.focus();
  });save.id='groupAuditExportCsv';audit.actions.append(save);
 }const coverage=result.coverage;$('groupAuditCoverage').textContent='Найдено: '+result.total+(coverage?' · '+(coverage.complete?'Полные данные':'Неполные данные')+' · Периодов: '+coverage.periods+' · Пропусков подразделение–период: '+coverage.missing+'. Пропуски не считаются нулём.':'');};
 function auditRequest(){
  const range=U.range(U.scope.displayType,$('groupAuditFrom').value,$('groupAuditTo').value,U.scope.source);
  if(!range)throw Error('Укажите корректный диапазон проверки.');
  return U.request({...U.scope,...range,selectedNames:[],selectedKeys:[],allNames:true});
 }
 audit.fetch=async(expected,offset)=>{
  const request=auditRequest(),mode=$('groupAuditMode').value,search=workSearch.value;
  const page=await api.unifiedGroupAuditPage(request,mode,search,offset,LIMIT);
  return {...page,request,mode,search};
 };
 function showWorkMode(mode){
  if(mode===workMode)return false;
  clear(works);clear(audit);workMode=mode;
  if(mode==='audit'){
   editor.hidden=true;editor.open=false;editorOpener=null;workSearchCaption.textContent='Поиск по всем названиям';auditSearchHome.append(workLabel);auditListHome.append(works.root);
  }else{
   auditPanel.open=false;workSearchCaption.textContent='Поиск работ';workSearchHome.append(workLabel);workListHome.insertBefore(works.root,workListNext??null);
  }
  return true;
 }
 for(const id of ['groupAuditMode','groupAuditFrom','groupAuditTo'])bind(id,()=>{if(workMode==='audit')return load(audit);},'change');
 editor.addEventListener('toggle',()=>{
  if(!editor.open&&!editor.hidden){
   editor.hidden=true;
   const opener=editorOpener && editorOpener.isConnected!==false && !editorOpener.disabled ? editorOpener : $('createGroup');
   editorOpener=null;opener.focus({preventScroll:true});
  }
 });
 auditPanel.addEventListener('toggle',()=>{if(auditPanel.open){showWorkMode('audit');return load(audit);}});
 function readGroupTotals(expected){
  const generation=U.generation;
  return api.unifiedManagementPage(expected,'works','',null,0,1).then(result=>{
   if(valid(expected)&&generation===U.generation)showWorkTotals(result);
  },error=>{if(valid(expected)&&generation===U.generation)fail(error);});
 }
 U.onScope(scope=>{if(scope){U.setPeriodInput('groupAuditFrom',scope.displayType,scope.from);U.setPeriodInput('groupAuditTo',scope.displayType,scope.to);}
  $('groupAuditCoverage').textContent=scope?'':auditEmptyPeriod;
  if(workMode==='audit'){clear(audit);if(scope&&active&&auditPanel.open)void load(audit);}
 });
 U.onSnapshot(snapshot=>{markedGroups.clear();selectingGroups=false;revision=Number.isSafeInteger(snapshot?.revision)?snapshot.revision:null;editTicket++;editing=null;selected=new Set();resetCatalogEditor();for(const p of pages)clear(p);found.textContent='';coverage.textContent='';$('catalogStatus').textContent='';$('groupAuditCoverage').textContent=U.scope?'':auditEmptyPeriod;organizationFields();lock();if(active&&valid(revision)){void reloadGroups();void load(workMode==='audit'?audit:works);if(workMode==='audit'||workSearch.value.trim())void readGroupTotals(revision);if(catalogPanel.open)void load(catalog);}});
 U.onView('groups',()=>{active=true;if(valid(revision)){void reloadGroups(groups.offset);const page=workMode==='audit'?audit:works;void load(page,page.offset);if(!coverage.textContent&&(workMode==='audit'||workSearch.value.trim()))void readGroupTotals(revision);if(catalogPanel.open)void load(catalog);}});U.onView('storage',()=>{organizationFields();});
}
window.unifiedManagement={init};
})();
