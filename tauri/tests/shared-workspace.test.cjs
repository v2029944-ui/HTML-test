'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=name=>fs.readFileSync(path.join(__dirname,'..',name+'.js'),'utf8');
const personal={mode:'personal',folder:null,generation:null,available:true,error:null,pending:false,workspace_token:'private'};
const team={...personal,mode:'shared',folder:'fixture/team',generation:0,workspace_token:'team-0'};

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
    if(name==='shared_workspace_sync'){if(scenario.syncError)throw Error(scenario.syncError);return scenario.server;}
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
    if(name==='shared_workspace_transfer_preview'){
      assert.equal(args.workspaceToken,scenario.server.workspace_token);
      if(typeof scenario.transferPreview==='function')return scenario.transferPreview(args.page);
      return scenario.transferPreview||{sourceRevision:7,targetRevision:3,totalReports:2,pageIndex:args.page,pageSize:25,hasNext:false,rows:[
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
  await f.click('sharedWorkspaceDisconnect');assert.equal(f.scenario.stops,0);
  f.scenario.accept=true;await f.click('sharedWorkspaceDisconnect');
  assert.equal(f.scenario.stops,1);assert.equal(f.refreshes.at(-1).mode,'personal');
  assert.equal(f.node('personalDatabaseCard').dataset.selected,'true');
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

test('personal transfer can select across pages while respecting the 25-record batch limit',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 f.scenario.transferPreview=page=>({
  sourceRevision:7,targetRevision:3,totalReports:26,pageIndex:page,pageSize:25,
  hasNext:page===0,initializesOrganizations:false,
  rows:page===0?[{id:'work-1',reportType:'day',start:'2024-01-01',end:'2024-01-01',status:'ready'}]:
    [{id:'work-2',reportType:'day',start:'2024-01-02',end:'2024-01-02',status:'ready'}]
 });
 await f.click('personalTransferPreview');
 await f.click('personalTransferSelectAll');
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 1/);
 await f.click('personalTransferNext');
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 1/);
 await f.click('personalTransferSelectAll');
 assert.match(f.node('personalTransferSummary').textContent,/выбрано: 2/);
 f.scenario.accept=true;
 await f.click('personalTransferApply');
 assert.deepEqual(Array.from(f.scenario.transferApplied.selectedIds),['work-1','work-2']);
});
test('server-side organization incompatibility disables the affected report',async()=>{
 const f=fixture(team);await f.window.sharedWorkspace.init();
 f.scenario.transferPreview={sourceRevision:7,targetRevision:3,totalReports:1,pageIndex:0,pageSize:25,
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
