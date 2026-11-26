'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');

class Node {
  constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.className='';this.events={};this.disabled=false;this.value='';}
  replaceChildren(...nodes){this.children=[...nodes];}
  append(...nodes){this.children.push(...nodes);}
  addEventListener(name,callback){this.events[name]=callback;}
  setAttribute(name,value){this[name]=value;}
}
function mount(options={}){
  const ids=['sourceFolderChoose','sourceFolderRefresh','sourceFolderLocation','sourceFolderStatus','sourceFolderLastSync','sourceFolderRows','sourceFolderCount','sourceRuleFile','sourceRuleFormat','sourceRuleType','sourceRuleStart','sourceRuleSheet','sourceRuleYear','sourceRuleSave','sourceRuleStatus'];
  const nodes=Object.fromEntries(ids.map(id=>[id,new Node('div')]));
  let listing=options.listing===undefined?{folder:null,revision:12,files:[],rules:{}}:options.listing;
  const calls=[],confirmations=[];let onChanged=null;let unsubscribed=false;
  const api={
    list:async()=>{calls.push('list');if(options.failList)throw Error('disk unavailable');return listing;},
    nativeSetRuleChecked:async(name,rule,revision,folder,signature)=>{
      calls.push({kind:'rule',name,rule,revision,folder,signature});
      if(options.failRule)throw Error('Stale storage revision; refresh the selection.');
      listing.rules[name]=rule;
      listing.revision++;
      return listing.revision;
    },
    chooseFolder:async()=>{
      calls.push('choose');
      return options.choice===undefined?{folder:'C:/Excel'}:options.choice;
    },
    onFilesChanged:(callback)=>{onChanged=callback;return ()=>{unsubscribed=true;};}
  };
  const window={
    unifiedReports:{openSource:source=>calls.push({kind:'inspect',source})},
    sqlRowScroll:require('./scroll-window-fixture.cjs'),
    desktopAPI:api,
    confirm:message=>{calls.push('confirm');confirmations.push(message);return options.confirm?options.confirm(message):options.consent!==false;},
    nativeFolderAutoImport:options.autoImport??(options.autoEnabled?{isEnabled:()=>true,updateSnapshot:()=>{}}:undefined),
    nativeWorkspaceRefresh:async()=>{
      calls.push('sql_refresh');
      if(options.refreshFails)throw Error('stale view');
      return true;
    },
    addEventListener:(name,callback)=>{if(name==='pagehide')window.onHide=callback;if(name==='native-folder-import-status')window.onFolderImportStatus=callback;}
  };
  nodes.__created=[];const document={getElementById:id=>nodes[id],createElement:tag=>{const node=new Node(tag);nodes.__created.push(node);return node;}};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-folder-status.js'),'utf8'),{window,document});
  return {nodes,window,calls,confirmations,getListing:()=>listing,setListing:v=>listing=v,onChanged:()=>onChanged?.(),isUnsubscribed:()=>unsubscribed};
}

// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).

test('metadata-only list bounds each page to 50 names and escapes untrusted data',async()=>{
  const files=Array.from({length:140},(_,i)=>({
    name:i===0?'<img src=x onerror=alert(1)>.xlsx':'report-'+i+'.xlsx',
    nativeSignature:'test-sig-'+i,
    size:i===0?100:1048576,
    tooLarge:i===2
  }));
  const panel=mount({listing:{folder:'C:/Excel',revision:12,files,rules:{'report-1.xlsx':{type:'day'}}}});
  await panel.window.nativeFolderReady;
  const {nodes,calls}=panel;
  assert.deepEqual(calls,['list']);
  assert.equal(nodes.sourceFolderRows.children.length,50);
  assert.equal(nodes.sourceFolderRows.children[0].children[0].textContent,files[0].name);
  assert.equal(nodes.sourceFolderRows.children[0].children[0].children.length,0);
  assert.equal(nodes.sourceFolderRows.children[1].children[2].textContent,'Есть правило');
  assert.match(nodes.sourceFolderRows.children[2].children[2].textContent,/Превышен предел/);
  assert.equal(nodes.sourceFolderCount.textContent,'Найдено XLSX: 140');
  assert.equal(calls.includes('save'),false);
});

test('folder change warns about losing rules and never modifies reports',async()=>{
  const listing={folder:'C:/Existing',revision:12,files:[],rules:{'old.xlsx':{}}};
  const no=mount({listing,consent:false});
  await no.window.nativeFolderReady;
  await no.nodes.sourceFolderChoose.events.click();
  assert.deepEqual(no.calls,['list','confirm']);
  const yes=mount({listing});
  await yes.window.nativeFolderReady;
  await yes.nodes.sourceFolderChoose.events.click();
  assert.deepEqual(yes.calls,['list','confirm','choose','sql_refresh','list']);
  assert.match(yes.nodes.sourceFolderStatus.textContent,/Папка проверена/);
});

