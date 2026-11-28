/* Original groups and settings controls; all report history stays in SQLite. */
(function(){
'use strict';
let initialized=false;
function init(){
 if(initialized)return;initialized=true;
 const U=window.unifiedUI,{api,$,el}=U,LIMIT=50;
 let revision=null,busy=false,active=false,editing=null,selected=new Set(),editTicket=0,workMode='edit';
 const controls=new Set(),pages=new Set();
 let organizationDirty=false;
 const valid=r=>Number.isSafeInteger(r)&&r===revision&&r===U.revision;
 async function confirmAction(expected,message){const generation=U.generation,token=window.sharedWorkspace?.state?.workspace_token;
  const accepted=await window.confirm(message);if(!valid(expected)||generation!==U.generation||token!==window.sharedWorkspace?.state?.workspace_token)throw Error('База изменилась. Повторите действие.');return accepted===true;}
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
 function lock(){for(const n of controls){if(n.isConnected===false){controls.delete(n);continue;}n.disabled=busy||revision===null||n.dataset.needsSelection==='true';}for(const p of pages){p.prev.disabled=busy||revision===null||p.loading||!p.offset;p.next.disabled=busy||revision===null||p.loading||p.offset+LIMIT>=p.total;}updateRemovalControls();$('saveOrganizations').disabled=busy||revision===null||!organizationDirty;}
 function bind(id,run,event='click'){const n=$(id);if(!n)return;controls.add(n);n.addEventListener(event,run);}
 function guard(run){return async()=>{if(busy||!valid(revision))return;busy=true;lock();try{await run(revision);}catch(e){fail(e);}finally{busy=false;lock();}};}
 async function mutate(expected,run,message){if(!valid(expected))throw Error('База изменилась. Обновите данные.');const result=await run();resetEditor(false);revision=null;for(const p of pages)clear(p);try{await U.refresh();}catch(e){U.notice(message+' Не удалось обновить рабочую область. '+(e?.message||String(e)),true);return result;}U.notice(message+((result?.cleanup_warning||result?.cleanup_error)?' Не удалось удалить временные данные редактора.':''));return result;}
 function pager(root,prefix){const bar=el('div',null,'change-pagination'),prev=button('← Предыдущая',()=>load(p,p.offset-LIMIT)),label=el('span','','muted small'),next=button('Следующая →',()=>load(p,p.offset+LIMIT));bar.append(prev,label,next);window.sqlRowScroll?.retire(prev,next);root.append(bar);const p={prefix,root:null,prev,next,label,offset:0,total:0,ticket:0,loading:false,query:()=>'',kind:'works',group:()=>null,render:()=>{}};pages.add(p);return p;}
 function existingPager(prefix,root){const p={prefix,root,prev:$(prefix+'Prev'),next:$(prefix+'Next'),label:$(prefix+'Page'),offset:0,total:0,ticket:0,loading:false,query:()=>'',kind:'works',group:()=>null,render:()=>{}};window.sqlRowScroll?.retire(p.prev,p.next);p.prev.addEventListener('click',()=>load(p,p.offset-LIMIT));p.next.addEventListener('click',()=>load(p,p.offset+LIMIT));pages.add(p);return p;}
 function clear(p){p.actions?.replaceChildren();window.sqlRowScroll?.reset(p.root);++p.ticket;p.loading=false;p.total=0;p.root?.replaceChildren();p.label.textContent='';lock();}
 async function load(p,offset=0){if(p.ready&&!p.ready()){clear(p);$('groupAuditCoverage').textContent=auditEmptyPeriod;return;}const expected=revision;if(!valid(expected))return;const ticket=++p.ticket;p.actions?.replaceChildren();p.loading=true;p.offset=Math.max(0,offset);p.root.replaceChildren();p.label.textContent='Загрузка…';if(p.prefix==='catalog')$('catalogStatus').textContent='Загрузка справочника…';lock();try{
  const result=p.fetch?await p.fetch(expected,p.offset):await api.unifiedManagementPage(expected,p.kind,p.query(),p.group(),p.offset,LIMIT);
  if(ticket!==p.ticket||!valid(expected))return;
  if(!result||!Array.isArray(result.rows)||result.rows.length>LIMIT||!Number.isSafeInteger(result.total)||result.total<0)throw Error('Получена некорректная страница.');
  if(p.prefix==='catalog'&&p.offset>0&&p.offset>=result.total)return load(p,Math.floor(Math.max(0,result.total-1)/LIMIT)*LIMIT);
  p.total=result.total;p.render(result,expected);p.label.textContent='';
  const label={unifiedGroup:'Группы работ',work:'Работы',catalog:'Справочник',groupAudit:'Проверка групп'}[p.prefix];
  p.root.dataset.sqlScrollHost=p.prefix==='groupAudit'?'groupAuditResults':p.root.id||p.prefix;
  window.sqlRowScroll.attach(p.root,{index:Math.floor(p.offset/LIMIT),size:LIMIT,count:result.rows.length,total:p.total,label,controlsOnly:true,edgeLinks:p.prefix==='catalog',footer:p.label.parentNode,go:i=>{
   if(ticket!==p.ticket||!valid(expected)||busy)return false;
   return load(p,i*LIMIT);
  }});
 }catch(e){if(ticket===p.ticket&&valid(expected)){window.sqlRowScroll?.reset(p.root);p.root.replaceChildren();p.total=0;p.label.textContent='Не удалось загрузить данные.';if(p.prefix==='catalog')$('catalogStatus').textContent='Справочник недоступен.';fail(e);}}finally{if(ticket===p.ticket){p.loading=false;lock();}}}
 const groupPagination=el('div');$('groupList').parentNode.insertBefore(groupPagination,$('groupList').nextSibling);
 const groups=pager(groupPagination,'unifiedGroup');groups.root=$('groupList');groups.kind='groups';groups.query=()=>$('groupSearch').value;
 groups.fetch=(expected,offset,query=groups.query())=>api.unifiedManagementPage(expected,U.snapshot?.work_group_hierarchy?'group_tree':'groups',query,null,offset,LIMIT);
 const maxGroupWorks=10000;
 const markedGroups=new Set(),maxGroupSelection=10000;
 let selectingGroups=false;
 const removalBar=el('div',null,'actions group-removal');
 const selectionStatus=el('span','','muted small');
 selectionStatus.setAttribute('role','status');
 const selectionMode=button('Выбрать группы для удаления',()=>{
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
 const clearGroups=button('Удалить все группы…',guard(expected=>removeGroups(expected,null)));
 removeSelected.className=clearGroups.className='button danger-button';
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
  selectionMode.textContent=selectingGroups?'Завершить выбор':'Выбрать группы для удаления';
  selectionStatus.hidden=!selectingGroups;
  selectionStatus.textContent=markedGroups.size?'Выбрано групп: '+markedGroups.size:'Выберите хотя бы одну группу.';
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
  if(!await confirmAction(expected,location+'\n'+(all?'Очистить весь список групп? ':'')+'Удаляемых групп: '+count+'.'+treeNote+' В аналитике работы удаляемых групп снова будут видны под исходными названиями. Отчёты, справочник, количества и суммы сохранятся. Продолжить?'))return;
  if(!current())throw Error('Активная база изменилась. Повторите действие.');
  await mutate(expected,()=>api.unifiedGroupsDelete(expected,handles,count,null),
   all?'Список групп очищен.':'Группы удалены: '+count+'.');
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
 coverage.className=found.className='muted small';counts.append(found);groups.root.parentNode.insertBefore(coverage,removalBar);
 works.query=()=>workSearch.value;
 function count(){return el('p','Выбрано работ: '+selected.size,'muted small');}
 function showWorkTotals(result){if(Number.isSafeInteger(result?.grouped)&&Number.isSafeInteger(result.total))coverage.textContent='В группах: '+result.grouped+' из '+result.total+' уникальных названий работ';}
 works.render=result=>{works.root.append(count());for(const row of result.rows){const label=el('label',null,'work-row'),check=el('input');check.type='checkbox';check.checked=selected.has(row.name);check.disabled=busy;controls.add(check);check.addEventListener('change',()=>{if(busy)return;if(check.checked)selected.add(row.name);else selected.delete(row.name);works.root.children[0].textContent='Выбрано работ: '+selected.size;});label.append(check,el('span',row.name));works.root.append(label);}if(!workSearch.value.trim())showWorkTotals(result);};
 function resetEditor(reload=true){editTicket++;editing=null;selected=new Set();$('groupName').value='';if(workMode==='edit')workSearch.value='';works.offset=0;if(reload&&active&&workMode==='edit')void load(works);}
 async function editGroup(group,expected,opener){if(busy||!valid(expected))return;const ticket=++editTicket;busy=true;lock();try{
  const chosen=new Set();let offset=0,total=1;
  while(offset<total){const page=await api.unifiedManagementPage(expected,'members','',group.handle,offset,LIMIT);if(ticket!==editTicket||!valid(expected))return;if(!Array.isArray(page?.rows)||page.rows.length>LIMIT||!Number.isSafeInteger(page.total)||page.total>maxGroupWorks||page.total<0||(!page.rows.length&&offset<page.total))throw Error('Не удалось загрузить состав группы (лимит 10 000 работ).');total=page.total;for(const row of page.rows)chosen.add(row.name);offset+=page.rows.length;}
  if(ticket!==editTicket||!valid(expected))return;editing=group;selected=chosen;showWorkMode('edit');openEditor(opener);$('groupName').value=group.name;workSearch.value='';await load(works);$('groupName').focus();
 }catch(e){if(ticket===editTicket)fail(e);}finally{busy=false;lock();}}
 function requestGroupSelection(anchor,row,expected,current,movingGroup=false,assigning=false){
  const dialog=el('dialog',null,'group-picker'),heading=el('h2',movingGroup?'Переместить группу':assigning?'Назначить группу':'Перенести работу'),work=el('p',row.name,'muted small');
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
    if(offset===0){choices.replaceChildren();if(!assigning)choices.append(choice(null,movingGroup?'Верхний уровень':'Без группы'));more.hidden=true;}
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
 let draggedWork=null;
 function allowWorkDrag(node,row,expected){
  node.draggable=true;
  node.addEventListener('dragstart',event=>{
   if(busy||!valid(expected)){event.preventDefault();return;}
   draggedWork={row,revision:expected,generation:U.generation,token:window.sharedWorkspace?.state?.workspace_token};
   event.dataTransfer.effectAllowed='move';event.dataTransfer.setData('application/x-analiz-work','1');
  });
  node.addEventListener('dragend',()=>{draggedWork=null;});
 }
 function moveControl(row,expected,sourceGroup,compact=false){
  const box=el('div',null,'group-member');
  const pick=button('Переместить',async()=>{
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
  if(!compact){const name=el('span',row.name);allowWorkDrag(name,row,expected);box.append(name);}box.append(pick);return box;
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
  const actions=el('div',null,'actions group-heading-actions'),memberRoot=el('div',null,'group-members');
  details.dataset.groupId=group.id;details.dataset.groupHandle=group.handle;
  function acceptsWork(){
   return !busy&&valid(expected)&&draggedWork?.revision===expected&&draggedWork.generation===U.generation&&
    draggedWork.token===window.sharedWorkspace?.state?.workspace_token&&draggedWork.row.group!==group.handle;
  }
  summary.addEventListener('dragover',event=>{if(acceptsWork()){event.preventDefault();event.dataTransfer.dropEffect='move';}});
  summary.addEventListener('drop',event=>{
   if(!acceptsWork())return;
   event.preventDefault();event.stopPropagation();
   const work=draggedWork.row.name;draggedWork=null;
   return guard(async()=>{
    if(await confirmAction(expected,'Перенести «'+work+'» в группу «'+group.name+'» для всех подразделений?'))
     await mutate(expected,()=>api.unifiedWorkAssign(expected,work,group.handle),'Работа перенесена.');
   })();
  });
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
   await mutate(expected,()=>api.unifiedGroupTreeEdit(expected,{
    kind:'delete',group:group.handle,delete_descendants:deleteDescendants,confirmed:true
   }),'Группа удалена.');
  }));
  remove.className='button danger-button';
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
 function focusGroupSection(target){
  target.focus({preventScroll:true});
  target.scrollIntoView({block:'start'});
  const toolsHeader=$('toolsPage').firstElementChild;
  const sticky=window.getComputedStyle(toolsHeader).position==='sticky'?toolsHeader:$('sharedHeader');
  const overlap=sticky.getBoundingClientRect().bottom+8-target.getBoundingClientRect().top;
  if(overlap>0)window.scrollBy(0,-overlap);
 }
 async function openAuditGroup(row,expected){
  if(!valid(expected)||busy)return;
  const groupNode=()=>Array.from(groups.root.querySelectorAll('details')).find(item=>item.dataset.groupHandle===row.group);
  let target=groupNode();
  if(!target){
   // Search includes descendants, even when their parents are on other pages.
   const search=$('groupSearch'),name=row.group_name||row.name;
   search.value=name;
   for(let offset=0;;offset+=LIMIT){
    const ticket=groups.ticket+1;
    await load(groups,offset);
    if(!valid(expected)||ticket!==groups.ticket||search.value!==name||$('refNative')?.dataset.panel!=='groups')return;
    target=groupNode();
    if(target||offset+LIMIT>=groups.total)break;
   }
  }
  if(!target){U.notice('Группа не найдена. Обновите список.',true);return;}
  for(let item=target;item;item=item.parentElement?.closest('details'))item.open=true;
  focusGroupSection(target.firstElementChild);
 }
 groups.render=(result,expected)=>{
  found.textContent=result.total?(result.hierarchy&&!groups.query().trim()?'Групп верхнего уровня: ':'Найдено групп: ')+result.total:
   groups.query().trim()?'По запросу групп не найдено':'Групп пока нет';
  for(const group of result.rows)renderGroupRow(groups.root,group,expected,result.hierarchy===true);
 };
 bind('createGroup',()=>{if(busy)return;showWorkMode('edit');resetEditor();openEditor($('createGroup'));$('groupName').focus();});bind('resetGroup',()=>{if(!busy)resetEditor();});bind('saveGroup',guard(async expected=>{const name=$('groupName').value.trim();if(!name||selected.size>maxGroupWorks)throw Error('Введите название и выберите не более 10 000 работ.');const id=editing?.id||window.crypto.randomUUID();await mutate(expected,()=>api.unifiedGroupSave(expected,editing?.handle||null,id,name,[...selected]),'Группа сохранена.');}));
 bind('groupSearch',()=>load(groups),'input');bind('workSearch',()=>{if(workMode==='audit')auditSelected.clear();return load(workMode==='audit'?audit:works);},'input');
 const catalogRoot=el('div',null,'catalog-list'),catalogPanel=$('groupCatalog'),catalogControls=el('div',null,'catalog-controls');catalogRoot.id='catalogList';catalogControls.id='catalogControls';
 const catalogSearch=el('input'),searchLabel=el('label','Поиск в справочнике','catalog-field');catalogSearch.id='catalogSearch';catalogSearch.type='search';catalogSearch.placeholder='Поиск в справочнике';searchLabel.append(catalogSearch);catalogControls.append(searchLabel);
 const form=el('div',null,'catalog-form'),workInput=el('input'),orgInput=el('select'),catalogWorkLabel=el('label','Название услуги','catalog-field'),orgLabel=el('label','Подразделение','catalog-field');workInput.id='catalogWorkName';orgInput.id='catalogOrganization';workInput.placeholder='Название услуги';catalogWorkLabel.append(workInput);orgLabel.append(orgInput);let catalogEdit=null;
 const catalogFeedback=el('p',null,'catalog-feedback');catalogFeedback.id='catalogFeedback';catalogFeedback.hidden=true;catalogFeedback.setAttribute('role','status');catalogFeedback.setAttribute('aria-live','polite');workInput.setAttribute('aria-describedby',catalogFeedback.id);orgInput.setAttribute('aria-describedby',catalogFeedback.id);
 function clearCatalogFeedback(){catalogFeedback.hidden=true;catalogFeedback.textContent='';}
 const formHeading=el('strong'),addCatalog=button('Добавить услугу',()=>openCatalogEditor());
 addCatalog.id='catalogAdd';addCatalog.setAttribute('aria-controls','catalogForm');form.id='catalogForm';form.hidden=true;
 let catalogOrigin=null,catalogFocusRow=null;
 function resetCatalogEditor(){
  catalogEdit=null;catalogOrigin=null;workInput.value='';orgInput.value='';form.hidden=true;
  addCatalog.hidden=false;addCatalog.setAttribute('aria-expanded','false');clearCatalogFeedback();
 }
 function openCatalogEditor(row=null,origin=addCatalog){
  if(busy||!valid(revision))return;
  catalogEdit=row;catalogOrigin=origin;clearCatalogFeedback();form.hidden=false;addCatalog.hidden=true;
  workInput.value=row?.name||'';orgInput.value=row?.organization||'';
  if(row?.organization&&orgInput.value!==row.organization){
   const option=el('option',row.organization);option.value=row.organization;orgInput.append(option);orgInput.value=row.organization;
  }
  formHeading.textContent=row?'Редактирование услуги · '+(row.organization||'Без подразделения'):'Новая услуга';
  saveCatalog.textContent=row?'Сохранить изменения':'Добавить услугу';
  addCatalog.setAttribute('aria-expanded','true');workInput.focus();
 }
 const saveCatalog=button('Добавить услугу',async()=>{
  let focusAfter;
  await guard(async expected=>{
   clearCatalogFeedback();
   try{
    const work=workInput.value.trim(),organization=orgInput.value||null;
    if(!work)throw Error('Введите название услуги.');
    const action={kind:catalogEdit?'update_catalog':'create_catalog',work,organization};
    if(catalogEdit)action.handle=catalogEdit.handle;
    if(!await confirmAction(expected,'Сохранить услугу в справочнике?'))return;
    const offset=catalog.offset,token=window.sharedWorkspace?.state?.workspace_token;
    await mutate(expected,()=>api.nativeSettingsEdit(expected,action),'Справочник сохранён.');
    resetCatalogEditor();
    const refreshed=valid(revision)&&token===window.sharedWorkspace?.state?.workspace_token;
    if(refreshed&&catalogPanel.open){
     await load(catalog,offset);
     focusAfter=catalogFocusRow?.(work,organization)||addCatalog;
    }
    catalogFeedback.dataset.kind=refreshed?'success':'warning';
    catalogFeedback.textContent=refreshed?'Услуга сохранена.':'Услуга сохранена. Не удалось обновить список. Обновите данные.';catalogFeedback.hidden=false;
   }catch(error){
    catalogFeedback.textContent=window.analizUserError?window.analizUserError(error):(error?.message||String(error));
    catalogFeedback.dataset.kind='error';catalogFeedback.hidden=false;throw error;
   }
  })();
  if(focusAfter?.isConnected!==false&&focusAfter&&!focusAfter.disabled)focusAfter.focus({preventScroll:true});
 });saveCatalog.id='catalogSave';saveCatalog.className='button primary';
 const resetCatalog=button('Отменить',()=>{
  if(busy)return;
  const origin=catalogOrigin;resetCatalogEditor();(origin?.isConnected!==false&&origin?origin:addCatalog).focus({preventScroll:true});
 });
 resetCatalog.id='catalogReset';const formActions=el('div',null,'catalog-actions');
 formActions.append(saveCatalog,resetCatalog);form.append(formHeading,catalogWorkLabel,orgLabel,formActions);
 catalogControls.append(addCatalog,form,catalogFeedback);catalogPanel.append(catalogControls,catalogRoot);
 for(const input of [catalogSearch,workInput,orgInput])controls.add(input);
 const catalog=pager(catalogPanel,'catalog');catalog.root=catalogRoot;catalog.kind='catalog_services';catalog.query=()=>catalogSearch.value;
 catalogSearch.addEventListener('input',()=>load(catalog));
 function catalogActions(row,expected,organization){
  const menu=el('details',null,'catalog-menu'),trigger=el('summary','⋯'),actions=el('div',null,'catalog-row-actions');
  trigger.setAttribute('aria-label','Действия · '+row.name+' · '+organization);
  const edit=button('Изменить',()=>{if(!valid(expected)||busy)return;menu.open=false;openCatalogEditor(row,trigger);});
  const remove=button('Удалить',guard(async()=>{
   if(!valid(expected)||!await confirmAction(expected,'Удалить «'+row.name+'» из справочника? Подразделение: '+organization+'.'))return;
   const offset=catalog.offset,token=window.sharedWorkspace?.state?.workspace_token;
   await mutate(expected,()=>api.nativeSettingsEdit(expected,{kind:'delete_catalog',handle:row.handle}),'Услуга удалена.');
   if(valid(revision)&&token===window.sharedWorkspace?.state?.workspace_token&&catalogPanel.open)await load(catalog,offset);
  }));
  remove.className='button danger-button';actions.append(edit,remove);menu.append(trigger,actions);return {menu,trigger};
 }
 catalog.render=(result,expected)=>{
  const entries=[],services=new Map(),search=catalogSearch.value.trim();
  addCatalog.textContent=!result.total&&!search?'Добавить первую услугу':'Добавить услугу';
  $('catalogStatus').textContent=(search?'Найдено уникальных услуг: ':'Уникальных услуг в справочнике: ')+result.total;
  catalogFocusRow=(work,org)=>{
   const match=entries.find(entry=>entry.row.name===work&&(entry.row.organization||null)===org);
   if(match?.disclosure){match.disclosure.open=true;match.detailRow.hidden=false;}
   return match?.trigger||services.get(work);
  };
  if(!result.total){
   catalogRoot.append(el('p',search?'Совпадений нет':'Справочник пуст','muted small'));
   if(search)catalogRoot.append(button('Сбросить поиск',()=>{catalogSearch.value='';catalogSearch.focus();return load(catalog);}));
   return;
  }
  const table=el('table',null,'catalog-table'),head=el('thead'),titles=el('tr'),body=el('tbody');
  for(const title of ['Услуга','Подразделение','Действия']){const th=el('th',title);th.setAttribute('scope','col');titles.append(th);}
  head.append(titles);table.append(head,body);catalogRoot.append(table);
  for(const row of result.rows){
   const node=el('tr',null,'catalog-row'),scope=el('td',null,'catalog-organization'),cell=el('td');
   node.append(el('td',row.name,'catalog-name'),scope,cell);body.append(node);
   const variants=row.variants||[],orgName=v=>v.organization||'Без подразделения';
   const departments=U.state.settings.orgNames;
   const badges=el('div',null,'catalog-badges');
   const names=new Set(variants.map(orgName));
   for(const name of [...departments,...[...names].filter(name=>!departments.includes(name)).sort((a,b)=>a.localeCompare(b,'ru'))]){
    if(!names.delete(name))continue;
    const badge=el('span',name,'catalog-badge');
    badge.dataset.department=String(departments.indexOf(name));
    badges.append(badge);
   }
   if(row.members>variants.length)badges.append(el('span','…'));
   scope.append(badges);
   function addAction(container,variant,disclosure,detailRow){
    const entry={...variant,name:row.name},action=catalogActions(entry,expected,orgName(entry));
    entries.push({row:entry,trigger:action.trigger,disclosure,detailRow});container.append(action.menu);
   }
   if(row.members===1&&variants.length===1){addAction(cell,variants[0]);continue;}
   const disclosure=el('details'),summary=el('summary','Записи ('+row.members+')'),list=el('div');
   summary.className='catalog-records';
   const detailRow=el('tr'),detailCell=el('td');detailCell.colSpan=3;detailRow.hidden=true;
   detailCell.append(list);detailRow.append(detailCell);body.append(detailRow);
   disclosure.append(summary);cell.append(disclosure);services.set(row.name,summary);
   disclosure.addEventListener('toggle',()=>{detailRow.hidden=!disclosure.open;});
   let cursor=0,fetching=false;
   const more=button('Ещё записи',async()=>{
    if(fetching||busy||!valid(expected))return;
    fetching=true;more.disabled=true;
    try{
     const page=await api.unifiedManagementPage(expected,'catalog_variants','',row.handle,cursor,LIMIT);
     if(!valid(expected)||detailRow.isConnected===false)return;
     if(!validManagementPage(page,cursor)||page.total!==row.members)throw Error('Состав услуги изменился. Обновите список.');
     appendVariants(page.rows);
    }catch(error){fail(error);}finally{fetching=false;more.disabled=false;}
   });
   function appendVariants(batch){
    for(const variant of batch){
     const line=el('div',null,'catalog-variant');line.append(el('span',orgName(variant)));addAction(line,variant,disclosure,detailRow);list.append(line);
    }
    cursor+=batch.length;more.hidden=cursor>=row.members;list.append(more);
   }
   appendVariants(variants);

  }
 };
 catalogPanel.addEventListener('toggle',()=>{if(catalogPanel.open)void load(catalog,catalog.offset);});
 function file(id,run){const n=$(id);if(!n)return;controls.add(n);n.addEventListener('click',event=>{event.preventDefault();return guard(run)();});}
 file('importGroups',async expected=>{const source=await api.chooseGroupsImport();if(!source)return;const review=await api.unifiedGroupsPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');const question='Заменить текущие группы? Групп: '+review.groups+', работ: '+review.works+'.'+(review.requires_backup?' Сначала будет создана проверенная копия SQLite.':'');if(!await confirmAction(expected,question))return;let backup;if(review.requires_backup){backup=await api.chooseSqliteBackupPath('analiz-before-group-import.sqlite3');if(!backup)return;}await mutate(expected,()=>review.requires_backup?api.unifiedGroupsImport(expected,source,review.hash,true,backup):api.unifiedGroupsImport(expected,source,review.hash,true),'Группы импортированы.');});
 bind('exportGroups',guard(async expected=>{const destination=await api.chooseGroupsExport();if(!destination)return;if(!valid(expected))throw Error('База изменилась. Повторите экспорт.');await api.unifiedGroupsExport(expected,destination);U.notice('Группы сохранены.');}));
 file('catalogFile',async expected=>{const source=await api.chooseXlsx();if(!source)return;const review=await api.unifiedCatalogPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');if(!await confirmAction(expected,'Добавить услуги из Excel? Найдено: '+review.catalog+'.'))return;await mutate(expected,()=>api.unifiedCatalogApply(expected,source,review.hash,true),'Справочник добавлен.');});
 function organizationDraftChanged(){
  const inputs=$('organizationFields').querySelectorAll('input');
  organizationDirty=Array.from(inputs).some((input,i)=>input.value.trim()!==U.state.settings.orgNames[i]);
  $('organizationSaveStatus').textContent=organizationDirty?'Есть несохранённые изменения':'';
  lock();
 }
 $('organizationFields').addEventListener('input',organizationDraftChanged);
 function organizationFields(){
  const root=$('organizationFields');root.replaceChildren();orgInput.replaceChildren();
  const none=el('option','Без подразделения');none.value='';orgInput.append(none);
  for(const [i,name] of U.state.settings.orgNames.entries()){
   const label=el('label','Подразделение '+(i+1)),input=el('input');
   input.value=name;input.dataset.org=i;label.append(input);root.append(label);
   controls.add(input);
   const option=el('option',name);option.value=name;orgInput.append(option);
  }
  organizationDraftChanged();
 }
 bind('saveOrganizations',guard(async expected=>{
  const names=Array.from($('organizationFields').querySelectorAll('input'),n=>n.value.trim());
  if(names.every((name,i)=>name===U.state.settings.orgNames[i]))return;
  const normalize=s=>s.toLowerCase().replace(/[.,;:]+$/,'').replace(/\s+/g,' ');
  if(names.length!==4||names.some(n=>!n)||new Set(names.map(normalize)).size!==4)throw Error('Нужны четыре непустых разных названия подразделений.');
  if(!await confirmAction(expected,'Изменить названия подразделений во всех отчётах и справочнике? Сначала будет создана резервная копия SQLite.'))return;
  const destination=await api.chooseSqliteBackupPath('analiz-before-rename.sqlite3');
  if(!destination)return;
  await mutate(expected,()=>api.unifiedRenameOrganizations(expected,names,destination),'Названия сохранены. Резервная копия создана.');
  $('organizationSaveStatus').textContent='Названия сохранены';
 }));
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
 const auditSelected=new Set(),activityNames={any:'Все состояния',active:'Есть выполнения',inactive:'Нет выполнений',unknown:'Недостаточно данных'};
 let activity='any';
 function auditMode(){const mode=$('groupAuditMode').value||'all';return mode==='unused'||activity==='any'?mode:mode+':'+activity;}
 function drawAuditToolbar(result,expected){
  const tools=el('div',null,'audit-tools'),states=el('div',null,'actions');
  const chooseState=(name,selected,count,run)=>{
   const control=button(name+' ('+count+')',run);control.setAttribute('aria-pressed',String(selected));if(count===0&&!selected)control.className+=' audit-zero';states.append(control);
  };
  const mode=$('groupAuditMode'),counts=result.counts;
  if(counts){
   chooseState('Без группы',mode.value==='ungrouped',counts.ungrouped,()=>{
    mode.value=mode.value==='ungrouped'?'all':'ungrouped';auditSelected.clear();return load(audit);
   });
   for(const [key,label] of Object.entries(activityNames)){
    chooseState(label,activity===key,counts[key==='any'?'all':key],()=>{activity=key;auditSelected.clear();return load(audit);});
   }
  }
  const selection=el('div',null,'actions'),status=el('span',null,'muted small');
  status.setAttribute('role','status');
  const assign=button('Назначить группу…',guard(async()=>{
   if(!auditSelected.size||!valid(expected))return;
   const generation=U.generation,token=window.sharedWorkspace?.state?.workspace_token,ticket=audit.ticket;
   const current=()=>valid(expected)&&generation===U.generation&&token===window.sharedWorkspace?.state?.workspace_token&&ticket===audit.ticket;
   const works=[...auditSelected],description={name:'Выбрано работ: '+works.length+'. Группа изменится во всех подразделениях.'};
   const target=await requestGroupSelection(assign,description,expected,current,false,true);
   if(target===undefined)return;
   if(!current())throw Error('База или выборка изменилась. Повторите назначение.');
   await mutate(expected,()=>api.unifiedWorksAssign(expected,works,target),'Группа назначена.');
  }));
  assign.setAttribute('aria-haspopup','dialog');
  function updateSelection(){
   assign.disabled=busy||!auditSelected.size||!valid(expected);assign.dataset.needsSelection=String(!auditSelected.size);
   status.textContent='Выбрано: '+auditSelected.size;
  }
  selection.append(assign,status);tools.append(states,selection);audit.root.append(tools);updateSelection();return updateSelection;
 }
 audit.render=(result,expected)=>{
  const unused=$('groupAuditMode').value==='unused';
  const updateSelection=unused?null:drawAuditToolbar(result,expected);
  const table=el('table',null,'audit-table'),head=el('thead'),titles=el('tr'),body=el('tbody');
  for(const text of (unused?['Группа','Работ','Статус','Действия']:['Работа','Группа','Статус','Действия'])){
   const th=el('th',text);th.setAttribute('scope','col');titles.append(th);
  }
  head.append(titles);table.append(head,body);audit.root.append(table);
  for(const row of result.rows){
   const line=el('tr'),name=el('td'),group=el('td',unused?String(row.members):(row.group_name||'Без группы'));
   if(!unused&&row.group&&row.group_name){
    const open=button(row.group_name,()=>openAuditGroup(row,expected));
    open.className='audit-group-link';
    open.setAttribute('aria-label','Открыть группу «'+row.group_name+'»');
    group.replaceChildren(open);
   }
   const state=row.active===true?'active':row.active===false?'inactive':'unknown';
   const status=el('td',activityNames[state]);status.dataset.activity=state;
   const actions=el('td'),menu=el('details',null,'catalog-menu'),trigger=el('summary','⋯');
   trigger.setAttribute('aria-label','Действия · '+row.name);menu.append(trigger);
   if(unused){
    name.textContent=row.name;menu.append(button('Открыть группу',()=>openAuditGroup(row,expected)));
   }else{
    const label=el('label'),check=el('input');check.type='checkbox';check.checked=auditSelected.has(row.name);controls.add(check);
    check.addEventListener('change',()=>{
     if(check.checked&&auditSelected.size>=500&&!auditSelected.has(row.name)){check.checked=false;fail(Error('Можно выбрать не более 500 работ.'));return;}
     if(check.checked)auditSelected.add(row.name);else auditSelected.delete(row.name);updateSelection();
    });
    const title=el('span',row.name);allowWorkDrag(title,row,expected);label.append(check,title);name.append(label);menu.append(moveControl(row,expected,undefined,true));
   }
   actions.append(menu);line.append(name,group,status,actions);body.append(line);
  }
  if(!result.rows.length)audit.root.append(el('p','Совпадений нет','muted small'));
  if(result.total){
  const ticket=audit.ticket,save=button('Скачать CSV',async()=>{
   await guard(async()=>{
    if(!valid(expected)||ticket!==audit.ticket)return;
    const destination=await api.chooseCsvPath('analiz-group-audit.csv');
    if(!destination||!valid(expected)||ticket!==audit.ticket)return;
    await api.unifiedGroupAuditCsv(result.request,result.mode,result.search,destination);
    U.notice('Результаты проверки групп сохранены в CSV.');
   })();
   if(save.isConnected&&valid(expected)&&ticket===audit.ticket)save.focus();
  });save.hidden=true;save.id='groupAuditExportCsv';audit.actions.append(save);
  }
 const coverage=result.coverage,summary=$('groupAuditCoverage');
 summary.replaceChildren(el('span','Найдено: '+result.total));
 if(coverage){
  const label=el('span',coverage.complete?'Полные данные':'Неполные данные','audit-coverage-label');
  if(!coverage.complete)label.title='Пропуски не считаются нулём.';
  summary.append(label,el('span','Периодов: '+coverage.periods),el('span','Пропусков подразделение–период: '+coverage.missing));
 }
 };
 function auditRequest(){
  const range=U.range(U.scope.displayType,$('groupAuditFrom').value,$('groupAuditTo').value,U.scope.source);
  if(!range)throw Error('Укажите корректный диапазон проверки.');
  return U.request({...U.scope,...range,selectedNames:[],selectedKeys:[],allNames:true});
 }
 audit.fetch=async(expected,offset)=>{
  const request=auditRequest(),mode=auditMode(),search=workSearch.value;
  const page=await api.unifiedGroupAuditPage(request,mode,search,offset,LIMIT);
  return {...page,request,mode,search};
 };
 function showWorkMode(mode){
  if(mode===workMode)return false;
  clear(works);clear(audit);workMode=mode;
  if(mode==='audit'){
   editor.hidden=true;editor.open=false;editorOpener=null;workSearchCaption.textContent='Отбор названий · поиск';auditSearchHome.append(workLabel);auditListHome.append(works.root);
  }else{
   auditPanel.open=false;workSearchCaption.textContent='Поиск работ';workSearchHome.append(workLabel);workListHome.insertBefore(works.root,workListNext??null);
  }
  return true;
 }
 for(const id of ['groupAuditMode','groupAuditFrom','groupAuditTo'])bind(id,()=>{auditSelected.clear();if(workMode==='audit')return load(audit);},'change');
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
 U.onScope(scope=>{auditSelected.clear();
  const type={day:'День',week:'Неделя',month:'Месяц',year:'Год'};
  $('groupAuditScope').textContent=scope?(type[scope.displayType]||scope.displayType)+' · '+scope.organizations.join(', '):'';
  if(scope){U.setPeriodInput('groupAuditFrom',scope.displayType,scope.from);U.setPeriodInput('groupAuditTo',scope.displayType,scope.to);}
  $('groupAuditCoverage').textContent=scope?'':auditEmptyPeriod;
  if(workMode==='audit'){clear(audit);if(scope&&active&&auditPanel.open)void load(audit);}
 });
 U.onSnapshot(snapshot=>{auditSelected.clear();markedGroups.clear();selectingGroups=false;revision=Number.isSafeInteger(snapshot?.revision)?snapshot.revision:null;editTicket++;editing=null;selected=new Set();resetCatalogEditor();for(const p of pages)clear(p);found.textContent='';coverage.textContent='';$('catalogStatus').textContent='';$('groupAuditCoverage').textContent=U.scope?'':auditEmptyPeriod;organizationFields();lock();if(active&&valid(revision)){void reloadGroups();void load(workMode==='audit'?audit:works);if(workMode==='audit'||workSearch.value.trim())void readGroupTotals(revision);if(catalogPanel.open)void load(catalog);}});
 U.onView('groups',()=>{active=true;if(valid(revision)){void reloadGroups(groups.offset);const page=workMode==='audit'?audit:works;void load(page,page.offset);if(!coverage.textContent&&(workMode==='audit'||workSearch.value.trim()))void readGroupTotals(revision);if(catalogPanel.open)void load(catalog);}});U.onView('storage',()=>{organizationFields();});
}
window.unifiedManagement={init};
})();
