'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
class Node{
 constructor(tag='div',text=''){this.tagName=tag;this.textContent=text??'';this.children=[];this.events={};this.value='';this.dataset={};this.open=false;this.disabled=false;}
 append(...nodes){for(const n of nodes){if(n.parentNode)n.parentNode.children=n.parentNode.children.filter(child=>child!==n);n.parentNode=this;this.children.push(n);}}
 insertBefore(node,before){if(node.parentNode)node.parentNode.children=node.parentNode.children.filter(child=>child!==node);node.parentNode=this;const i=this.children.indexOf(before);if(i<0)this.children.push(node);else this.children.splice(i,0,node);}
 replaceChildren(...nodes){this.children=[];this.append(...nodes);}
 addEventListener(type,fn){this.events[type]=fn;}
 querySelectorAll(tag){return this.children.flatMap(n=>[...(n.tagName===tag?[n]:[]),...n.querySelectorAll(tag)]);}
 setAttribute(key,value){this[key]=value;}
 closest(selector){let node=this;while(node){if(selector==='dialog[open]'&&node.tagName==='dialog'&&node.open||selector==='details'&&node.tagName==='details')return node;node=node.parentNode;}return null;}
 showModal(){this.open=true;}
 close(){this.open=false;this.fire('close');}
 remove(){if(this.parentNode)this.parentNode.children=this.parentNode.children.filter(n=>n!==this);this.parentNode=null;this.isConnected=false;}
 focus(){this.focused=true;}
 select(){this.selected=true;}
 fire(type='click',event={}){return this.events[type]?.({preventDefault(){},stopPropagation(){},...event});}
}
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function mount(){
 const html=fs.readFileSync(path.resolve(__dirname,'../../index.html'),'utf8'),nodes={};for(const [,id]of html.matchAll(/id="([^"]+)"/g))nodes[id]=new Node();
 const body=new Node(),groupsParent=new Node(),backupPanel=new Node(),backupActions=new Node(),backupLabel=new Node();body.append(groupsParent,backupPanel);groupsParent.append(nodes.groupList,nodes.groupSearchCount,nodes.groupCoverage,nodes.groupEditor,nodes.groupAuditPanel);
 const editListParent=new Node(),editSearchParent=new Node(),auditFilters=new Node(),auditSearchLabel=new Node('label'),searchLabel=new Node('label');
 nodes.groupEditor.append(editSearchParent,editListParent);editSearchParent.append(searchLabel);nodes.workSearch.tagName='input';searchLabel.append(nodes.workSearch);editListParent.append(nodes.worksList);
 nodes.groupAuditPanel.append(auditFilters,nodes.groupAuditResults);auditFilters.append(auditSearchLabel);auditSearchLabel.append(nodes.groupAuditSearch);backupPanel.append(backupActions);backupActions.append(backupLabel);backupLabel.append(nodes.importBackup);
 const reads=[],writes=[],messages=[],questions=[],pickers=[],snapshots=[],views={},scopes=[];let confirm=true,prompt=null,backup=null,source=null,handler=null;
 const api={unifiedManagementPage:async(...args)=>{reads.push(args);if(handler)return handler(...args);const kind=args[1];return {rows:kind==='works'?[{name:'Работа '+args[4],group:'g1'}]:kind==='groups'?[{handle:'g1',id:'group-1',name:'Группа',members:1}]:kind==='members'?[{name:'Работа 0',group:'g1'}]:[],total:kind==='works'?101:1,grouped:1,ungrouped:100};},
  unifiedGroupSave:async(...args)=>{writes.push(['save',...args]);return {revision:8};},unifiedWorkAssign:async(...args)=>{writes.push(['assign',...args]);return {revision:8};},nativeSettingsEdit:async(...args)=>{writes.push(['settings',...args]);return {revision:8};},nativeGroupRename:async(...args)=>{writes.push(['rename',...args]);return {revision:8};},
  chooseSqliteBackupPath:async()=>{pickers.push('backup');return backup;},unifiedClearReports:async(...args)=>{writes.push(['clear',...args]);return {revision:8};},unifiedRenameOrganizations:async(...args)=>{writes.push(['org',...args]);return {revision:8};},chooseGroupsImport:async()=>source,unifiedGroupsPreview:async()=>({hash:'hash',groups:2,works:3}),unifiedGroupsImport:async(...args)=>{writes.push(['import',...args]);return {revision:8};},chooseXlsx:async()=>source,unifiedCatalogPreview:async()=>({hash:'catalog-hash',catalog:3}),unifiedCatalogApply:async(...args)=>{writes.push(['catalog',...args]);return {revision:8};},chooseRestoreBackupPath:async()=>source,nativeRestorePreview:async()=>({verification:'a'.repeat(64),reportCount:2}),chooseRestoreDestination:async()=>{pickers.push('restore');return backup;},nativeRestoreCopy:async(...args)=>{writes.push(['copy',...args]);return {verification:'a'.repeat(64),reportCount:2};},nativeRestoreSchedule:async(...args)=>{writes.push(['schedule',...args]);return {restartRequired:true,reportCount:2};},nativeRestoreCancel:async()=>{writes.push(['cancel']);return true;},nativeBackupSqlite:async(...args)=>{writes.push(['backup',...args]);return 7;}};
 const UI={api,$:id=>nodes[id],el:(tag,text,cls)=>Object.assign(new Node(tag,text),{className:cls}),revision:null,state:{reports:[],settings:{orgNames:['А','Б','В','Г']}},notice:(...x)=>messages.push(x),onSnapshot:fn=>snapshots.push(fn),onView:(view,fn)=>views[view]=fn,onScope:fn=>scopes.push(fn),setPeriodInput:(id,kind,date)=>{nodes[id].value=date;},range:(kind,from,to,source)=>({displayType:kind,from,to,source}),scope:{revision:7,displayType:'day',source:'daily',organizations:['А'],selectedNames:['Only selected'],from:'2024-01-01',to:'2024-01-31'},request:scope=>scope,refresh:async()=>snapshot(8)};
 const sharedWorkspace={state:{mode:'personal',workspace_token:'personal',available:true,pending:false}};
 const window={sharedWorkspace,sqlRowScroll:require('./scroll-window-fixture.cjs'),unifiedUI:UI,confirm:text=>{questions.push(text);return confirm;},prompt:(...args)=>{questions.push(args[0]);return prompt;},crypto:{randomUUID:()=> 'new-id'}};
 function snapshot(revision,reports=12,work_group_hierarchy=false,groups){UI.revision=revision;UI.snapshot={revision,reports,work_group_hierarchy,groups};for(const fn of snapshots)fn(UI.snapshot);}
 vm.runInNewContext(fs.readFileSync(path.resolve(__dirname,'../unified-management.js'),'utf8'),{window,document:{body}});window.unifiedManagement.init();window.unifiedManagement.init();
 return {nodes,body,backupPanel,UI,sharedWorkspace,api,reads,writes,messages,questions,pickers,snapshot,views,scopes,setConfirm:v=>confirm=v,setPrompt:v=>prompt=v,setBackup:v=>backup=v,setSource:v=>source=v,setHandler:fn=>handler=fn};
}
test('no snapshot causes no reads or writes, and search/pagination stay on server',async()=>{const m=mount();await m.nodes.saveGroup.fire();assert.equal(m.writes.length,0);assert.equal(m.reads.length,0);m.snapshot(7);m.views.groups();await tick();m.nodes.workSearch.value='вся база';await m.nodes.workSearch.fire('input');assert.deepEqual(m.reads.at(-1),[7,'works','вся база',null,0,50]);await m.nodes.worksList.scrollWindow.go(1);assert.deepEqual(m.reads.at(-1),[7,'works','вся база',null,50,50]);});
test('empty group list distinguishes initial setup from an unsuccessful search',async()=>{
 const m=mount();
 m.setHandler(()=>({rows:[],total:0,grouped:0,ungrouped:0}));
 m.snapshot(7);m.views.groups();await tick();
 assert.equal(m.nodes.groupSearchCount.textContent,'Групп пока нет');
 m.nodes.groupSearch.value='Ремонт';await m.nodes.groupSearch.fire('input');await tick();
 assert.equal(m.nodes.groupSearchCount.textContent,'По запросу групп не найдено');
});
test('catalog search and labelled editor precede the bounded list and keep SQL search on each page',async()=>{
 const m=mount(),panel=m.nodes.groupCatalog;
 const tools=panel.children.find(n=>n.id==='catalogControls'),list=panel.children.find(n=>n.id==='catalogList');
 assert.ok(panel.children.indexOf(tools)<panel.children.indexOf(list));
 const search=panel.querySelectorAll('input').find(n=>n.id==='catalogSearch');
 assert.equal(search.parentNode.tagName,'label');
 assert.equal(panel.querySelectorAll('input').find(n=>n.id==='catalogWorkName').parentNode.tagName,'label');
 m.setHandler((rev,kind,query,group,offset)=>({rows:kind==='catalog'?[{handle:'c1',name:'Работа '+offset,organization:'А'}]:[],total:kind==='catalog'?101:0}));
 m.snapshot(7);panel.open=true;await panel.fire('toggle');await tick();search.value='Точное название';await search.fire('input');await tick();
 assert.deepEqual(m.reads.at(-1),[7,'catalog','Точное название',null,0,50]);
 await list.scrollWindow.go(2);assert.deepEqual(m.reads.at(-1),[7,'catalog','Точное название',null,100,50]);
});
test('catalog validation, cancelled confirmation and failed writes retain the draft; reset clears edit and organization',async()=>{
 const m=mount(),panel=m.nodes.groupCatalog;
 const work=panel.querySelectorAll('input').find(n=>n.id==='catalogWorkName'),org=panel.querySelectorAll('select')[0];
 const save=actionByText(panel,'Сохранить услугу'),reset=actionByText(panel,'Сбросить');
 const feedback=panel.querySelectorAll('p').find(n=>n.id==='catalogFeedback');
 m.snapshot(7);await save.fire();assert.equal(m.writes.length,0);assert.match(m.messages.at(-1)[0],/Введите название/);assert.equal(feedback.hidden,false);assert.match(feedback.textContent,/Введите название/);
 work.value='  Новая услуга  ';org.value='А';m.setConfirm(false);await save.fire();assert.equal(m.writes.length,0);assert.equal(work.value,'  Новая услуга  ');assert.equal(org.value,'А');
 m.setConfirm(true);const original=m.api.nativeSettingsEdit;m.api.nativeSettingsEdit=async()=>{throw Error('storage failure');};await save.fire();
 assert.equal(m.writes.length,0);assert.equal(work.value,'  Новая услуга  ');assert.equal(org.value,'А');assert.match(m.messages.at(-1)[0],/storage failure/);assert.equal(feedback.hidden,false);assert.match(feedback.textContent,/storage failure/);
 await reset.fire();assert.equal(work.value,'');assert.equal(org.value,'');assert.equal(work.focused,true);assert.equal(feedback.hidden,true);
 m.setHandler((rev,kind)=>({rows:kind==='catalog'?[{handle:'c1',name:'Existing',organization:'А'}]:[],total:kind==='catalog'?1:0}));
 panel.open=true;await panel.fire('toggle');await tick();await actionByText(panel,'Изменить').fire();assert.equal(work.value,'Existing');assert.equal(org.value,'А');
 await reset.fire();work.value='После сброса';org.value='Б';m.api.nativeSettingsEdit=original;await save.fire();
 assert.deepEqual(JSON.parse(JSON.stringify(m.writes.at(-1))),['settings',7,{kind:'create_catalog',work:'После сброса',organization:'Б'}]);assert.equal(work.value,'');assert.equal(org.value,'');assert.equal(feedback.hidden,false);assert.equal(feedback.dataset.kind,'success');assert.match(feedback.textContent,/Услуга сохранена/);
});
test('catalog name and organization cannot change during the asynchronous save confirmation',async()=>{
 const m=mount(),panel=m.nodes.groupCatalog; m.snapshot(7);
 const work=panel.querySelectorAll('input').find(n=>n.id==='catalogWorkName'),org=panel.querySelectorAll('select')[0];work.value='Черновик';org.value='А';
 const answer=deferred();m.setConfirm(answer.promise);const pending=actionByText(panel,'Сохранить услугу').fire();await tick();
 assert.equal(work.disabled,true);assert.equal(org.disabled,true);await actionByText(panel,'Сбросить').fire();assert.equal(work.value,'Черновик');
 answer.resolve(false);await pending;assert.equal(work.disabled,false);assert.equal(org.disabled,false);assert.equal(work.value,'Черновик');assert.equal(org.value,'А');assert.equal(m.writes.length,0);
});
test('catalog committed before refresh failure stays saved and requires refreshing before another write',async()=>{
 const m=mount(),panel=m.nodes.groupCatalog;m.snapshot(7);m.UI.refresh=async()=>{throw Error('refresh failure');};
 const work=panel.querySelectorAll('input').find(n=>n.id==='catalogWorkName'),save=actionByText(panel,'Сохранить услугу');work.value='Сохранённая услуга';await save.fire();
 const feedback=panel.querySelectorAll('p').find(n=>n.id==='catalogFeedback');assert.equal(m.writes.length,1);assert.match(feedback.textContent,/Услуга сохранена.*Обновите данные/);assert.equal(feedback.dataset.kind,'warning');assert.equal(save.disabled,true);
 await save.fire();assert.equal(m.writes.length,1);
});
test('selected works survive pages and search and save through one atomic API',async()=>{const m=mount();m.snapshot(7);m.views.groups();await tick();const check=m.nodes.worksList.children[1].children[0];check.checked=true;await check.fire('change');await m.nodes.worksList.scrollWindow.go(1);const other=m.nodes.worksList.children[1].children[0];other.checked=true;await other.fire('change');m.nodes.workSearch.value='unknown';await m.nodes.workSearch.fire('input');m.nodes.groupName.value='Новая';await m.nodes.saveGroup.fire();assert.deepEqual(JSON.parse(JSON.stringify(m.writes)),[['save',7,null,'new-id','Новая',['Работа 0','Работа 50']]]);});
test('deferred stale pages cannot resurrect rows after snapshot invalidation',async()=>{const m=mount();let resolve;m.setHandler((r,kind)=>kind==='works'?new Promise(done=>resolve=done):({rows:[],total:0}));m.snapshot(7);m.views.groups();await tick();m.snapshot(null);resolve({rows:[{name:'stale'}],total:1});await tick();assert.equal(m.nodes.worksList.children.length,0);assert.equal(m.nodes.workNext.disabled,true);});
test('moving a member uses the Group picker and cannot assign from a stale dialog',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');await tick();
 const assignment=actionByText(group.children[2],'Перенести…');
 const opening=assignment.fire();await tick();
 const picker=m.body.querySelectorAll('dialog').find(n=>n.id==='groupPickerDialog');
 assert.equal(picker.open,true);
 await actionByText(picker,'Без группы').fire();await opening;
 assert.deepEqual(m.writes[0],['assign',7,'Работа 0',null]);
 await assignment.fire();assert.equal(m.writes.length,1);
});
test('group picker returns focus on Escape and selecting the current group does not write',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();
 const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');
 const assignment=actionByText(group.children[2],'Перенести…');
 for(const action of ['cancel','escape','current']){
  assignment.focused=false;
  const opening=assignment.fire();await tick();
  const picker=m.body.querySelectorAll('dialog').find(n=>n.id==='groupPickerDialog');
  assert.equal(picker.querySelectorAll('input')[0].focused,true);
  if(action==='cancel')await picker.fire('cancel');
  else if(action==='escape')await picker.fire('keydown',{key:'Escape'});
  else await actionByText(picker,'Группа').fire();
  await opening;
  assert.equal(assignment.focused,true);
  assert.equal(m.writes.length,0);
 }
});
test('confirmed group move focuses the refreshed group or the group search if it disappeared',async()=>{
 for(const survives of [true,false]){
  const m=mount();m.snapshot(7);m.views.groups();await tick();
  const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');
  const assignment=actionByText(group.children[2],'Перенести…'),refresh=m.UI.refresh;
  m.UI.refresh=async()=>{
   assignment.isConnected=false;
   if(!survives)m.setHandler(()=>({rows:[],total:0,grouped:0}));
   await refresh();
  };
  const opening=assignment.fire();await tick();
  const picker=m.body.querySelectorAll('dialog').find(n=>n.id==='groupPickerDialog');
  await actionByText(picker,'Без группы').fire();await opening;
  assert.equal(m.writes.length,1);
  if(survives)assert.equal(m.nodes.groupList.children[0].children[0].focused,true);
  else assert.equal(m.nodes.groupSearch.focused,true);
 }
});
test('expanded groups load one member page on demand and reuse loaded rows when reopened',async()=>{
 const m=mount();
 m.setHandler((_rev,kind,_query,_group,offset,limit)=>{
  if(kind==='groups')return {total:1,rows:[{name:'Group',handle:'g0',id:'id0',members:101}]};
  if(kind==='members')return {total:101,rows:Array.from({length:Math.min(101-offset,limit)},(_,i)=>({name:'Work '+(offset+i),group:'g0'}))};
  return {total:210,grouped:101,ungrouped:109,rows:[{name:'Work 0'}]};
 });
 m.snapshot(7);m.views.groups();await tick();
 assert.equal(m.nodes.groupSearchCount.parentNode,m.nodes.groupCoverage.parentNode);
 assert.equal(m.nodes.groupSearchCount.className,m.nodes.groupCoverage.className);
 assert.match(m.nodes.groupCoverage.textContent,/101/);
 const group=m.nodes.groupList.children[0],members=group.children[2],before=m.reads.length;
 group.open=true;await group.fire('toggle');await tick();
 const rows=()=>members.children.filter(row=>row.className==='group-member');
 const more=actionByText(members,'Показать ещё работы');
 assert.deepEqual(m.reads.slice(before),[[7,'members','','g0',0,50]]);
 assert.equal(rows().length,50);
 assert.equal(more.hidden,false);
 group.open=false;await group.fire('toggle');group.open=true;await group.fire('toggle');
 assert.equal(m.reads.length,before+1);
 await Promise.all([more.fire(),more.fire()]);
 assert.equal(rows().length,100);
 await more.fire();
 assert.equal(more.hidden,true);
 assert.deepEqual(rows().map(row=>row.children[0].textContent),Array.from({length:101},(_,i)=>'Work '+i));
 assert.deepEqual(m.reads.slice(before),[[7,'members','','g0',0,50],[7,'members','','g0',50,50],[7,'members','','g0',100,50]]);
 assert.equal(members.children.filter(row=>row.className==='group-member').length,101);
 assert.equal(members.scrollWindow,undefined);
 assert.equal(members.querySelectorAll('button').filter(row=>row.textContent==='Перенести…').length,101);
 assert.equal(members.querySelectorAll('input').length,0);
});
test('group picker searches across SQLite pages and applies only an explicit choice',async()=>{
 const m=mount();m.setHandler((_rev,kind,query,_group,offset)=>{
  if(kind==='groups')return query==='Север'?{total:1,rows:[{name:'Север',handle:'g51',id:'id51',members:1}]}:{total:60,rows:Array.from({length:Math.min(50,60-offset)},(_,i)=>({name:'Group '+(offset+i),handle:'g'+(offset+i),id:'id'+(offset+i),members:1}))};
  return {total:1,grouped:1,ungrouped:0,rows:[{name:'A',group:'g0'}]};
 });
 m.snapshot(7);m.views.groups();await tick();const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');await tick();
 const opening=actionByText(group.children[2],'Перенести…').fire();await tick();
 const picker=m.body.querySelectorAll('dialog').find(n=>n.id==='groupPickerDialog');
 const search=picker.querySelectorAll('input')[0];
 await actionByText(picker,'Показать ещё').fire();
 assert.deepEqual(m.reads.at(-1),[7,'groups','',null,50,50]);
 search.value='Север';await search.fire('input');
 assert.deepEqual(m.reads.at(-1),[7,'groups','Север',null,0,50]);
 assert.equal(m.writes.length,0);
 await actionByText(picker,'Север').fire();await opening;
 assert.deepEqual(m.writes[0],['assign',7,'A','g51']);
});
test('editing and audit share one visible search and result list',async()=>{
 const m=mount();
 m.api.unifiedGroupAuditPage=async()=>({rows:[{key:'work:a',name:'Работа А',group:'g1',active:true}],total:1,coverage:{complete:true}});
 m.snapshot(7);m.views.groups();await tick();
 assert.equal(m.nodes.groupEditor.hidden,true);
 await m.nodes.createGroup.fire();await tick();
 assert.equal(m.nodes.groupEditor.hidden,false);
 assert.equal(m.nodes.groupEditor.open,true);
 m.nodes.groupAuditPanel.open=true;await m.nodes.groupAuditPanel.fire('toggle');await tick();
 assert.equal(m.nodes.groupEditor.open,false);
 assert.equal(m.nodes.groupEditor.hidden,true);
 assert.equal(m.nodes.worksList.parentNode,m.nodes.groupAuditResults);
 assert.ok(m.nodes.groupAuditPanel.querySelectorAll('input').includes(m.nodes.workSearch));
 assert.ok(!m.nodes.groupEditor.querySelectorAll('input').includes(m.nodes.workSearch));
 const opening=actionByText(m.nodes.worksList,'Перенести…').fire();await tick();
 const picker=m.body.querySelectorAll('dialog').find(n=>n.id==='groupPickerDialog');
 assert.ok(picker);
 await actionByText(picker,'Отмена').fire();await opening;
 await m.nodes.createGroup.fire();await tick();
 assert.equal(m.nodes.groupAuditPanel.open,false);
 assert.equal(m.nodes.groupEditor.hidden,false);
 assert.notEqual(m.nodes.worksList.parentNode,m.nodes.groupAuditResults);
 assert.ok(m.nodes.groupEditor.querySelectorAll('input').includes(m.nodes.workSearch));
 m.nodes.groupEditor.open=false;await m.nodes.groupEditor.fire('toggle');
 assert.equal(m.nodes.groupEditor.hidden,true);
 assert.equal(m.nodes.createGroup.focused,true);
 assert.equal(m.writes.length,0);
});
test('closing an existing group editor returns focus to its edit button',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();
 const edit=actionByText(m.nodes.groupList,'Изменить состав');
 await edit.fire();
 assert.equal(m.nodes.groupEditor.hidden,false);
 m.nodes.groupEditor.open=false;await m.nodes.groupEditor.fire('toggle');
 assert.equal(edit.focused,true);
});
test('failure to open a group editor leaves the active audit results intact',async()=>{
 const m=mount();
 m.api.unifiedGroupAuditPage=async()=>({rows:[{key:'work:a',name:'Проверенная работа',group:'g1',active:true}],total:101});
 m.snapshot(7);m.views.groups();await tick();
 m.nodes.groupAuditPanel.open=true;await m.nodes.groupAuditPanel.fire('toggle');
 const original=m.nodes.worksList.children[0],pager=m.nodes.worksList.scrollWindow;
 m.setHandler(()=>{throw Error('Не удалось прочитать состав');});
 await actionByText(m.nodes.groupList,'Изменить состав').fire();
 assert.equal(m.nodes.worksList.children[0],original);
 assert.equal(m.nodes.worksList.scrollWindow,pager);
 assert.equal(m.nodes.groupAuditPanel.open,true);
 assert.equal(m.nodes.groupEditor.open,false);
 assert.match(m.messages.at(-1)[0],/Не удалось прочитать состав/);
});
function clearDialog(m){return m.body.querySelectorAll('dialog').find(n=>n.id==='clearReportsDialog');}
async function confirmClear(m){
 const action=m.nodes.clearReports.fire();
 await actionByText(clearDialog(m),'Удалить отчёты').fire();
 await action;
}
test('clear dialog shows the selected personal database and actual whole-database count',async()=>{
 const m=mount();m.snapshot(7,15);
 const action=m.nodes.clearReports.fire(),dialog=clearDialog(m);
 assert.equal(dialog.open,true);
 assert.equal(actionByText(dialog,'Отмена').focused,true);
 assert.match(dialog.querySelectorAll('p').map(x=>x.textContent).join(' '),/Личная база.*15.*Группы останутся/);
 assert.equal(m.writes.length,0);
 await actionByText(dialog,'Отмена').fire();await action;
 assert.equal(m.writes.length,0);assert.equal(m.nodes.clearReports.focused,true);
 await confirmClear(m);assert.deepEqual(m.writes,[['clear',7]]);assert.deepEqual(m.pickers,[]);
});
test('organization form tracks edits, reverting, a failed write and a successful retry',async()=>{
 const m=mount();m.snapshot(7);
 const fields=m.nodes.organizationFields,save=m.nodes.saveOrganizations,status=m.nodes.organizationSaveStatus;
 const edit=value=>{fields.querySelectorAll('input')[0].value=value;fields.fire('input');};
 assert.equal(save.disabled,true);edit('АА');assert.equal(save.disabled,false);assert.equal(status.textContent,'Есть несохранённые изменения');
 edit('А');assert.equal(save.disabled,true);assert.equal(status.textContent,'');
 edit('АА');m.setBackup('/org-backup.sqlite3');
 m.api.unifiedRenameOrganizations=async()=>{throw Error('Нет доступа');};
 await save.fire();assert.equal(fields.querySelectorAll('input')[0].value,'АА');assert.equal(save.disabled,false);
 m.api.unifiedRenameOrganizations=async(_revision,names)=>{m.UI.state.settings.orgNames=names;};
 await save.fire();assert.equal(status.textContent,'Названия сохранены');assert.equal(save.disabled,true);
 m.views.storage();assert.equal(fields.querySelectorAll('input')[0].value,'АА');assert.equal(save.disabled,true);
});
test('organization rename validates exactly four names and backs up before native mutation',async()=>{const m=mount();m.snapshot(7);const inputs=m.nodes.organizationFields.querySelectorAll('input');inputs[0].value='Б';await m.nodes.saveOrganizations.fire();assert.equal(m.writes.length,0);assert.equal(m.messages.at(-1)[1],true);inputs[0].value='АА';m.setBackup('/org-backup.sqlite3');await m.nodes.saveOrganizations.fire();assert.deepEqual(JSON.parse(JSON.stringify(m.writes)),[['org',7,['АА','Б','В','Г'],'/org-backup.sqlite3']]);assert.equal(m.UI.state.reports.length,0);});
test('group file import is previewed and hash-confirmed; cancelled picker and denial never write',async()=>{const m=mount();m.snapshot(7);await m.nodes.importGroups.fire();assert.equal(m.writes.length,0);m.setSource('/groups.json');m.setConfirm(false);await m.nodes.importGroups.fire();assert.equal(m.writes.length,0);m.setConfirm(true);await m.nodes.importGroups.fire();assert.deepEqual(m.writes,[['import',7,'/groups.json','hash',true]]);});

