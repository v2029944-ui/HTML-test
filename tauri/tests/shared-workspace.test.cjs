'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=name=>fs.readFileSync(path.join(__dirname,'..',name+'.js'),'utf8');
const personal={mode:'personal',personal_database_path:'fixture/private/reports.sqlite3',folder:null,generation:null,available:true,error:null,pending:false,workspace_token:'private'};
const team={...personal,mode:'shared',folder:'fixture/team',generation:0,workspace_token:'team-0',retention_enabled:false,retention_limit:3};

function fixture(initial=personal) {
  const calls=[],refreshes=[],nodes=new Map(),handlers=new Map(),timers=new Map(),timerIds={value:0};
  function node(id){
    if(!nodes.has(id))nodes.set(id,{addEventListener:(event,fn)=>handlers.set(id+'-'+event,fn),dataset:{},textContent:'',hidden:false,disabled:false,children:[],
      replaceChildren(...children){this.children=children;},append(...children){this.children.push(...children);}});
    return nodes.get(id);
  }
  const scenario={selected:null,server:initial,accept:false,chooseCalls:[],stops:0,revocations:0,syncError:null,read:null,
    transferPreview:null,transferApplied:null,confirmation:null};
  const window={__TAURI__:{core:{invoke:async(name,args)=>{
    calls.push({name,args});
    if(name==='shared_workspace_status')return scenario.server;
    if(name==='shared_workspace_choose_folder')return scenario.selected;
    if(name==='shared_workspace_connect'){assert.equal(args.folder,scenario.selected);scenario.server=team;return team;}
    if(name==='shared_workspace_disconnect'){assert.equal(args.workspaceToken,scenario.server.workspace_token);return scenario.server=personal;}
    if(name==='shared_workspace_sync'){if(scenario.syncError)throw Error(scenario.syncError);return scenario.syncResult||scenario.server;}
    if(name==='shared_workspace_discard_pending'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      return scenario.server={...team,generation:3,workspace_token:'team-3'};
    }
    if(name==='shared_workspace_reset_preview'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      return {folder:scenario.server.folder,confirmedGeneration:3,foundGeneration:0};
    }
    if(name==='shared_workspace_reset_apply'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      return scenario.server={...team,generation:4,workspace_token:'reset-4'};
    }
    if(name==='shared_workspace_set_snapshot_retention'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      return scenario.server={...scenario.server,retention_enabled:args.enabled};
    }
    if(name==='shared_workspace_transfer_preview'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      if(typeof scenario.transferPreview==='function')return scenario.transferPreview(args.page);
      return scenario.transferPreview||{sourceRevision:7,targetRevision:3,totalReports:2,pageIndex:args.page,pageSize:50,hasNext:false,rows:[
        {id:'local-new',reportType:'day',start:'2024-02-28',end:'2024-02-28',status:'ready',sourceName:'import.xlsx'},
        {id:'local-conflict',reportType:'day',start:'2024-02-29',end:'2024-02-29',status:'conflict',sourceName:'previous.xlsx'}
      ]};
    }
    if(name==='shared_workspace_transfer_apply'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      scenario.transferApplied=args;
      scenario.server={...scenario.server,generation:1};
      return {imported:args.selectedIds.length,revision:4,backupPath:'C:/local/transfer-backups/snapshot.sqlite3'};
    }
    if(name==='native_read_revision')return scenario.read||7;
    throw Error('Unexpected IPC '+name);
  }},dialog:{open:async options=>{scenario.chooseCalls.push(options);return scenario.selected;}}},
    confirm:async message=>{scenario.confirmation=message;return scenario.accept;},unifiedUI:{snapshot:{revision:7},refreshWorkspace:async()=>refreshes.push(window.sharedWorkspace.state)},
    addEventListener:(name,fn)=>handlers.set(name,fn),
    sqlRowScroll:{attach:(root,options)=>{root.pager=options;},reset:root=>{root.pager=null;}},
    nativeFolderAutoImport:{stopForWorkspaceChange:async()=>{scenario.stops++;},forgetWorkspaceSession:()=>{scenario.revocations++;}}};
  const context={window,document:{getElementById:node,createElement:tag=>({
      tagName:tag.toUpperCase(),children:[],listeners:{},dataset:{},
      addEventListener(name,fn){this.listeners[name]=fn;},
      append(...children){this.children.push(...children);},
      replaceChildren(...children){this.children=children;},
    })},setTimeout:fn=>{const id=++timerIds.value;timers.set(id,fn);return id;},
    clearTimeout:id=>timers.delete(id)};
  vm.runInNewContext(script('shared-workspace'),context);vm.runInNewContext(script('bridge'),context);
  const click=id=>handlers.get(id+'-click')();
  async function tick(){const next=timers.entries().next().value;assert.ok(next,'poll scheduled');timers.delete(next[0]);await next[1]();}
  return {window,calls,refreshes,scenario,node,click,tick,timers,handlers};
}

