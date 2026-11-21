/* Original groups and settings controls; all report history stays in SQLite. */
(function(){
'use strict';
let initialized=false;
function init(){
 if(initialized)return;initialized=true;
 const U=window.unifiedUI,{api,$,el}=U,LIMIT=50;
 let revision=null,busy=false,active=false,editing=null,selected=new Set(),restore=null,editTicket=0;
 const controls=new Set(),pages=new Set(),memberPages=new Set();
 const valid=r=>Number.isSafeInteger(r)&&r===revision&&r===U.revision;
 async function confirmAction(expected,message){const accepted=await window.confirm(message);if(!valid(expected))throw Error('База изменилась. Повторите действие.');return accepted===true;}
 function requestGroupName(anchor,group){
  const dialog=el('dialog'),form=el('form'),heading=el('h2','Переименовать группу');
  dialog.id='groupRenameDialog';heading.id='groupRenameTitle';dialog.setAttribute('aria-labelledby',heading.id);
  const label=el('label','Новое название группы'),input=el('input');
  input.id='groupRenameName';input.type='text';input.required=true;input.value=group.name;label.htmlFor=input.id;
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
 function lock(){for(const n of controls){if(n.isConnected===false){controls.delete(n);continue;}n.disabled=busy||revision===null;}for(const p of pages){p.prev.disabled=busy||revision===null||p.loading||!p.offset;p.next.disabled=busy||revision===null||p.loading||p.offset+LIMIT>=p.total;}}
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
  const label={unifiedGroup:'Группы работ',work:'Работы',catalog:'Справочник',members:'Работы группы',groupAudit:'Проверка групп'}[p.prefix];
  p.root.dataset.sqlScrollHost=p.root.id||p.prefix;
  window.sqlRowScroll.attach(p.root,{index:Math.floor(p.offset/LIMIT),size:LIMIT,count:result.rows.length,total:p.total,label,footer:p.label.parentNode,go:i=>{
   if(ticket!==p.ticket||!valid(expected)||busy)return false;
   return load(p,i*LIMIT);
  }});
 }catch(e){if(ticket===p.ticket&&valid(expected)){window.sqlRowScroll?.reset(p.root);p.root.replaceChildren();p.total=0;p.label.textContent='Не удалось загрузить данные.';fail(e);}}finally{if(ticket===p.ticket){p.loading=false;lock();}}}
 const groupPagination=el('div');$('groupList').parentNode.insertBefore(groupPagination,$('groupList').nextSibling);
 const groups=pager(groupPagination,'unifiedGroup');groups.root=$('groupList');groups.kind='groups';groups.query=()=>$('groupSearch').value;
 const works=existingPager('work',$('worksList'));works.query=()=>$('workSearch').value;
 function count(){return el('p','Выбрано работ: '+selected.size,'muted small');}
 works.render=result=>{works.root.append(count());for(const row of result.rows){const label=el('label',null,'work-row'),check=el('input');check.type='checkbox';check.checked=selected.has(row.name);check.disabled=busy;controls.add(check);check.addEventListener('change',()=>{if(busy)return;if(check.checked)selected.add(row.name);else selected.delete(row.name);works.root.children[0].textContent='Выбрано работ: '+selected.size;});label.append(check,el('span',row.name));works.root.append(label);} $('groupCoverage').textContent='Названий в группах: '+result.grouped+' из '+result.total+' · Без группы: '+result.ungrouped;};
 function resetEditor(reload=true){editTicket++;editing=null;selected=new Set();$('groupName').value='';$('workSearch').value='';works.offset=0;if(reload&&active)void load(works);}
 async function editGroup(group,expected){if(busy||!valid(expected))return;const ticket=++editTicket;busy=true;lock();selected=new Set();editing=null;works.root.replaceChildren();try{
  const chosen=new Set();let offset=0,total=1;
  while(offset<total){const page=await api.unifiedManagementPage(expected,'members','',group.handle,offset,LIMIT);if(ticket!==editTicket||!valid(expected))return;if(!Array.isArray(page?.rows)||page.rows.length>LIMIT||!Number.isSafeInteger(page.total)||page.total>5000||page.total<0||(!page.rows.length&&offset<page.total))throw Error('Не удалось загрузить состав группы (лимит 5000 работ).');total=page.total;for(const row of page.rows)chosen.add(row.name);offset+=page.rows.length;}
  if(ticket!==editTicket||!valid(expected))return;editing=group;selected=chosen;$('groupEditor').open=true;$('groupName').value=group.name;$('workSearch').value='';await load(works);$('groupName').focus();
 }catch(e){if(ticket===editTicket){resetEditor(false);fail(e);}}finally{busy=false;lock();}}
 function moveControl(row,expected){
  const box=el('div',null,'group-member'),search=el('input'),select=el('select'),label=el('label','Переместить в'),footer=el('div');
  search.type='search';search.placeholder='Найти группу назначения';label.dataset.sqlScrollHost='group-destinations';
  let ticket=0;
  async function targets(index=0,query=search.value.trim()){
   const t=++ticket;
   const current=()=>t===ticket&&valid(expected)&&box.isConnected!==false;
   select.disabled=true;
   try{
    const result=await api.unifiedManagementPage(expected,'groups',query,null,index*LIMIT,LIMIT);
    if(!current())return;
    const none=el('option','Без группы');none.value='';select.replaceChildren(none);
    for(const g of result.rows){const option=el('option',g.name);option.value=g.handle;select.append(option);}
    if(row.group&&!result.rows.some(g=>g.handle===row.group)){const same=el('option','Текущая группа');same.value=row.group;select.append(same);}
    select.value=row.group||'';select.disabled=busy;
    if(result.total>LIMIT)window.sqlRowScroll.attach(label,{index,size:LIMIT,total:result.total,count:result.rows.length,label:'Группы назначения',footer,go:i=>current()&&targets(i,query)});
    else window.sqlRowScroll.reset(label);
   }catch(error){if(current()){window.sqlRowScroll.reset(label);select.replaceChildren();fail(error);}}
  }
  const find=button('Найти',()=>targets());controls.add(select);
  select.addEventListener('change',guard(async r=>{if(r!==expected)return;await mutate(expected,()=>api.unifiedWorkAssign(expected,row.name,select.value||null),'Работа перенесена.');}));
  label.append(select);box.append(el('span',row.name),search,find,label,footer);void targets();return box;
 }
 groups.render=(result,expected)=>{for(const p of memberPages){clear(p);pages.delete(p);}memberPages.clear(); $('groupSearchCount').textContent='Найдено групп: '+result.total;for(const group of result.rows){const details=el('details',null,'group-details'),summary=el('summary',group.name),actions=el('div',null,'actions'),memberRoot=el('div',null,'group-members');summary.append(el('small','Работ: '+group.members));details.append(summary,actions,memberRoot);
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
   const current=Array.from(groups.root.querySelectorAll('button')).find(item=>item.dataset.groupRename===group.id);
   if(current){current.closest('details').open=true;current.focus({preventScroll:true});}else $('groupSearch').focus({preventScroll:true});
  });
  rename.dataset.groupRename=group.id;
  actions.append(button('Изменить группу',()=>editGroup(group,expected)),rename,button('Удалить группу',guard(async()=>{if(!valid(expected)||!await confirmAction(expected,'Удалить группу «'+group.name+'»? Исходные работы останутся.'))return;await mutate(expected,()=>api.nativeSettingsEdit(expected,{kind:'delete_group',handle:group.handle}),'Группа удалена.');})));
  let members=null;details.addEventListener('toggle',()=>{if(!details.open||!valid(expected))return;if(!members){members=pager(memberRoot,'members');memberPages.add(members);members.root=el('div');memberRoot.append(members.root);members.kind='members';members.group=()=>group.handle;members.render=r=>{for(const row of r.rows)members.root.append(moveControl(row,expected));};}void load(members,members.offset);});groups.root.append(details);
 }};
 bind('createGroup',()=>{if(busy)return;resetEditor();$('groupEditor').open=true;$('groupName').focus();});bind('resetGroup',()=>{if(!busy)resetEditor();});bind('saveGroup',guard(async expected=>{const name=$('groupName').value.trim();if(!name||!selected.size||selected.size>5000)throw Error('Введите название и выберите от 1 до 5000 работ.');const id=editing?.id||window.crypto.randomUUID();await mutate(expected,()=>api.unifiedGroupSave(expected,editing?.handle||null,id,name,[...selected]),'Группа сохранена.');}));
 bind('groupSearch',()=>load(groups),'input');bind('workSearch',()=>load(works),'input');
 const catalogRoot=el('div',null,'catalog-list'),catalogPanel=$('groupCatalog'),catalogControls=el('div',null,'catalog-controls');catalogRoot.id='catalogList';catalogControls.id='catalogControls';
 const catalogSearch=el('input'),searchLabel=el('label','Поиск в справочнике','catalog-field');catalogSearch.id='catalogSearch';catalogSearch.type='search';catalogSearch.placeholder='Поиск в справочнике';searchLabel.append(catalogSearch);catalogControls.append(searchLabel);
 const form=el('div',null,'catalog-form'),workInput=el('input'),orgInput=el('select'),workLabel=el('label','Название услуги','catalog-field'),orgLabel=el('label','Подразделение','catalog-field');workInput.id='catalogWorkName';orgInput.id='catalogOrganization';workInput.placeholder='Название услуги';workLabel.append(workInput);orgLabel.append(orgInput);let catalogEdit=null;
 const catalogFeedback=el('p',null,'catalog-feedback');catalogFeedback.id='catalogFeedback';catalogFeedback.hidden=true;catalogFeedback.setAttribute('role','status');catalogFeedback.setAttribute('aria-live','polite');workInput.setAttribute('aria-describedby',catalogFeedback.id);orgInput.setAttribute('aria-describedby',catalogFeedback.id);
 function clearCatalogFeedback(){catalogFeedback.hidden=true;catalogFeedback.textContent='';}
 function resetCatalogEditor(focus=false){catalogEdit=null;workInput.value='';orgInput.value='';clearCatalogFeedback();if(focus)workInput.focus();}
 const saveCatalog=button('Сохранить услугу',guard(async expected=>{
  clearCatalogFeedback();
  try{const work=workInput.value.trim();if(!work)throw Error('Введите название услуги.');const action={kind:catalogEdit?'update_catalog':'create_catalog',work,organization:orgInput.value||null};if(catalogEdit)action.handle=catalogEdit.handle;if(!await confirmAction(expected,'Сохранить услугу в справочнике?'))return;await mutate(expected,()=>api.nativeSettingsEdit(expected,action),'Справочник сохранён.');resetCatalogEditor();const refreshed=valid(revision);catalogFeedback.dataset.kind=refreshed?'success':'warning';catalogFeedback.textContent=refreshed?'Услуга сохранена.':'Услуга сохранена. Не удалось обновить список. Обновите данные.';catalogFeedback.hidden=false;}
  catch(error){catalogFeedback.textContent=window.analizUserError?window.analizUserError(error):(error?.message||String(error));catalogFeedback.dataset.kind='error';catalogFeedback.hidden=false;throw error;}
 }));saveCatalog.id='catalogSave';
 const resetCatalog=button('Сбросить',()=>{if(!busy)resetCatalogEditor(true);});resetCatalog.id='catalogReset';const formActions=el('div',null,'catalog-actions');formActions.append(saveCatalog,resetCatalog);form.append(workLabel,orgLabel,formActions);catalogControls.append(form,catalogFeedback);catalogPanel.append(catalogControls,catalogRoot);
 for(const input of [catalogSearch,workInput,orgInput])controls.add(input);
 const catalog=pager(catalogPanel,'catalog');catalog.root=catalogRoot;catalog.kind='catalog';catalog.query=()=>catalogSearch.value;catalogSearch.addEventListener('input',()=>load(catalog));
 catalog.render=(result,expected)=>{ $('catalogStatus').textContent='Записей в справочнике: '+result.total;for(const row of result.rows){const node=el('div',null,'group-member catalog-row'),entry=el('div',null,'catalog-entry'),actions=el('div',null,'catalog-row-actions');entry.append(el('span',row.name,'catalog-name'),el('span',row.organization||'Без подразделения','catalog-organization'));actions.append(button('Изменить',()=>{if(!valid(expected)||busy)return;catalogEdit=row;clearCatalogFeedback();workInput.value=row.name;orgInput.value=row.organization||'';workInput.focus();}),button('Удалить',guard(async()=>{if(!valid(expected)||!await confirmAction(expected,'Удалить «'+row.name+'» из справочника?'))return;await mutate(expected,()=>api.nativeSettingsEdit(expected,{kind:'delete_catalog',handle:row.handle}),'Услуга удалена.');})));node.append(entry,actions);catalogRoot.append(node);}};
 catalogPanel.addEventListener('toggle',()=>{if(catalogPanel.open)void load(catalog);});
 function file(id,run){const n=$(id);if(!n)return;controls.add(n);n.addEventListener('click',event=>{event.preventDefault();return guard(run)();});}
 file('importGroups',async expected=>{const source=await api.chooseGroupsImport();if(!source)return;const review=await api.unifiedGroupsPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');if(!await confirmAction(expected,'Заменить текущие группы? Групп: '+review.groups+', работ: '+review.works+'.'))return;await mutate(expected,()=>api.unifiedGroupsImport(expected,source,review.hash,true),'Группы импортированы.');});
 bind('exportGroups',guard(async expected=>{const destination=await api.chooseGroupsExport();if(!destination)return;if(!valid(expected))throw Error('База изменилась. Повторите экспорт.');await api.unifiedGroupsExport(expected,destination);U.notice('Группы сохранены.');}));
 file('catalogFile',async expected=>{const source=await api.chooseXlsx();if(!source)return;const review=await api.unifiedCatalogPreview(expected,source);if(!valid(expected))throw Error('База изменилась. Повторите импорт.');if(!await confirmAction(expected,'Добавить услуги из Excel? Найдено: '+review.catalog+'.'))return;await mutate(expected,()=>api.unifiedCatalogApply(expected,source,review.hash,true),'Справочник добавлен.');});
 function organizationFields(){const root=$('organizationFields');root.replaceChildren();orgInput.replaceChildren();const none=el('option','Без подразделения');none.value='';orgInput.append(none);for(const [i,name] of U.state.settings.orgNames.entries()){const label=el('label','Подразделение '+(i+1)),input=el('input');input.value=name;input.dataset.org=i;label.append(input);root.append(label);const option=el('option',name);option.value=name;orgInput.append(option);}}
 bind('saveOrganizations',guard(async expected=>{const names=Array.from($('organizationFields').querySelectorAll('input'),n=>n.value.trim());const normalize=s=>s.toLowerCase().replace(/[.,;:]+$/,'').replace(/\s+/g,' ');if(names.length!==4||names.some(n=>!n)||new Set(names.map(normalize)).size!==4)throw Error('Нужны четыре непустых разных названия подразделений.');if(!await confirmAction(expected,'Изменить названия подразделений во всех отчётах и справочнике? Сначала будет создана резервная копия SQLite.'))return;const destination=await api.chooseSqliteBackupPath('analiz-before-rename.sqlite3');if(!destination)return;await mutate(expected,()=>api.unifiedRenameOrganizations(expected,names,destination),'Названия сохранены. Резервная копия создана.');}));
 bind('clearReports',guard(async expected=>{const question=window.sharedWorkspace?.state?.mode==='shared'?'Удалить все импортированные отчёты из общей базы для всех пользователей? Группы останутся.':'Удалить все импортированные отчёты? Группы останутся.';if(!await confirmAction(expected,question))return;await mutate(expected,()=>api.unifiedClearReports(expected),'Отчёты удалены.');}));
 bind('exportBackup',guard(async()=>{const destination=await api.chooseSqliteBackupPath('analiz-backup.sqlite3');if(!destination)return;await api.nativeBackupSqlite(destination);U.notice('Проверенная резервная копия SQLite сохранена.');}));
 $('exportBackup').title='Сохранить проверенную резервную копию SQLite';$('clearReports').title='Удалить загруженные отчёты после подтверждения';$('importBackup').accept='.sqlite3';const backupPanel=$('importBackup').parentNode.parentNode.parentNode,restoreStatus=el('p','','muted small');backupPanel.append(restoreStatus);function restoreMessage(text){restoreStatus.textContent=text;}
 file('importBackup',async()=>{const source=await api.chooseRestoreBackupPath();if(!source){if(!restore)restoreMessage('Выбор отменён.');return;}restore=null;restoreMessage('Проверка резервной копии…');const preview=await api.nativeRestorePreview(source);if(!preview||!/^[a-f0-9]{64}$/i.test(preview.verification)||!Number.isSafeInteger(preview.reportCount)||preview.reportCount<0)throw Error('Не удалось подтвердить целостность резервной копии.');restore={source,verification:preview.verification};restoreMessage('Копия проверена. Отчётов: '+preview.reportCount+'. Рабочая база не изменена.');});
 const auditEmptyPeriod='Выберите корректный период в аналитике.';
 const audit=existingPager('groupAudit',$('groupAuditResults'));audit.ready=()=>Boolean(U.scope);audit.query=()=>$('groupAuditSearch').value;
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
  const request=auditRequest(),mode=$('groupAuditMode').value,search=$('groupAuditSearch').value;
  const page=await api.unifiedGroupAuditPage(request,mode,search,offset,LIMIT);
  return {...page,request,mode,search};
 };
 for(const id of ['groupAuditSearch','groupAuditMode','groupAuditFrom','groupAuditTo'])bind(id,()=>load(audit),id==='groupAuditSearch'?'input':'change');
 U.onScope(scope=>{if(scope){U.setPeriodInput('groupAuditFrom',scope.displayType,scope.from);U.setPeriodInput('groupAuditTo',scope.displayType,scope.to);}clear(audit);$('groupAuditCoverage').textContent=scope?'':auditEmptyPeriod;if(scope&&active&&$('groupAuditPanel').open)void load(audit);});$('groupAuditPanel').addEventListener('toggle',()=>{if($('groupAuditPanel').open)void load(audit);});
 U.onSnapshot(snapshot=>{revision=Number.isSafeInteger(snapshot?.revision)?snapshot.revision:null;editTicket++;editing=null;selected=new Set();resetCatalogEditor();for(const p of pages)clear(p);$('groupSearchCount').textContent='';$('groupCoverage').textContent='';$('catalogStatus').textContent='';$('groupAuditCoverage').textContent=U.scope?'':auditEmptyPeriod;organizationFields();lock();if(active&&valid(revision)){void load(groups);void load(works);if(catalogPanel.open)void load(catalog);if($('groupAuditPanel').open)void load(audit);}});
 U.onView('groups',()=>{active=true;if(valid(revision)){void load(groups,groups.offset);void load(works,works.offset);if(catalogPanel.open)void load(catalog);}});U.onView('storage',()=>{organizationFields();});
 $('storageModeInfo').textContent='Резервная копия активной базы включает отчёты, группы, справочник и настройки.';
 if(typeof api.nativeRestoreLaunchInfo==='function')void api.nativeRestoreLaunchInfo().then(result=>{if(result?.message)restoreMessage(result.message);if(result?.restartRequired)restoreMessage('Восстановление ожидает перезапуска приложения.');}).catch(fail);
 lock();
}
window.unifiedManagement={init};
})();