test('audit uses whole-name SQL query, custom dates, scoped coverage and never moves unused group labels',async()=>{const m=mount(),calls=[];m.api.unifiedGroupAuditPage=async(...args)=>{calls.push(args);return {rows:[{key:'group:one',name:'Unused group',group:'9007199254740999',members:2,active:null}],total:1,coverage:{complete:false,periods:2,missing:1}};};m.snapshot(7);for(const fn of m.scopes)fn(m.UI.scope);m.nodes.groupAuditMode.value='unused';m.nodes.groupAuditFrom.value='2024-01-02';m.nodes.groupAuditTo.value='2024-01-20';m.nodes.workSearch.value='all search';m.nodes.groupAuditPanel.open=true;await m.nodes.groupAuditPanel.fire('toggle');await tick();assert.deepEqual(JSON.parse(JSON.stringify(calls[0])),[{revision:7,displayType:'day',source:'daily',organizations:['А'],selectedNames:[],selectedKeys:[],allNames:true,from:'2024-01-02',to:'2024-01-20'},'unused','all search',0,50]);assert.equal(m.nodes.groupAuditResults.querySelectorAll('select').length,0);assert.match(m.nodes.groupAuditCoverage.textContent,/Неполные данные/);});

test('successful mutation followed by refresh failure clears stale rows and prevents duplicate writes',async()=>{const m=mount();m.snapshot(7);m.views.groups();await tick();m.UI.refresh=async()=>{throw Error('read failure');};m.setBackup('/safe.sqlite3');await confirmClear(m);assert.equal(m.writes.length,1);assert.equal(m.nodes.groupList.children.length,0);await m.nodes.clearReports.fire();assert.equal(m.writes.length,1);assert.match(m.messages.at(-1)[0],/Отчёты удалены.*Не удалось обновить/);});