test('startup and normal SQL share one workspace identity; cancelled selection keeps personal data',async()=>{
  const f=fixture();
  await Promise.all([f.window.sharedWorkspace.init(),f.window.desktopAPI.nativeRevision()]);
  assert.equal(f.calls.filter(c=>c.name==='shared_workspace_status').length,1);
  assert.equal(f.calls.at(-1).args.workspaceToken,'private');
  await f.click('sharedWorkspaceConnect');
  assert.equal(f.calls.at(-1).name,'shared_workspace_choose_folder');assert.equal(f.scenario.chooseCalls.length,0);
  assert.equal(f.scenario.stops,0);assert.equal(f.refreshes.length,0);assert.equal(f.timers.size,0);
  f.scenario.selected='fixture/team';await f.click('sharedWorkspaceConnect');
  assert.equal(f.scenario.stops,1);assert.equal(f.refreshes.length,1);
  assert.equal(f.calls.filter(c=>c.name==='shared_workspace_connect').length,1);
  assert.match(f.node('sharedWorkspaceStatus').textContent,/fixture\/team/);
  await f.window.desktopAPI.nativeRevision();assert.equal(f.calls.at(-1).args.workspaceToken,'team-0');
  assert.ok(!f.calls.some(c=>['save_data','load_data','set_folder'].includes(c.name)));
});

test('peer changes refresh equal SQL revisions, discard late reads and block offline use until reconnect',async()=>{
  const f=fixture(team);await f.window.sharedWorkspace.init();
  let resolve;f.scenario.read=new Promise(r=>{resolve=r;});
  const previous=f.window.desktopAPI.nativeRevision();
  f.scenario.server={...team,generation:1,workspace_token:'team-1'};
  await f.tick();assert.equal(f.refreshes.length,1);
  resolve(7);await assert.rejects(previous,/Активная база изменилась/);
  await f.tick();assert.equal(f.refreshes.length,1,'unchanged polls preserve open forms');
  assert.equal(f.scenario.revocations,1,'the old native folder lease does not survive a peer pull');
  f.scenario.syncError='Связь прервана';await f.tick();
  assert.match(f.node('sharedWorkspaceStatus').textContent,/Связь.*отсутствует/);
  assert.equal(f.node('sharedWorkspaceMode').textContent,'Общая база недоступна');
  assert.equal(f.node('sharedWorkspaceDisconnect').hidden,false);
  assert.equal(f.node('sharedWorkspaceRetry').hidden,false);
  const before=f.calls.length;await assert.rejects(f.window.desktopAPI.nativeRevision(),/Связь прервана/);
  assert.equal(f.calls.length,before,'offline reads never go to another database');
  f.scenario.syncError=null;f.scenario.server={...team,generation:1,workspace_token:'reconnected'};
  await f.tick();assert.equal(f.refreshes.at(-1).workspace_token,'reconnected');
  assert.equal(await f.window.desktopAPI.nativeRevision(),7);
  f.handlers.get('pagehide')();assert.equal(f.timers.size,0);
});

