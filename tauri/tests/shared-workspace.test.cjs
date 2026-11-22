'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=name=>fs.readFileSync(path.join(__dirname,'..',name+'.js'),'utf8');
const personal={mode:'personal',folder:null,generation:null,available:true,error:null,pending:false,workspace_token:'private'};
const team={...personal,mode:'shared',folder:'fixture/team',generation:0,workspace_token:'team-0'};

function fixture(initial=personal) {
  const calls=[],refreshes=[],nodes=new Map(),handlers=new Map(),timers=new Map(),timerIds={value:0};
  function node(id){
    if(!nodes.has(id))nodes.set(id,{addEventListener:(event,fn)=>handlers.set(id+'-'+event,fn),dataset:{},textContent:'',hidden:false,disabled:false});
    return nodes.get(id);
  }
  const scenario={selected:null,server:initial,accept:false,chooseCalls:[],stops:0,revocations:0,syncError:null,read:null};
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
    if(name==='native_read_revision')return scenario.read||7;
    throw Error('Unexpected IPC '+name);
  }},dialog:{open:async options=>{scenario.chooseCalls.push(options);return scenario.selected;}}},
    confirm:async()=>scenario.accept,unifiedUI:{snapshot:{revision:7},refreshWorkspace:async()=>refreshes.push(window.sharedWorkspace.state)},
    addEventListener:(name,fn)=>handlers.set(name,fn),
    nativeFolderAutoImport:{stopForWorkspaceChange:async()=>{scenario.stops++;},forgetWorkspaceSession:()=>{scenario.revocations++;}}};
  const context={window,document:{getElementById:node},setTimeout:fn=>{const id=++timerIds.value;timers.set(id,fn);return id;},
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