function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
function actionByText(root,text){const action=root.querySelectorAll('button').find(n=>n.textContent===text);assert.ok(action,'Missing action: '+text);return action;}
test('shared clear names scope and folder, cancellation, Escape and close never write',async()=>{
 const m=mount();m.snapshot(7,23);m.sharedWorkspace.state={mode:'shared',folder:'//server/reports',workspace_token:'shared-A',available:true,pending:false};
 for(const method of ['cancel','escape','close']){
  const action=m.nodes.clearReports.fire(),dialog=clearDialog(m),text=dialog.querySelectorAll('p').map(x=>x.textContent).join(' ');
  assert.match(text,/Общая база/);assert.match(text,/server\/reports/);assert.match(text,/23/);assert.match(text,/всех пользователей/);
  assert.equal(m.nodes.clearReports.disabled,true);await m.nodes.clearReports.fire();assert.equal(clearDialog(m),dialog);
  if(method==='escape')await dialog.fire('cancel');
  else if(method==='cancel')await actionByText(dialog,'Отмена').fire();
  else await dialog.querySelectorAll('button').find(b=>b['aria-label']==='Закрыть').fire();
  await action;
  assert.equal(m.nodes.clearReports.focused,true);assert.equal(m.nodes.clearReports.disabled,false);assert.equal(clearDialog(m),undefined);assert.equal(m.writes.length,0);
 }
 await confirmClear(m);assert.deepEqual(m.writes,[['clear',7]]);assert.deepEqual(m.pickers,[]);
});
test('clear rejects a stale snapshot and a switch to a different workspace with the same revision',async()=>{
 const m=mount();m.snapshot(7);
 let pending=m.nodes.clearReports.fire();m.snapshot(8);await actionByText(clearDialog(m),'Удалить отчёты').fire();await pending;
 assert.match(m.messages.at(-1)[0],/База изменилась/);assert.equal(m.writes.length,0);
 pending=m.nodes.clearReports.fire();m.sharedWorkspace.state={...m.sharedWorkspace.state,workspace_token:'other'};
 await actionByText(clearDialog(m),'Удалить отчёты').fire();await pending;
 assert.match(m.messages.at(-1)[0],/База изменилась/);assert.equal(m.writes.length,0);assert.equal(m.nodes.clearReports.disabled,false);
});
test('clear refuses unknown or empty counts and a dialog that fails to open',async()=>{
 const m=mount();
 for(const n of [null,0,NaN,-1]){m.snapshot(7,n);await m.nodes.clearReports.fire();assert.equal(clearDialog(m),undefined);assert.equal(m.writes.length,0);}
 m.snapshot(7);const original=Node.prototype.showModal;
 try{Node.prototype.showModal=()=>{throw Error('dialog unavailable');};await m.nodes.clearReports.fire();}finally{Node.prototype.showModal=original;}
 assert.match(m.messages.at(-1)[0],/dialog unavailable/);assert.equal(clearDialog(m),undefined);assert.equal(m.nodes.clearReports.disabled,false);assert.deepEqual(m.writes,[]);
});
test('group rename inside an open workspace discards Cancel and Escape, restores the trigger and reopens',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();
 const workspace=new Node('dialog');workspace.open=true;m.body.append(workspace);workspace.append(m.nodes.groupList.parentNode);
 const rename=actionByText(m.nodes.groupList,'Переименовать');
 const more=actionByText(m.nodes.groupList,'Ещё'),extra=rename.parentNode;
 assert.deepEqual(extra.children.map(action=>action.textContent),['Переименовать','Переместить','Удалить']);
 assert.equal(extra.hidden,true);await more.fire();assert.equal(extra.hidden,false);
 await extra.parentNode.fire('keydown',{key:'Escape'});assert.equal(extra.hidden,true);assert.equal(more.focused,true);
 await more.fire();
 assert.equal(rename.dataset.groupRename,'group-1');
 for(const method of ['cancel','escape']){
  rename.focused=false;const action=rename.fire();await tick();
  const dialog=workspace.querySelectorAll('dialog').find(n=>n.id==='groupRenameDialog');
  assert.equal(dialog.open,true);const input=dialog.querySelectorAll('input')[0];
  assert.equal(input.value,'Группа');assert.equal(input.focused,true);input.value='Черновик';
  if(method==='cancel')await actionByText(dialog,'Отмена').fire();else await dialog.fire('cancel');
  await action;assert.equal(dialog.isConnected,false);assert.equal(workspace.open,true);
  assert.equal(rename.focused,true);assert.equal(rename.disabled,false);assert.equal(m.writes.length,0);
 }
});
test('group rename submits a trimmed name once and rejects a stale revision before writing',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();
 let rename=actionByText(m.nodes.groupList,'Переименовать');const stale=rename.fire();await tick();
 let dialog=m.body.querySelectorAll('dialog')[0];dialog.querySelectorAll('input')[0].value='Устарело';
 m.snapshot(8);dialog.querySelectorAll('form')[0].fire('submit');await stale;
 assert.equal(m.writes.length,0);assert.match(m.messages.at(-1)[0],/База изменилась/);
 await tick();rename=actionByText(m.nodes.groupList,'Переименовать');const accepted=rename.fire();await tick();
 dialog=m.body.querySelectorAll('dialog')[0];const input=dialog.querySelectorAll('input')[0],form=dialog.querySelectorAll('form')[0];
 assert.equal(actionByText(dialog,'Сохранить').type,'submit');
 input.value='   ';form.fire('submit');assert.equal(dialog.open,true);assert.equal(m.writes.length,0);
 input.value=' New name ';form.fire('submit');await accepted;
 assert.deepEqual(m.writes,[['rename',8,'g1','New name']]);assert.equal(m.body.querySelectorAll('dialog').length,0);
});
test('all management destructive confirmations await native false promises',async()=>{
 for(const name of ['delete group','save catalog','delete catalog','import groups','import catalog','rename organizations']){
  const m=mount();m.snapshot(7);m.views.groups();await tick();m.setBackup('/safe.sqlite3');m.setSource('/source');
  let action;
  if(name==='delete group')action=actionByText(m.nodes.groupList,'Удалить');
  if(name==='save catalog'){const field=m.nodes.groupCatalog.querySelectorAll('input').find(n=>n.placeholder==='Название услуги');field.value='Service';action=actionByText(m.nodes.groupCatalog,'Сохранить услугу');}
  if(name==='delete catalog'){m.setHandler((r,kind)=>({rows:kind==='catalog'?[{handle:'c1',name:'Service',organization:'А'}]:[],total:kind==='catalog'?1:0}));m.nodes.groupCatalog.open=true;await m.nodes.groupCatalog.fire('toggle');await tick();action=actionByText(m.nodes.groupCatalog,'Удалить');}
  if(name==='import groups')action=m.nodes.importGroups;
  if(name==='import catalog')action=m.nodes.catalogFile;
  if(name==='rename organizations'){m.nodes.organizationFields.querySelectorAll('input')[0].value='Новое название';action=m.nodes.saveOrganizations;}
  const answer=deferred();m.setConfirm(answer.promise);const pending=action.fire();await tick();assert.equal(m.questions.length,1,name);assert.equal(m.writes.length,0,name+' wrote before answer');assert.equal(m.pickers.length,0,name+' opened destination before answer');answer.resolve(false);await pending;assert.equal(m.writes.length,0,name+' ignored denial');assert.equal(m.pickers.length,0,name+' opened destination after denial');
 }
});