test('pending publication blocks edits and requires positive confirmation to discard it',async()=>{
  const pending={...team,available:false,pending:true,error:'Публикация не подтверждена',workspace_token:'pending'};
  const f=fixture(pending);await f.window.sharedWorkspace.init();
  assert.equal(f.node('sharedWorkspaceDiscard').hidden,false);
  await assert.rejects(f.window.desktopAPI.unifiedClearReports(7),/Публикация/);
  await f.click('sharedWorkspaceDiscard');assert.equal(f.calls.length,1);
  f.scenario.accept=true;await f.click('sharedWorkspaceDiscard');
  assert.equal(f.calls.at(-1).name,'shared_workspace_discard_pending');assert.equal(f.node('sharedWorkspaceDiscard').hidden,true);
  assert.equal(f.refreshes.length,1);assert.equal(f.window.sharedWorkspace.state.workspace_token,'team-3');
});

test('reset confirms the exact share and missing generation without touching personal mode or pending writes',async()=>{
  const f=fixture({...team,generation:3,available:false});await f.window.sharedWorkspace.init();
  assert.equal(f.node('sharedWorkspaceReset').hidden,false);
  await f.click('sharedWorkspaceReset');
  assert.match(f.scenario.confirmation,/fixture\/team/);
  assert.match(f.scenario.confirmation,/поколение 3/);
  assert.equal(f.calls.filter(c=>c.name==='shared_workspace_reset_apply').length,0);
  assert.equal(f.scenario.revocations,0);
  f.scenario.accept=true;await f.click('sharedWorkspaceReset');
  assert.equal(f.calls.filter(c=>c.name==='shared_workspace_reset_apply').length,1);
  assert.equal(f.refreshes[0].generation,4);
  for(const mode of [personal,{...team,available:false,pending:true}]){
    const blocked=fixture(mode);await blocked.window.sharedWorkspace.init();
    assert.equal(blocked.node('sharedWorkspaceReset').hidden,true);
    await blocked.click('sharedWorkspaceReset');
    assert.equal(blocked.calls.length,1);
  }
});

test('snapshot retention confirms the shared folder and limit before enabling automatic deletion',async()=>{
  const f=fixture(team);await f.window.sharedWorkspace.init();
  const button=f.node('sharedSnapshotRetention');
  assert.equal(button.hidden,false);
  await f.click('sharedSnapshotRetention');
  assert.match(f.scenario.confirmation,/fixture\/team/);
  assert.match(f.scenario.confirmation,/до 3 последних снимков/);
  assert.match(f.scenario.confirmation,/обновите приложение на всех компьютерах/i);
  assert.equal(f.calls.filter(call=>call.name==='shared_workspace_set_snapshot_retention').length,0);
  f.scenario.accept=true;await f.click('sharedSnapshotRetention');
  assert.equal(f.window.sharedWorkspace.state.retention_enabled,true);
  assert.match(button.textContent,/Выключить/);
  assert.match(f.node('sharedSnapshotRetentionStatus').textContent,/до 3 снимков/);
  await f.click('sharedSnapshotRetention');
  assert.equal(f.window.sharedWorkspace.state.retention_enabled,false);
});

test('a registered offline share can be replaced on the first selection without invoking its old lease',async()=>{
  const f=fixture({...team,available:false,error:'Нет доступа'});
  await f.window.sharedWorkspace.init();f.scenario.selected='fixture/team';
  await f.click('sharedWorkspaceConnect');
  assert.equal(f.scenario.stops,0);assert.ok(f.scenario.revocations>0);
  assert.equal(f.calls.filter(c=>c.name==='shared_workspace_connect').length,1);
});

