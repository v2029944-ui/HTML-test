'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const root=path.resolve(__dirname,'..');
const group={handle:'9223372036854775700',id:'group-1',name:'<b>Группа</b>',members:51};
const handle='9223372036854775710';
class Element{
 constructor(){this.children=[];this.textContent='';this.value='';this.events={};}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=children;}
 addEventListener(type,fn){this.events[type]=fn;}
 setAttribute(){}
 focus(){}
 fire(type='click'){return this.events[type]({preventDefault(){}});}
}
function mount(){
 const html=fs.readFileSync(path.join(root,'native-workspace.html'),'utf8');
 const e=Object.fromEntries([...html.matchAll(/id="(management[^"]+)"/g)].map(([,id])=>[id,new Element()]));
 const reads=[],writes=[],confirmations=[];
 let confirmed=true,backupPath=null;
 const window={
  desktopAPI:{
   nativeMemberPage:async(...args)=>{
    reads.push(args);
    return args[2]===null?Array.from({length:50},()=>({handle,work:'<img src=x>'})):[{handle,work:'Другая работа'}];
   },
   nativeCatalogPage:async(...args)=>{
    reads.push(args);
    return [{handle,work:'<img src=x>',organization:'Цех'}];
   },
   chooseSqliteBackupPath:async()=>backupPath,
   nativeDeleteReport:async(revision,id,backup)=>{
    writes.push({revision,id,backup});
    return {revision:revision+1,deleted:1};
   },
   nativeSettingsEdit:async(revision,action)=>{
    writes.push({revision,action});
    return {revision:revision+1,cleanup_warning:null};
   }
  },
  crypto:{randomUUID:()=> 'new-group-id'},
  confirm:message=>{confirmations.push(message);return confirmed;},
  nativeWorkspaceRefresh:async()=>{
   await window.nativeManagement.setContext({revision:writes.at(-1).revision+1});
   return true;
  }
 };
 const document={getElementById:id=>e[id],createElement:()=>new Element()};
 vm.runInNewContext(fs.readFileSync(path.join(root,'native-management.js'),'utf8'),{window,document});
 return {m:window.nativeManagement,e,reads,writes,confirmations,setBackup:path=>{backupPath=path;},deny:()=>{confirmed=false;}};
}
test('member pages use the group and cursor strings unchanged; removal targets the rendered revision',async()=>{
 const {m,e,reads,writes,confirmations}=mount();
 await m.setContext({revision:7});
 const actions=new Element();
 m.addGroupActions(actions,group,7);
 await actions.children[0].fire();
 assert.deepEqual(reads[0],[7,group.handle,null,50]);
 assert.equal(e.managementMemberRows.children[0].children[0].textContent,'<img src=x>');
 await e.managementMemberNext.fire();
 assert.deepEqual(reads[1],[7,group.handle,handle,50]);
 await e.managementMemberRows.children[0].children[1].children[0].fire();
 assert.deepEqual(JSON.parse(JSON.stringify(writes[0])),{
  revision:7,action:{kind:'remove_member',group:group.handle,handle}
 });
 assert.match(confirmations[0],/Другая работа/);
 assert.deepEqual(reads.at(-1),[8,group.handle,null,50]);
});
test('group create, add and delete require assent and stale rows cannot write',async()=>{
 const {m,e,writes,deny}=mount();
 await m.setContext({revision:7});
 e.managementGroupName.value='Новая';
 await e.managementGroupForm.fire('submit');
 assert.equal(writes[0].action.kind,'create_group');
 assert.equal(writes[0].action.id,'new-group-id');
 const actions=new Element();
 m.addGroupActions(actions,group,8);
 await actions.children[0].fire();
 e.managementMemberName.value='Точная работа';
 await e.managementMemberForm.fire('submit');
 assert.equal(writes[1].revision,8);
 assert.equal(writes[1].action.work,'Точная работа');
 await actions.children[1].fire();
 assert.equal(writes.length,2,'stale button');
 m.addGroupActions(actions,group,9);
 await actions.children[3].fire();
 assert.equal(writes[2].action.kind,'delete_group');
 m.addGroupActions(actions,group,10);
 deny();
 await actions.children[5].fire();
 assert.equal(writes.length,3,'cancelled dialog');
});
test('catalog loads only on demand and supports create, edit, delete with revision confirmation',async()=>{
 const {m,e,reads,writes,confirmations}=mount();
 await m.setContext({revision:7});
 assert.equal(reads.length,0);
 e.managementCatalog.open=true;
 await e.managementCatalog.fire('toggle');
 assert.deepEqual(reads[0],[7,null,50]);
 const buttons=()=>e.managementCatalogRows.children[0].children[2].children;
 buttons()[0].fire();
 assert.equal(e.managementCatalogWork.value,'<img src=x>');
 e.managementCatalogOrganization.value='';
 await e.managementCatalogForm.fire('submit');
 assert.equal(writes[0].action.kind,'update_catalog');
 assert.equal(writes[0].action.handle,handle);
 assert.equal(writes[0].action.organization,null);
 await buttons()[1].fire();
 assert.equal(writes[1].action.kind,'delete_catalog');
 e.managementCatalogWork.value='Новая работа';
 await e.managementCatalogForm.fire('submit');
 assert.equal(writes[2].action.kind,'create_catalog');
 assert.equal(confirmations.length,3);
});
test('management bridge tracks revision-checked settings reads and writes',async()=>{
 const calls=[];
 const window={__TAURI__:{dialog:{open(){}},core:{invoke:async(command,args)=>{
  calls.push({command,args,saves:window.tauriOperations.saves,reads:window.tauriOperations.reads});
 }}}};
 vm.runInNewContext(fs.readFileSync(path.join(root,'bridge.js'),'utf8'),{window});
 const action={kind:'delete_catalog',handle};
 await window.desktopAPI.nativeMemberPage(4,group.handle,handle);
 await window.desktopAPI.nativeCatalogPage(4,handle);
 await window.desktopAPI.nativeSettingsEdit(4,action);
 assert.deepEqual(JSON.parse(JSON.stringify(calls)),[
  {command:'native_member_page',args:{revision:4,group:group.handle,afterHandle:handle,limit:50},saves:0,reads:1},
  {command:'native_catalog_page',args:{revision:4,afterHandle:handle,limit:50},saves:0,reads:1},
  {command:'native_settings_edit',args:{revision:4,action},saves:1,reads:0}
 ]);
 assert.equal(window.tauriOperations.saves,0);
});

test('report deletion requires affirmative confirmation and a backup destination',async()=>{
 const report={id:'report-id',period_start:'2025-05-01',period_end:'2025-05-07'};
 const {m,e,writes,confirmations,setBackup}=mount();
 await m.setContext({revision:7});
 const actions=new Element();
 m.addReportAction(actions,report,7);
 await actions.children[0].fire();
 assert.equal(writes.length,0,'cancelled backup dialog');
 setBackup('D:/backups/report.sqlite3');
 await actions.children[0].fire();
 assert.deepEqual(writes,[{revision:7,id:'report-id',backup:'D:/backups/report.sqlite3'}]);
 assert.match(confirmations[0],/2025-05-01.*2025-05-07.*report-id/);
 assert.match(e.managementReportStatus.textContent,/Отчёт удалён/);
 await actions.children[0].fire();
 assert.equal(writes.length,1,'report row with old revision');
 const declined=mount();
 await declined.m.setContext({revision:7});
 const buttons=new Element();
 declined.m.addReportAction(buttons,report,7);
 declined.setBackup('D:/backups/report.sqlite3');
 declined.deny();
 await buttons.children[0].fire();
 assert.equal(declined.writes.length,0,'declined confirmation');
});