test('audit export captures the filter for all pages and rejects a stale destination choice',async()=>{
 const m=mount(),writes=[];m.api.unifiedGroupAuditPage=async()=>({rows:[],total:64,coverage:{complete:false}});
 m.api.chooseCsvPath=async()=>'/tmp/audit.csv';m.api.unifiedGroupAuditCsv=async(...args)=>writes.push(args);
 m.snapshot(7);for(const fn of m.scopes)fn(m.UI.scope);
 m.nodes.workSearch.value='игла';m.nodes.groupAuditMode.value='unused';
 m.nodes.groupAuditPanel.open=true;await m.nodes.groupAuditPanel.fire('toggle');await tick();
 await actionByText(m.nodes.groupAuditPanel,'Скачать CSV').fire();
 assert.equal(writes[0][0].from,m.UI.scope.from);assert.deepEqual(writes[0].slice(1),['unused','игла','/tmp/audit.csv']);
 const picker=deferred();m.api.chooseCsvPath=()=>picker.promise;
 const opened=actionByText(m.nodes.groupAuditPanel,'Скачать CSV').fire();
 m.snapshot(8);picker.resolve('/tmp/stale.csv');await opened;assert.equal(writes.length,1);
});

test('an open audit without a valid period explains the selection and never requests SQL or CSV',async()=>{
 const m=mount();let reads=0;m.api.unifiedGroupAuditPage=async()=>{reads++;throw Error('should not read');};
 m.UI.scope=null;m.snapshot(7);for(const fn of m.scopes)fn(null);m.views.groups();m.nodes.groupAuditPanel.open=true;
 await m.nodes.groupAuditPanel.fire('toggle');await m.nodes.workSearch.fire('input');await tick();
 assert.equal(reads,0);assert.equal(m.nodes.groupAuditCoverage.textContent,'Выберите корректный период в аналитике.');
 assert.equal(m.nodes.groupAuditPage.textContent,'');assert.equal(m.nodes.groupAuditPrev.disabled,true);
 assert.equal(m.nodes.groupAuditNext.disabled,true);assert.equal(m.nodes.worksList.children.length,0);
 assert.equal(m.messages.length,0);
});