test('switching back confirms, never moves reports and never crosses pending publication',async()=>{
  const f=fixture(team);await f.window.sharedWorkspace.init();
  assert.equal(f.node('sharedDatabaseCard').dataset.selected,'true');
  assert.equal(f.node('personalDatabasePath').textContent,personal.personal_database_path);
  await f.click('sharedWorkspaceDisconnect');assert.equal(f.scenario.stops,0);
  f.scenario.accept=true;await f.click('sharedWorkspaceDisconnect');
  assert.equal(f.scenario.stops,1);assert.equal(f.refreshes.at(-1).mode,'personal');
  assert.equal(f.node('personalDatabaseCard').dataset.selected,'true');
  assert.equal(f.node('personalDatabasePath').textContent,personal.personal_database_path);
  assert.equal(f.node('sharedWorkspaceDisconnect').hidden,true);
  const pending=fixture({...team,pending:true});await pending.window.sharedWorkspace.init();
  pending.scenario.accept=true;
  await pending.click('sharedWorkspaceDisconnect');await pending.click('sharedWorkspaceConnect');
  assert.equal(pending.calls.length,1);assert.equal(pending.scenario.stops,0);
  const offline=fixture({...team,available:false});await offline.window.sharedWorkspace.init();
  offline.scenario.accept=true;await offline.click('sharedWorkspaceDisconnect');
  assert.equal(offline.scenario.stops,0);assert.equal(offline.refreshes.at(-1).mode,'personal');
});

test('reviewed personal reports migrate only selected new rows and preserve conflict entries',async()=>{
 const f=fixture(team);
 await f.window.sharedWorkspace.init();
 assert.equal(f.node('personalTransferPanel').hidden,false);
 await f.click('personalTransferPreview');
 assert.equal(f.node('personalTransferList').children.length,2);
 assert.match(f.node('personalTransferSummary').textContent,/новых на этой странице: 1/);
 assert.equal(f.node('personalTransferApply').disabled,true);
 await f.click('personalTransferSelectAll');
 assert.equal(f.node('personalTransferApply').disabled,false);
 await f.click('personalTransferApply');
 assert.equal(f.scenario.transferApplied,null,'confirmation is necessary');
 f.scenario.accept=true;
 await f.click('personalTransferApply');
 assert.deepEqual(Array.from(f.scenario.transferApplied.selectedIds),['local-new']);
 assert.equal(f.scenario.transferApplied.sourceRevision,7);
 assert.equal(f.scenario.transferApplied.targetRevision,3);
 assert.match(f.node('personalTransferResult').textContent,/Перенесено отчётов: 1/);
 assert.match(f.node('personalTransferResult').textContent,/резервная копия/i);
 assert.equal(f.refreshes.length,1,'shared SQL views refreshed after confirmed publication');
});
test('confirmed transfer stays successful if refreshing the shared dashboard fails',async()=>{
 const f=fixture(team);
 await f.window.sharedWorkspace.init();
 await f.click('personalTransferPreview');
 await f.click('personalTransferSelectAll');
 f.scenario.accept=true;
 f.window.unifiedUI.refreshWorkspace=async()=>{throw Error('Сбой обновления интерфейса');};
 await f.click('personalTransferApply');
 assert.deepEqual(Array.from(f.scenario.transferApplied.selectedIds),['local-new']);
 assert.match(f.node('personalTransferResult').textContent,/Перенесено отчётов: 1/);
 assert.match(f.node('personalTransferResult').textContent,/Не удалось обновить экран/);
 assert.doesNotMatch(f.node('personalTransferResult').textContent,/Не повторяйте перенос/);
 assert.equal(f.calls.filter(c=>c.name==='shared_workspace_transfer_apply').length,1);
});
test('transfer is unavailable in personal mode or while network publication is pending',async()=>{
 for(const mode of [personal,{...team,pending:true,available:false}]){
  const f=fixture(mode);
  await f.window.sharedWorkspace.init();
  assert.equal(f.node('personalTransferPreview').disabled,true);
  await f.click('personalTransferPreview');
  assert.equal(f.calls.filter(x=>x.name==='shared_workspace_transfer_preview').length,0);
  assert.equal(f.node('personalTransferPanel').hidden,mode.mode==='personal');
 }
});

