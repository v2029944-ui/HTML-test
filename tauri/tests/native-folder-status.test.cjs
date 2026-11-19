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
  const ids=['sourceFolderChoose','sourceFolderRefresh','sourceFolderLocation','sourceFolderStatus','sourceFolderRows','sourceFolderCount','sourceRuleFile','sourceRuleFormat','sourceRuleType','sourceRuleStart','sourceRuleSheet','sourceRuleYear','sourceRuleSave','sourceRuleStatus'];
  const nodes=Object.fromEntries(ids.map(id=>[id,new Node('div')]));
  let listing=options.listing===undefined?{folder:null,revision:12,files:[],rules:{}}:options.listing;
  const calls=[];let onChanged=null;let unsubscribed=false;
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
    desktopAPI:api,
    confirm:()=>{calls.push('confirm');return options.consent!==false;},
    nativeWorkspaceRefresh:async()=>{
      calls.push('sql_refresh');
      if(options.refreshFails)throw Error('stale view');
      return true;
    },
    addEventListener:(name,callback)=>{if(name==='pagehide')window.onHide=callback;}
  };
  const document={getElementById:id=>nodes[id],createElement:tag=>new Node(tag)};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-folder-status.js'),'utf8'),{window,document});
  return {nodes,window,calls,getListing:()=>listing,setListing:v=>listing=v,onChanged:()=>onChanged?.(),isUnsubscribed:()=>unsubscribed};
}

test('native Excel source overview is packaged only for Tauri SQL workspace',()=>{
  const root=path.resolve(__dirname,'../..');
  const output=fs.mkdtempSync(path.join(os.tmpdir(),'sql-folder-'));
  try{
    prepare({base:root,output});
    const html=fs.readFileSync(path.join(output,'native-workspace.html'),'utf8');
    assert.match(html,/id="sourceFolderRows"/);
    assert.match(html,/tauri-native-folder-status\.js/);
    assert.equal(fs.readFileSync(path.join(output,'tauri-native-folder-status.js'),'utf8'),
      fs.readFileSync(path.join(root,'tauri/native-folder-status.js'),'utf8'));
    const browser=fs.readFileSync(path.join(root,'index.html'),'utf8');
    assert.ok(!browser.includes('sourceFolderRows'));
    assert.ok(!browser.includes('tauri-native-folder-status.js'));
  }finally{fs.rmSync(output,{recursive:true,force:true});}
});

test('metadata-only list is capped to 100 rendered names and escapes untrusted data',async()=>{
  const files=Array.from({length:140},(_,i)=>({
    name:i===0?'<img src=x onerror=alert(1)>.xlsx':'report-'+i+'.xlsx',
    signature:'test-sig-'+i,
    size:i===0?100:1048576,
    tooLarge:i===2
  }));
  const panel=mount({listing:{folder:'C:/Excel',revision:12,files,rules:{'report-1.xlsx':{type:'day'}}}});
  await panel.window.nativeFolderReady;
  const {nodes,calls}=panel;
  assert.deepEqual(calls,['list']);
  assert.equal(nodes.sourceFolderRows.children.length,100);
  assert.equal(nodes.sourceFolderRows.children[0].children[0].textContent,files[0].name);
  assert.equal(nodes.sourceFolderRows.children[0].children[0].children.length,0);
  assert.equal(nodes.sourceFolderRows.children[1].children[2].textContent,'Есть правило');
  assert.match(nodes.sourceFolderRows.children[2].children[2].textContent,/Превышен предел/);
  assert.match(nodes.sourceFolderCount.textContent,/показаны первые 100/);
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

test('failed SQL revision refresh after folder selection is not mistaken for a failed folder change',async()=>{
 const panel=mount({listing:{folder:'C:/Existing',revision:12,files:[],rules:{}},refreshFails:true});
 await panel.window.nativeFolderReady;
 await panel.nodes.sourceFolderChoose.events.click();
 assert.equal(panel.calls.filter(x=>x==='choose').length,1);
 assert.equal(panel.calls.filter(x=>x==='sql_refresh').length,1);
 assert.match(panel.nodes.sourceFolderStatus.textContent,/Нажмите «Обновить данные»/);
});

test('save Excel rule only after explicit source selection and valid date',async()=>{
 const file={name:'week-2026.xlsx',size:2048,signature:'2048:123:456',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],rules:{}}});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 assert.equal(e.sourceRuleSave.disabled,true);
 e.sourceFolderRows.children[0].children[3].children[0].events.click();
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
 assert.equal(saved.signature,file.signature);
 assert.equal(saved.rule.type,'week');
 assert.equal(saved.rule.start,'2026-11-16');
 assert.equal(saved.rule.sheet,'Week');
 assert.equal(saved.rule.format,'legacy');
 assert.equal(panel.calls.some(x=>x==='sql_refresh'),true);
 assert.equal(e.sourceRuleSave.disabled,true);
 assert.match(e.sourceRuleStatus.textContent,/Правило сохранено/);
 assert.equal(panel.calls.some(x=>x==='load_data'||x==='read_excel'||x==='save_data'),false);
});