test('file-change watcher only refreshes listings; cancellation does not import',async()=>{
  const panel=mount({choice:null});
  await panel.window.nativeFolderReady;
  await panel.nodes.sourceFolderChoose.events.click();
  assert.equal(panel.calls.includes('choose'),true);
  const initial=panel.calls.filter(x=>x==='list').length;
  panel.onChanged();
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(panel.calls.filter(x=>x==='list').length,initial+1);
  assert.equal(panel.calls.some(x=>x==='apply'||x==='read_excel'||x==='save_data'),false);
  panel.window.onHide();
  assert.equal(panel.isUnsubscribed(),true);
  panel.onChanged();
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(panel.calls.filter(x=>x==='list').length,initial+1);
});

test('invalid Rust listing or unavailable folder reports an error instead of inventing data',async()=>{
  const panel=mount({listing:{folder:'C:/Excel',revision:12,files:null,rules:{}}});
  await panel.window.nativeFolderReady;
  assert.match(panel.nodes.sourceFolderStatus.textContent,/Не удалось проверить/);
  assert.equal(panel.nodes.sourceFolderRows.children.length,0);
  const error=mount({failList:true});
  await error.window.nativeFolderReady;
  assert.match(error.nodes.sourceFolderStatus.textContent,/disk unavailable/);
});

test('autoimport updates visible rows and sync time without recreating their controls',async()=>{
 const files=['import.xlsx','review.xlsx','locked.xlsx'].map(name=>({name,size:1,nativeSignature:name==='locked.xlsx'?null:'sig',tooLarge:false}));
 let completed=false;
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files,rules:{'import.xlsx':{},'review.xlsx':{}}},autoImport:{
  updateSnapshot(){},lastSuccessfulImport:()=>completed?Date.UTC(2026,10,24,12,0):null,
  fileStatus:(_,file)=>completed?(file.name==='import.xlsx'?'imported':'manual'):null
 }});
 await panel.window.nativeFolderReady;
 const {nodes}=panel,table=nodes.sourceFolderRows;
 assert.match(table.children[2].children[2].textContent,/недоступен/i);
 assert.match(nodes.sourceFolderLastSync.textContent,/ещё не было/i);
 const action=table.children[0].children[3].children[0];
 completed=true;panel.window.onFolderImportStatus();
 assert.equal(table.children[0].children[3].children[0],action);
 assert.match(table.children[0].children[2].textContent,/добавлены автоимпортом/i);
 assert.match(table.children[1].children[2].textContent,/ручной проверки/i);
 assert.match(nodes.sourceFolderLastSync.textContent,/Последний успешный автоимпорт/);
 assert.ok(nodes.sourceFolderLastSync.textContent.endsWith(new Date(Date.UTC(2026,10,24,12,0)).toLocaleString('ru-RU')));
});

test('failed SQL revision refresh after folder selection is not mistaken for a failed folder change',async()=>{
 const panel=mount({listing:{folder:'C:/Existing',revision:12,files:[],rules:{}},refreshFails:true});
 await panel.window.nativeFolderReady;
 await panel.nodes.sourceFolderChoose.events.click();
 assert.equal(panel.calls.filter(x=>x==='choose').length,1);
 assert.equal(panel.calls.filter(x=>x==='sql_refresh').length,1);
 assert.match(panel.nodes.sourceFolderStatus.textContent,/Нажмите «Обновить данные»/);
});

test('save Excel rule only after explicit source selection and valid date',async()=>{
 const file={name:'week-2026.xlsx',size:2048,nativeSignature:'2048:123:456',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],rules:{}}});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 assert.equal(e.sourceRuleSave.disabled,true);
 e.sourceFolderRows.children[0].children[3].children[1].events.click();
 assert.equal(e.sourceRuleSave.disabled,false);
 e.sourceRuleFormat.value='legacy';e.sourceRuleType.value='week';
 e.sourceRuleStart.value='invalid';
 await e.sourceRuleSave.events.click();
 assert.equal(panel.calls.some(x=>x.kind==='rule'),false);
 assert.match(e.sourceRuleStatus.textContent,/дату/);
 e.sourceRuleStart.value='2026-11-16';e.sourceRuleSheet.value='Week';
 await e.sourceRuleSave.events.click();
 const saved=panel.calls.find(x=>x.kind==='rule');
 assert.equal(saved.name,'week-2026.xlsx');
 assert.equal(saved.revision,12);
 assert.equal(saved.folder,'C:/Excel');
 assert.equal(saved.signature,file.nativeSignature);
 assert.equal(saved.rule.type,'week');
 assert.equal(saved.rule.start,'2026-11-16');
 assert.equal(saved.rule.sheet,'Week');
 assert.equal(saved.rule.format,'legacy');
 assert.equal(panel.calls.some(x=>x==='sql_refresh'),true);
 assert.equal(e.sourceRuleSave.disabled,true);
 assert.match(e.sourceRuleStatus.textContent,/Правило сохранено/);
 assert.equal(panel.calls.some(x=>x==='load_data'||x==='read_excel'||x==='save_data'),false);
});