test('personal transfer shows 50 rows with direct navigation and keeps the 25-record batch limit',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 const report=(i,status='ready')=>({id:'work-'+i,reportType:'day',start:'2024-01-01',end:'2024-01-01',status});
 f.scenario.transferPreview=page=>({
  sourceRevision:7,targetRevision:3,totalReports:103,pageIndex:page,pageSize:50,
  hasNext:page<2,initializesOrganizations:false,
  rows:page===0?Array.from({length:50},(_,i)=>report(i,i<24?'ready':'conflict')):[report(100),report(101),report(102)]
 });
 await f.click('personalTransferPreview');
 const list=f.node('personalTransferList');
 assert.equal(list.children.length,50);
 assert.equal(list.pager.size,50);
 assert.equal(list.pager.total,103);
 await f.click('personalTransferSelectAll');
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 24/);
 await list.pager.go(2);
 assert.equal(list.children.length,3);
 assert.equal(list.pager.index,2);
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 24/);
 await f.click('personalTransferSelectAll');
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 25/);
 assert.equal(list.children[1].children[0].disabled,true);
 f.scenario.accept=true;
 await f.click('personalTransferApply');
 assert.deepEqual(Array.from(f.scenario.transferApplied.selectedIds),[...Array.from({length:24},(_,i)=>'work-'+i),'work-100']);
});
test('a transfer preview returns to page one if the selected page disappears',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 const row={id:'work-1',reportType:'day',start:'2024-01-01',end:'2024-01-01',status:'ready'};
 f.scenario.transferPreview={sourceRevision:7,targetRevision:3,totalReports:101,pageIndex:0,pageSize:50,hasNext:true,rows:[row]};
 await f.click('personalTransferPreview');
 await f.click('personalTransferSelectAll');
 f.scenario.transferPreview=page=>({sourceRevision:8,targetRevision:3,totalReports:1,pageIndex:page,pageSize:50,hasNext:false,rows:page===0?[row]:[]});
 await f.node('personalTransferList').pager.go(2);
 assert.deepEqual(f.calls.filter(call=>call.name==='shared_workspace_transfer_preview').map(call=>call.args.page),[0,2,0]);
 assert.equal(f.node('personalTransferList').children.length,1);
 assert.equal(f.node('personalTransferPager').hidden,true);
 assert.equal(f.node('personalTransferApply').disabled,true);
});
test('server-side organization incompatibility disables the affected report',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 f.scenario.transferPreview={sourceRevision:7,targetRevision:3,totalReports:1,pageIndex:0,pageSize:50,
  hasNext:false,organizationWarning:'Названия подразделений различаются.',
  rows:[{id:'private-a',reportType:'day',start:'2024-02-28',end:'2024-02-28',status:'organization_conflict'}]
 };
 await f.click('personalTransferPreview');
 assert.match(f.node('personalTransferSummary').textContent,/подразделений различаются/);
 assert.match(f.node('personalTransferList').children[0].children[1].textContent,/Подразделение отсутствует/);
 assert.equal(f.node('personalTransferApply').disabled,true);
 await f.click('personalTransferSelectAll');
 assert.equal(f.node('personalTransferApply').disabled,true);
});