test('bulk selection spans search pages, and deletion confirms the active workspace', async()=>{
 const m=mount();
 const groups=Array.from({length:52},(_,i)=>({handle:String(i+1),id:'g'+i,name:'Ремонт '+i,members:0}));
 groups.push({handle:'53',id:'other',name:'Монтаж',members:0});
 m.setHandler((revision,kind,query,group,offset,limit)=>{
  const rows=kind==='groups'?groups.filter(item=>item.name.includes(query)):[];
  return {rows:rows.slice(offset,offset+limit),total:rows.length,grouped:0};
 });
 m.api.unifiedGroupsDelete=async(...args)=>{m.writes.push(['groups',...JSON.parse(JSON.stringify(args))]);return {revision:8};};
 m.sharedWorkspace.state={mode:'shared',folder:'//team/data',workspace_token:'shared',available:true};
 m.snapshot(7);m.views.groups();await tick();
 await actionByText(m.body,'Выбрать группы').fire();
 m.nodes.groupSearch.value='Ремонт';await m.nodes.groupSearch.fire('input');
 await actionByText(m.body,'Выбрать все найденные (52)').fire();
 await m.nodes.groupList.scrollWindow.go(1);
 assert.equal(m.nodes.groupList.querySelectorAll('input').length,2);
 assert.ok(m.nodes.groupList.querySelectorAll('input').every(item=>item.checked));
 m.setConfirm(false);await actionByText(m.body,'Удалить выбранные').fire();
 assert.equal(m.writes.length,0);
 assert.match(m.questions.at(-1),/Общая база · \/\/team\/data.*52/s);
 m.setConfirm(true);await actionByText(m.body,'Удалить выбранные').fire();
 assert.deepEqual(m.writes,[['groups',7,groups.slice(0,52).map(item=>item.handle),52,null]]);
});