test('shared import rule retains the selected period, optional year and blank first date',async()=>{
 const file={name:'year.xlsx',size:10,nativeSignature:'10:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:23,files:[file],rules:{}}});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 e.sourceFolderRows.children[0].children[3].children[1].events.click();
 e.sourceRuleFormat.value='period-pairs';e.sourceRuleFormat.events.change();
 assert.equal(e.sourceRuleType.value,'week');
 assert.equal(e.sourceRuleType.disabled,false);
 assert.equal(e.sourceRuleStart.disabled,false);
 e.sourceRuleYear.value='2024';
 await e.sourceRuleSave.events.click();
 const saved=panel.calls.find(x=>x.kind==='rule');
 assert.equal(saved.rule.type,'week');
 assert.equal(saved.rule.format,'period-pairs');
 assert.equal(saved.rule.start,'');
 assert.equal(saved.rule.year,2024);
 assert.equal(saved.rule.sheet,'');
});

test('advanced mapping is protected against accidental deletion',async()=>{
 const file={name:'complex.xlsx',size:20,nativeSignature:'20:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],
   rules:{'complex.xlsx':{type:'day',start:'',format:'daily-wide',mappings:{Sheet1:{work_col:0}}}}}});
 await panel.window.nativeFolderReady;
 panel.nodes.sourceFolderRows.children[0].children[3].children[1].events.click();
 assert.equal(panel.nodes.sourceRuleSave.disabled,true);
 assert.match(panel.nodes.sourceRuleStatus.textContent,/расширенную настройку/);
});

test('stale revision is rejected and never marked saved',async()=>{
 const file={name:'week.xlsx',size:10,nativeSignature:'10:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],rules:{}},failRule:true});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 e.sourceFolderRows.children[0].children[3].children[1].events.click();
 e.sourceRuleStart.value='2026-11-16';
 await e.sourceRuleSave.events.click();
 assert.equal(panel.calls.some(x=>x.kind==='rule'),true);
 assert.equal(panel.calls.some(x=>x==='sql_refresh'),false);
 assert.match(e.sourceRuleStatus.textContent,/Правило не сохранено/);
 assert.equal(e.sourceRuleSave.disabled,true);
});

test('missing source revision cannot authorize a rule change',async()=>{
 const panel=mount({listing:{folder:'C:/Excel',files:[],rules:{}}});
 await panel.window.nativeFolderReady;
 assert.match(panel.nodes.sourceFolderStatus.textContent,/Некорректный снимок/);
 assert.equal(panel.nodes.sourceRuleSave.disabled,true);
});

test('SQL folder accepts 40-MiB files in native mode while classic 30-MiB indicator remains',async()=>{
 const small={name:'forty.xlsx',size:40*1024*1024,nativeSignature:'40m:1:2',tooLarge:true,nativeTooLarge:false};
 const tooBig={name:'hundred-one.xlsx',size:101*1024*1024,nativeSignature:'101m:1:2',tooLarge:true,nativeTooLarge:true};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[small,tooBig],rules:{}}});
 await panel.window.nativeFolderReady;
 const rows=panel.nodes.sourceFolderRows.children;
 assert.equal(rows.length,2);
 assert.equal(rows[0].children[3].children[1].disabled,false);
 rows[0].children[3].children[1].events.click();
 assert.equal(panel.nodes.sourceRuleSave.disabled,false);
 assert.equal(rows[1].children[3].children[1].disabled,true);
 assert.match(rows[1].children[2].textContent,/100 МиБ/);
 assert.equal(panel.calls.some(x=>x.kind==='rule'),false);
});