test('reviewing a date range from the team applies only reviewed IDs to personal SQLite',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 f.scenario.transferPreview={sourceRevision:9,targetRevision:6,totalReports:3,pageIndex:0,pageSize:50,
  hasNext:false,rows:[
   {id:'ready',reportType:'day',start:'2024-02-28',end:'2024-02-28',status:'ready'},
   {id:'duplicate',reportType:'day',start:'2024-02-29',end:'2024-02-29',status:'duplicate'},
   {id:'conflict',reportType:'day',start:'2024-02-29',end:'2024-02-29',status:'conflict'}
  ]};
 const prompts=[];f.window.confirm=async prompt=>{prompts.push(prompt);return f.scenario.accept;};
 f.node('reportTransferDirection').value='toPersonal';
 f.handlers.get('reportTransferDirection-change')();
 f.node('reportTransferFrom').value='2024-02-01';f.node('reportTransferTo').value='2024-02-29';
 await f.click('personalTransferPreview');
 const args=f.calls.find(call=>call.name==='shared_workspace_transfer_preview').args;
 assert.equal(args.direction,'toPersonal');
 assert.deepEqual({...args.period},{from:'2024-02-01',to:'2024-02-29'});
 assert.match(f.node('personalTransferSummary').textContent,/Общая база: 3 отчётов/);
 assert.match(f.node('personalTransferList').children[1].children[1].textContent,/Уже есть в личной базе/);
 await f.click('personalTransferSelectAll');
 await f.click('personalTransferApply');
 assert.match(prompts[0],/в личную базу/);
 assert.equal(f.scenario.transferApplied,null);
 f.scenario.accept=true;
 await f.click('personalTransferApply');
 assert.equal(f.scenario.transferApplied.direction,'toPersonal');
 assert.deepEqual(Array.from(f.scenario.transferApplied.selectedIds),['ready']);
 assert.equal(f.scenario.transferApplied.sourceRevision,9);
 assert.equal(f.scenario.transferApplied.targetRevision,6);
 assert.match(f.node('personalTransferResult').textContent,/Резервная копия личной базы/);
 assert.equal(f.refreshes.length,0,'shared workspace should not refresh after a personal-only write');
});

test('changing a transfer filter discards a previous selection',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 await f.click('personalTransferPreview');await f.click('personalTransferSelectAll');
 f.node('reportTransferFrom').value='2024-03-01';
 f.handlers.get('reportTransferFrom-change')();
 assert.equal(f.node('personalTransferApply').disabled,true);
 await f.click('personalTransferPreview');
 assert.equal(f.calls.filter(call=>call.name==='shared_workspace_transfer_preview').length,1);
 assert.match(f.node('personalTransferResult').textContent,/обе даты/i);
});

test('a personal-base switch clicked during automatic sync survives an unchanged poll',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 const button=f.node('sharedWorkspaceDisconnect'),states=[];
 Object.defineProperty(button,'disabled',{get(){return states.at(-1)??false;},set(value){states.push(value);}});
 let finish;f.scenario.syncResult=new Promise(done=>{finish=done;});
 const poll=f.tick();
 assert.equal(button.disabled,false);
 const switching=f.click('sharedWorkspaceDisconnect');
 assert.equal(f.scenario.confirmation,null);
 finish(team);await poll;await switching;
 assert.match(f.scenario.confirmation,/Перейти в личную базу/);
 assert.equal(f.window.sharedWorkspace.state.mode,'shared','cancel preserves the mode');
 assert.equal(f.calls.filter(c=>c.name==='shared_workspace_disconnect').length,0);
 states.length=0;await f.tick();
 assert.ok(!states.includes(true),'an unchanged background read must preserve control focus');
});

test('an automatic sync that changes the workspace invalidates a waiting disconnect',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();f.scenario.accept=true;
 let finish;f.scenario.syncResult=new Promise(done=>{finish=done;});
 const poll=f.tick(),switching=f.click('sharedWorkspaceDisconnect');
 finish({...team,workspace_token:'team-next'});await poll;await switching;
 assert.equal(f.scenario.confirmation,null);
 assert.equal(f.calls.filter(c=>c.name==='shared_workspace_disconnect').length,0);
});

test('repeated activation during one automatic poll switches once after positive confirmation',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();f.scenario.accept=true;
 let finish;f.scenario.syncResult=new Promise(done=>{finish=done;});
 const poll=f.tick(),first=f.click('sharedWorkspaceDisconnect'),second=f.click('sharedWorkspaceDisconnect');
 finish(team);await Promise.all([poll,first,second]);
 assert.equal(f.calls.filter(c=>c.name==='shared_workspace_disconnect').length,1);
 assert.equal(f.window.sharedWorkspace.state.mode,'personal');
});
