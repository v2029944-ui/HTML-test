'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
class Node{
 constructor(tag='div',text=''){this.tagName=tag;this.textContent=text??'';this.children=[];this.events={};this.value='';this.dataset={};this.open=false;this.disabled=false;}
 append(...nodes){for(const n of nodes){n.parentNode=this;this.children.push(n);}}
 insertBefore(node,before){node.parentNode=this;const i=this.children.indexOf(before);if(i<0)this.children.push(node);else this.children.splice(i,0,node);}
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
 fire(type='click'){return this.events[type]?.({preventDefault(){}});}
}
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function mount(){
 const html=fs.readFileSync(path.resolve(__dirname,'../../index.html'),'utf8'),nodes={};for(const [,id]of html.matchAll(/id="([^"]+)"/g))nodes[id]=new Node();
 const body=new Node(),groupsParent=new Node(),backupPanel=new Node(),backupActions=new Node(),backupLabel=new Node();body.append(groupsParent,backupPanel);groupsParent.append(nodes.groupList);backupPanel.append(backupActions);backupActions.append(backupLabel);backupLabel.append(nodes.importBackup);
 const reads=[],writes=[],messages=[],questions=[],pickers=[],snapshots=[],views={},scopes=[];let confirm=true,prompt=null,backup=null,source=null,handler=null;
 const api={unifiedManagementPage:async(...args)=>{reads.push(args);if(handler)return handler(...args);const kind=args[1];return {rows:kind==='works'?[{name:'Работа '+args[4],group:'g1'}]:kind==='groups'?[{handle:'g1',id:'group-1',name:'Группа',members:1}]:kind==='members'?[{name:'Работа 0',group:'g1'}]:[],total:kind==='works'?101:1,grouped:1,ungrouped:100};},
  unifiedGroupSave:async(...args)=>{writes.push(['save',...args]);return {revision:8};},unifiedWorkAssign:async(...args)=>{writes.push(['assign',...args]);return {revision:8};},nativeSettingsEdit:async(...args)=>{writes.push(['settings',...args]);return {revision:8};},nativeGroupRename:async(...args)=>{writes.push(['rename',...args]);return {revision:8};},
  chooseSqliteBackupPath:async()=>{pickers.push('backup');return backup;},unifiedClearReports:async(...args)=>{writes.push(['clear',...args]);return {revision:8};},unifiedRenameOrganizations:async(...args)=>{writes.push(['org',...args]);return {revision:8};},chooseGroupsImport:async()=>source,unifiedGroupsPreview:async()=>({hash:'hash',groups:2,works:3}),unifiedGroupsImport:async(...args)=>{writes.push(['import',...args]);return {revision:8};},chooseXlsx:async()=>source,unifiedCatalogPreview:async()=>({hash:'catalog-hash',catalog:3}),unifiedCatalogApply:async(...args)=>{writes.push(['catalog',...args]);return {revision:8};},chooseRestoreBackupPath:async()=>source,nativeRestorePreview:async()=>({verification:'a'.repeat(64),reportCount:2}),chooseRestoreDestination:async()=>{pickers.push('restore');return backup;},nativeRestoreCopy:async(...args)=>{writes.push(['copy',...args]);return {verification:'a'.repeat(64),reportCount:2};},nativeRestoreSchedule:async(...args)=>{writes.push(['schedule',...args]);return {restartRequired:true,reportCount:2};},nativeRestoreCancel:async()=>{writes.push(['cancel']);return true;},nativeBackupSqlite:async(...args)=>{writes.push(['backup',...args]);return 7;}};
 const UI={api,$:id=>nodes[id],el:(tag,text)=>new Node(tag,text),revision:null,state:{reports:[],settings:{orgNames:['А','Б','В','Г']}},notice:(...x)=>messages.push(x),onSnapshot:fn=>snapshots.push(fn),onView:(view,fn)=>views[view]=fn,onScope:fn=>scopes.push(fn),setPeriodInput:(id,kind,date)=>{nodes[id].value=date;},range:(kind,from,to,source)=>({displayType:kind,from,to,source}),scope:{revision:7,displayType:'day',source:'daily',organizations:['А'],selectedNames:['Only selected'],from:'2024-01-01',to:'2024-01-31'},request:scope=>scope,refresh:async()=>snapshot(8)};
 const window={sqlRowScroll:require('./scroll-window-fixture.cjs'),unifiedUI:UI,confirm:text=>{questions.push(text);return confirm;},prompt:(...args)=>{questions.push(args[0]);return prompt;},crypto:{randomUUID:()=> 'new-id'}};
 function snapshot(revision){UI.revision=revision;for(const fn of snapshots)fn({revision});}
 vm.runInNewContext(fs.readFileSync(path.resolve(__dirname,'../unified-management.js'),'utf8'),{window,document:{body}});window.unifiedManagement.init();window.unifiedManagement.init();
 return {nodes,body,backupPanel,UI,api,reads,writes,messages,questions,pickers,snapshot,views,scopes,setConfirm:v=>confirm=v,setPrompt:v=>prompt=v,setBackup:v=>backup=v,setSource:v=>source=v,setHandler:fn=>handler=fn};
}
test('no snapshot causes no reads or writes, and search/pagination stay on server',async()=>{const m=mount();await m.nodes.saveGroup.fire();assert.equal(m.writes.length,0);assert.equal(m.reads.length,0);m.snapshot(7);m.views.groups();await tick();m.nodes.workSearch.value='вся база';await m.nodes.workSearch.fire('input');assert.deepEqual(m.reads.at(-1),[7,'works','вся база',null,0,50]);await m.nodes.worksList.scrollWindow.go(1);assert.deepEqual(m.reads.at(-1),[7,'works','вся база',null,50,50]);});
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
test('moving a member uses a single server assignment; stale member actions cannot write',async()=>{const m=mount();m.snapshot(7);m.views.groups();await tick();const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');await tick();const memberContainer=group.children[2].children[1],row=memberContainer.children[0],select=row.querySelectorAll('select')[0];select.value='';await select.fire('change');assert.deepEqual(m.writes[0],['assign',7,'Работа 0',null]);await select.fire('change');assert.equal(m.writes.length,1);});
test('member destination chooser jumps directly with unchanged selection and filter',async()=>{
 const m=mount();m.setHandler((_rev,kind,query,_group,offset)=>{
  if(kind==='groups')return {total:125,rows:[{name:'Group '+offset,handle:'g'+offset,id:'id'+offset,members:1}]};
  return {total:1,rows:[{name:'A',group:'g0'}]};
 });
 m.snapshot(7);m.views.groups();await tick();const group=m.nodes.groupList.children[0];group.open=true;await group.fire('toggle');await tick();
 const member=group.children[2].children[1].children[0],label=member.querySelectorAll('label').find(n=>n.dataset.sqlScrollHost==='group-destinations');
 const search=member.querySelectorAll('input')[0];search.value='Group';await actionByText(member,'Найти').fire();const before=m.reads.length;
 search.value='Другое имя, поиск ещё не запущен';
 await label.scrollWindow.go(2);
 assert.deepEqual(m.reads.slice(before),[[7,'groups','Group',null,100,50]]);
 assert.equal(label.children[0].value,'g0');assert.equal(m.writes.length,0);
});
test('clear requires confirmation and removes reports without choosing or creating a backup',async()=>{const m=mount();m.snapshot(7);m.setConfirm(false);await m.nodes.clearReports.fire();assert.equal(m.writes.length,0);m.setConfirm(true);await m.nodes.clearReports.fire();assert.deepEqual(m.writes,[['clear',7]]);assert.deepEqual(m.pickers,[]);assert.match(m.questions.at(-1),/Удалить все импортированные отчёты/);assert.doesNotMatch(m.questions.at(-1),/резервная копия/);});
test('organization rename validates exactly four names and backs up before native mutation',async()=>{const m=mount();m.snapshot(7);const inputs=m.nodes.organizationFields.querySelectorAll('input');inputs[0].value='Б';await m.nodes.saveOrganizations.fire();assert.equal(m.writes.length,0);assert.equal(m.messages.at(-1)[1],true);inputs[0].value='АА';m.setBackup('/org-backup.sqlite3');await m.nodes.saveOrganizations.fire();assert.deepEqual(JSON.parse(JSON.stringify(m.writes)),[['org',7,['АА','Б','В','Г'],'/org-backup.sqlite3']]);assert.equal(m.UI.state.reports.length,0);});
test('group file import is previewed and hash-confirmed; cancelled picker and denial never write',async()=>{const m=mount();m.snapshot(7);await m.nodes.importGroups.fire();assert.equal(m.writes.length,0);m.setSource('/groups.json');m.setConfirm(false);await m.nodes.importGroups.fire();assert.equal(m.writes.length,0);m.setConfirm(true);await m.nodes.importGroups.fire();assert.deepEqual(m.writes,[['import',7,'/groups.json','hash',true]]);});

test('backup verification remains read-only and does not render restore actions',async()=>{
 const m=mount();m.snapshot(7);
 const actions=()=>m.backupPanel.querySelectorAll('button').map(node=>node.textContent);
 for(const label of ['Создать отдельную восстановленную копию','Восстановить при следующем запуске','Отменить запланированное восстановление'])assert.equal(actions().includes(label),false);
 m.setSource('/backup.sqlite3');await m.nodes.importBackup.fire();
 assert.equal(m.writes.length,0);
 assert.match(m.backupPanel.children.find(n=>n.tagName==='p').textContent,/Копия проверена.*Рабочая база не изменена/);
 assert.equal(m.backupPanel.querySelectorAll('button').length,0);
});
test('backup verification reports invalid evidence without exposing a restore mutation',async()=>{
 const m=mount();m.snapshot(7);m.setSource('/bad.sqlite3');
 m.api.nativeRestorePreview=async()=>({verification:'invalid',reportCount:2});
 await m.nodes.importBackup.fire();
 assert.equal(m.writes.length,0);assert.match(m.messages.at(-1)[0],/целостность/);
 assert.equal(m.backupPanel.querySelectorAll('button').length,0);
});

test('audit uses whole-name SQL query, custom dates, scoped coverage and never moves unused group labels',async()=>{const m=mount(),calls=[];m.api.unifiedGroupAuditPage=async(...args)=>{calls.push(args);return {rows:[{key:'group:one',name:'Unused group',group:'9007199254740999',members:2,active:null}],total:1,coverage:{complete:false,periods:2,missing:1}};};m.snapshot(7);for(const fn of m.scopes)fn(m.UI.scope);m.nodes.groupAuditMode.value='unused';m.nodes.groupAuditFrom.value='2024-01-02';m.nodes.groupAuditTo.value='2024-01-20';m.nodes.groupAuditSearch.value='all search';m.nodes.groupAuditPanel.open=true;await m.nodes.groupAuditPanel.fire('toggle');await tick();assert.deepEqual(JSON.parse(JSON.stringify(calls[0])),[{revision:7,displayType:'day',source:'daily',organizations:['А'],selectedNames:[],selectedKeys:[],allNames:true,from:'2024-01-02',to:'2024-01-20'},'unused','all search',0,50]);assert.equal(m.nodes.groupAuditResults.querySelectorAll('select').length,0);assert.match(m.nodes.groupAuditCoverage.textContent,/Неполные данные/);});

test('successful mutation followed by refresh failure clears stale rows and prevents duplicate writes',async()=>{const m=mount();m.snapshot(7);m.views.groups();await tick();m.UI.refresh=async()=>{throw Error('read failure');};m.setBackup('/safe.sqlite3');await m.nodes.clearReports.fire();assert.equal(m.writes.length,1);assert.equal(m.nodes.groupList.children.length,0);await m.nodes.clearReports.fire();assert.equal(m.writes.length,1);assert.match(m.messages.at(-1)[0],/Отчёты удалены.*Не удалось обновить/);});

function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
function actionByText(root,text){const action=root.querySelectorAll('button').find(n=>n.textContent===text);assert.ok(action,'Missing action: '+text);return action;}
test('native async delete confirmation blocks writes, denial cancels and acceptance writes once without backup',async()=>{
 const m=mount();m.snapshot(7);m.setBackup('/safe.sqlite3');const denied=deferred();m.setConfirm(denied.promise);
 const pending=m.nodes.clearReports.fire();await tick();assert.equal(m.questions.length,1);assert.equal(m.pickers.length,0);assert.equal(m.writes.length,0);assert.equal(m.nodes.clearReports.disabled,true);
 await m.nodes.clearReports.fire();assert.equal(m.questions.length,1);denied.resolve(false);await pending;assert.equal(m.pickers.length,0);assert.equal(m.writes.length,0);
 const accepted=deferred();m.setConfirm(accepted.promise);const saved=m.nodes.clearReports.fire();await tick();assert.equal(m.pickers.length,0);accepted.resolve(true);await saved;assert.deepEqual(m.pickers,[]);assert.deepEqual(m.writes,[['clear',7]]);
});
test('confirmation rechecks revision after native modal and rejected dialogs surface an error',async()=>{
 const m=mount();m.snapshot(7);m.setBackup('/safe.sqlite3');const stale=deferred();m.setConfirm(stale.promise);const pending=m.nodes.clearReports.fire();await tick();m.snapshot(8);stale.resolve(true);await pending;assert.equal(m.pickers.length,0);assert.equal(m.writes.length,0);assert.match(m.messages.at(-1)[0],/База изменилась/);
 const failed=deferred();m.setConfirm(failed.promise);const rejection=m.nodes.clearReports.fire();await tick();failed.reject(Error('native dialog rejected'));await rejection;assert.equal(m.pickers.length,0);assert.equal(m.writes.length,0);assert.match(m.messages.at(-1)[0],/native dialog rejected/);assert.equal(m.messages.at(-1)[1],true);assert.equal(m.nodes.clearReports.disabled,false);
});
test('group rename inside an open workspace discards Cancel and Escape, restores the trigger and reopens',async()=>{
 const m=mount();m.snapshot(7);m.views.groups();await tick();
 const workspace=new Node('dialog');workspace.open=true;m.body.append(workspace);workspace.append(m.nodes.groupList.parentNode);
 const rename=actionByText(m.nodes.groupList,'Переименовать');
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
  if(name==='delete group')action=actionByText(m.nodes.groupList,'Удалить группу');
  if(name==='save catalog'){const field=m.nodes.groupCatalog.querySelectorAll('input').find(n=>n.placeholder==='Название услуги');field.value='Service';action=actionByText(m.nodes.groupCatalog,'Сохранить услугу');}
  if(name==='delete catalog'){m.setHandler((r,kind)=>({rows:kind==='catalog'?[{handle:'c1',name:'Service',organization:'А'}]:[],total:kind==='catalog'?1:0}));m.nodes.groupCatalog.open=true;await m.nodes.groupCatalog.fire('toggle');await tick();action=actionByText(m.nodes.groupCatalog,'Удалить');}
  if(name==='import groups')action=m.nodes.importGroups;
  if(name==='import catalog')action=m.nodes.catalogFile;
  if(name==='rename organizations')action=m.nodes.saveOrganizations;
  const answer=deferred();m.setConfirm(answer.promise);const pending=action.fire();await tick();assert.equal(m.questions.length,1,name);assert.equal(m.writes.length,0,name+' wrote before answer');assert.equal(m.pickers.length,0,name+' opened destination before answer');answer.resolve(false);await pending;assert.equal(m.writes.length,0,name+' ignored denial');assert.equal(m.pickers.length,0,name+' opened destination after denial');
 }
});
test('canceling backup verification selection retains the previous read-only result',async()=>{
 const m=mount();m.snapshot(7);m.setSource('/A.sqlite3');await m.nodes.importBackup.fire();
 const status=m.backupPanel.children.find(n=>n.tagName==='p').textContent;
 m.setSource(null);await m.nodes.importBackup.fire();
 assert.equal(m.backupPanel.children.find(n=>n.tagName==='p').textContent,status);
 assert.equal(m.writes.length,0);assert.equal(m.backupPanel.querySelectorAll('button').length,0);
});
test('audit export captures the filter for all pages and rejects a stale destination choice',async()=>{
 const m=mount(),writes=[];m.api.unifiedGroupAuditPage=async()=>({rows:[],total:64,coverage:{complete:false}});
 m.api.chooseCsvPath=async()=>'/tmp/audit.csv';m.api.unifiedGroupAuditCsv=async(...args)=>writes.push(args);
 m.snapshot(7);for(const fn of m.scopes)fn(m.UI.scope);
 m.nodes.groupAuditSearch.value='игла';m.nodes.groupAuditMode.value='unused';
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
 await m.nodes.groupAuditPanel.fire('toggle');await m.nodes.groupAuditSearch.fire('input');await tick();
 assert.equal(reads,0);assert.equal(m.nodes.groupAuditCoverage.textContent,'Выберите корректный период в аналитике.');
 assert.equal(m.nodes.groupAuditPage.textContent,'');assert.equal(m.nodes.groupAuditPrev.disabled,true);
 assert.equal(m.nodes.groupAuditNext.disabled,true);assert.equal(m.nodes.groupAuditResults.children.length,0);
 assert.equal(m.messages.length,0);
});