test('clear all counts the entire workspace and skips the backup picker after confirmation', async()=>{
 const m=mount();
 m.api.unifiedGroupsDelete=async(...args)=>{m.writes.push(args);return {revision:8};};
 m.snapshot(7);m.views.groups();await tick();
 m.nodes.groupSearch.value='поиск';await m.nodes.groupSearch.fire('input');
 await actionByText(m.body,'Очистить список групп').fire();
 assert.equal(m.pickers.length,0);
 assert.equal(m.questions.length,1);
 assert.match(m.questions[0],/Личная база.*Очистить весь список групп.*Удаляемых групп: 1/s);
 assert.deepEqual(m.writes,[[7,null,1,null]]);
});

test('work group tree requests nested pages and offers explicit choices for descendants',async()=>{
 const m=mount();
 m.setHandler((revision,kind,query,parent,offset)=>{
  const rows=kind==='group_tree'?(parent?[{handle:'child',id:'child-id',name:'Подгруппа',children:0,members:0,group:'root'}]:
   [{handle:'root',id:'root-id',name:'Родитель',children:1,members:0}]):[];
  return {rows,total:rows.length,hierarchy:true};
 });
 m.api.unifiedGroupTreeEdit=async(...args)=>{m.writes.push(JSON.parse(JSON.stringify(args)));return {revision:8};};
 m.snapshot(7,12,true);m.views.groups();await tick();
 const parent=m.nodes.groupList.children[0];
 parent.open=true;await parent.fire('toggle');await tick();
 assert.ok(m.reads.some(args=>args[1]==='group_tree'&&args[3]==='root'));
 let pending=actionByText(parent,'Удалить').fire();
 const dialog=m.body.querySelectorAll('dialog').find(d=>d.id==='groupTreeDeletion');
 await actionByText(dialog,'Отмена').fire();await pending;
 assert.equal(m.writes.length,0);
 pending=actionByText(parent,'Удалить').fire();
 const next=m.body.querySelectorAll('dialog').find(d=>d.id==='groupTreeDeletion');
 await actionByText(next,'Перенести подгруппы на уровень выше').fire();await pending;
 assert.deepEqual(m.writes,[[7,{kind:'delete',group:'root',delete_descendants:false,confirmed:true}]]);
});