test('saving a rule discloses possible automatic writes when the session opt-in is active',async()=>{
 const file={name:'one.xlsx',size:10,nativeSignature:'10:1:1',tooLarge:false};
 const panel=mount({autoEnabled:true,listing:{folder:'C:/Excel',revision:12,files:[file],rules:{}}});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 e.sourceFolderRows.children[0].children[3].children[1].events.click();
 e.sourceRuleStart.value='2026-11-16';
 await e.sourceRuleSave.events.click();
 assert.match(panel.confirmations[0],/Автоимпорт включён/);
 assert.match(panel.confirmations[0],/могут быть добавлены автоматически/);
});

const deferredConsent=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};
test('folder choice waits for asynchronous consent and denial never opens its picker',async()=>{
 const answer=deferredConsent(),x=mount({listing:{folder:'C:/Excel',revision:12,files:[],rules:{}},confirm:()=>answer.promise});await x.window.nativeFolderReady;
 const pending=x.nodes.sourceFolderChoose.events.click();assert.equal(x.calls.includes('choose'),false);assert.equal(x.nodes.sourceFolderChoose.disabled,true);
 await x.nodes.sourceFolderChoose.events.click();answer.resolve(false);await pending;assert.equal(x.calls.includes('choose'),false);assert.equal(x.nodes.sourceFolderChoose.disabled,false);
});
test('rule save waits for asynchronous consent, writes once after acceptance, and reports dialog errors',async()=>{
 const listing={folder:'C:/Excel',revision:12,files:[{name:'report.xlsx',nativeSignature:'sig',size:1,tooLarge:false}],rules:{}};
 const answer=deferredConsent(),x=mount({listing,confirm:()=>answer.promise});await x.window.nativeFolderReady;
 const row=x.nodes.sourceFolderRows.children[0];await row.children.at(-1).children[1].events.click();
 x.nodes.sourceRuleStart.value='2024-01-01';x.nodes.sourceRuleType.value='day';
 const pending=x.nodes.sourceRuleSave.events.click();assert.equal(x.calls.some(c=>c.kind==='rule'),false);await x.nodes.sourceRuleSave.events.click();answer.resolve(true);await pending;
 assert.equal(x.calls.filter(c=>c.kind==='rule').length,1);
 const rejected=deferredConsent(),y=mount({listing:{...listing,rules:{}},confirm:()=>rejected.promise});await y.window.nativeFolderReady;
 await y.nodes.sourceFolderRows.children[0].children.at(-1).children[1].events.click();y.nodes.sourceRuleStart.value='2024-01-01';y.nodes.sourceRuleType.value='day';
 const failing=y.nodes.sourceRuleSave.events.click();rejected.reject(Error('dialog unavailable'));await failing;
 assert.equal(y.calls.some(c=>c.kind==='rule'),false);assert.match(y.nodes.sourceRuleStatus.textContent,/dialog unavailable/);
});

test('one folder table pages beyond 100 and checks the actual 101st path without writing',async()=>{
 for(const folder of ['C:/Excel',String.raw`\\?\C:\Excel`]){
 const files=Array.from({length:101},(_,i)=>({name:'report-'+i+'.xlsx',nativeSignature:'sig-'+i,size:1,tooLarge:false}));const x=mount({listing:{folder,revision:12,files,rules:{}}});await x.window.nativeFolderReady;
 await x.nodes.sourceFolderRows.scrollWindow.go(2);assert.equal(x.nodes.sourceFolderRows.children.length,1);const row=x.nodes.sourceFolderRows.children[0];assert.equal(row.children[0].textContent,'report-100.xlsx');await row.children[3].children[0].events.click();assert.equal(x.calls.at(-1).source,folder+(folder.includes('\\')?'\\':'/')+'report-100.xlsx');assert.equal(x.calls.some(c=>c.kind==='rule'),false);
 }
});

test('rescanning a file preserves its page chooser while refreshing row metadata',async()=>{
 const files=Array.from({length:101},(_,i)=>({name:i+'.xlsx',size:1024,nativeSignature:'sig',tooLarge:false}));
 const listing={folder:'C:/reports',revision:1,files,rules:{}};
 const x=mount({listing});await x.window.nativeFolderReady;
 const rows=x.nodes.sourceFolderRows;
 const picker=rows.scrollWindow;
 files[0]={...files[0],size:2048,nativeSignature:'changed'};
 await x.window.unifiedReports.refreshFolder();
 assert.equal(rows.scrollWindow,picker);
 assert.equal(rows.children[0].children[1].textContent,'2 КБ');
 await picker.go(2);
 assert.equal(rows.children[0].children[0].textContent,'100.xlsx');
 files.pop();await x.window.unifiedReports.refreshFolder();
 assert.equal(rows.scrollWindow.index,1);
 assert.equal(rows.scrollWindow.total,100);
});