test('wide import rule accepts day only, optional year and no first period',async()=>{
 const file={name:'year.xlsx',size:10,signature:'10:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:23,files:[file],rules:{}}});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 e.sourceFolderRows.children[0].children[3].children[0].events.click();
 e.sourceRuleFormat.value='daily-wide';e.sourceRuleFormat.events.change();
 assert.equal(e.sourceRuleType.value,'day');
 assert.equal(e.sourceRuleType.disabled,true);
 assert.equal(e.sourceRuleStart.disabled,true);
 e.sourceRuleYear.value='2024';
 await e.sourceRuleSave.events.click();
 const saved=panel.calls.find(x=>x.kind==='rule');
 assert.equal(saved.rule.type,'day');
 assert.equal(saved.rule.start,'');
 assert.equal(saved.rule.year,2024);
 assert.equal(saved.rule.sheet,'');
});

test('advanced mapping is protected against accidental deletion',async()=>{
 const file={name:'complex.xlsx',size:20,signature:'20:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],
   rules:{'complex.xlsx':{type:'day',start:'',format:'daily-wide',mappings:{Sheet1:{work_col:0}}}}}});
 await panel.window.nativeFolderReady;
 panel.nodes.sourceFolderRows.children[0].children[3].children[0].events.click();
 assert.equal(panel.nodes.sourceRuleSave.disabled,true);
 assert.match(panel.nodes.sourceRuleStatus.textContent,/расширенную настройку/);
});

test('stale revision is rejected and never marked saved',async()=>{
 const file={name:'week.xlsx',size:10,signature:'10:1:1',tooLarge:false};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[file],rules:{}},failRule:true});
 await panel.window.nativeFolderReady;
 const e=panel.nodes;
 e.sourceFolderRows.children[0].children[3].children[0].events.click();
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
 const small={name:'forty.xlsx',size:40*1024*1024,signature:'40m:1:2',tooLarge:true,nativeTooLarge:false};
 const tooBig={name:'hundred-one.xlsx',size:101*1024*1024,signature:'101m:1:2',tooLarge:true,nativeTooLarge:true};
 const panel=mount({listing:{folder:'C:/Excel',revision:12,files:[small,tooBig],rules:{}}});
 await panel.window.nativeFolderReady;
 const rows=panel.nodes.sourceFolderRows.children;
 assert.equal(rows.length,2);
 assert.equal(rows[0].children[3].children[0].disabled,false);
 rows[0].children[3].children[0].events.click();
 assert.equal(panel.nodes.sourceRuleSave.disabled,false);
 assert.equal(rows[1].children[3].children[0].disabled,true);
 assert.match(rows[1].children[2].textContent,/100 МиБ/);
 assert.equal(panel.calls.some(x=>x.kind==='rule'),false);
});