test('tree bulk picking includes expanded children and uses the whole tree for clear-all',async()=>{
 const m=mount(),root={handle:'r',id:'root',name:'Родитель',members:0,children:1};
 const child={handle:'c',id:'child',name:'Подгруппа',members:0,children:0,group:'r'};
 m.setHandler((revision,kind,query,parent,offset,limit)=>{
  const all=kind==='group_tree'?(parent?[child]:[root]):kind==='groups'?[root,child]:[];
  return {rows:all.slice(offset,offset+limit),total:all.length,hierarchy:kind==='group_tree'};
 });
 m.api.unifiedGroupsDelete=async(...args)=>{m.writes.push(JSON.parse(JSON.stringify(args)));return {revision:8};};
 m.snapshot(7,12,true,2);m.views.groups();await tick();
 const tools=m.nodes.groupList.parentNode;
 await actionByText(tools,'Выбрать группы').fire();
 await actionByText(tools,'Выбрать на странице').fire();
 assert.match(tools.querySelectorAll('span').find(node=>node.role==='status').textContent,/групп: 1/);
 const displayed=m.nodes.groupList.children[0];
 displayed.open=true;await displayed.fire('toggle');await tick();
 await actionByText(tools,'Выбрать на странице').fire();
 assert.match(tools.querySelectorAll('span').find(node=>node.role==='status').textContent,/групп: 2/);
 await actionByText(tools,'Снять выделение').fire();
 await actionByText(tools,'Выбрать все группы (2)').fire();
 await actionByText(tools,'Удалить выбранные').fire();
 assert.deepEqual(m.writes,[[7,['r','c'],2,null]]);
});

test('removing the sole root subtree uses the confirmed choice without opening a backup picker',async()=>{
 const m=mount(),root={handle:'r',id:'root',name:'Родитель',members:0,children:1};
 m.setHandler((revision,kind,query,parent)=>({rows:kind==='group_tree'&&!parent?[root]:[],total:kind==='group_tree'&&!parent?1:0,hierarchy:true}));
 m.api.unifiedGroupTreeEdit=async(...args)=>{m.writes.push(JSON.parse(JSON.stringify(args)));return {revision:8};};
 m.snapshot(7,12,true,2);m.views.groups();await tick();
 const remove=actionByText(m.nodes.groupList,'Удалить');
 async function attempt(){
  const pending=remove.fire();
  await actionByText(m.body.querySelectorAll('dialog').at(-1),'Удалить группу и подгруппы').fire();
  await pending;
 }
 await attempt();
 assert.equal(m.pickers.length,0);
 assert.deepEqual(m.writes,[[7,{kind:'delete',group:'r',delete_descendants:true,confirmed:true}]]);
});

test('import replacement requires a chosen SQLite backup when the preview requests it', async()=>{
 const m=mount();m.snapshot(7);m.setSource('/new-groups.json');
 m.api.unifiedGroupsPreview=async()=>({hash:'hash',groups:3,works:5,requires_backup:true});
 await m.nodes.importGroups.fire();
 assert.match(m.questions.at(-1),/проверенная копия SQLite/);
 assert.deepEqual(m.pickers,['backup']);
 assert.equal(m.writes.length,0);
 m.setBackup('/before.sqlite3');
 await m.nodes.importGroups.fire();
 assert.deepEqual(m.writes,[['import',7,'/new-groups.json','hash',true,'/before.sqlite3']]);
});
