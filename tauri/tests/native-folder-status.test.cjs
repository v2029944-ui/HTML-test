'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');

class Node {
  constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.className='';this.events={};this.disabled=false;}
  replaceChildren(...nodes){this.children=[...nodes];}
  append(...nodes){this.children.push(...nodes);}
  addEventListener(name,callback){this.events[name]=callback;}
}
function mount(options={}){
  const ids=['sourceFolderChoose','sourceFolderRefresh','sourceFolderLocation','sourceFolderStatus','sourceFolderRows','sourceFolderCount'];
  const nodes=Object.fromEntries(ids.map(id=>[id,new Node('div')]));
  let listing=options.listing===undefined?{folder:null,files:[],rules:{}}:options.listing;
  const calls=[];let onChanged=null;let unsubscribed=false;
  const api={
    list:async()=>{calls.push('list');if(options.failList)throw Error('disk unavailable');return listing;},
    chooseFolder:async()=>{
      calls.push('choose');
      return options.choice===undefined?{folder:'C:/Excel'}:options.choice;
    },
    onFilesChanged:(callback)=>{onChanged=callback;return ()=>{unsubscribed=true;};}
  };
  const window={
    desktopAPI:api,
    confirm:()=>{calls.push('confirm');return options.consent!==false;},
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
    size:i===0?100:1048576,
    tooLarge:i===2
  }));
  const panel=mount({listing:{folder:'C:/Excel',files,rules:{'report-1.xlsx':{type:'day'}}}});
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
  const listing={folder:'C:/Existing',files:[],rules:{'old.xlsx':{}}};
  const no=mount({listing,consent:false});
  await no.window.nativeFolderReady;
  await no.nodes.sourceFolderChoose.events.click();
  assert.deepEqual(no.calls,['list','confirm']);
  const yes=mount({listing});
  await yes.window.nativeFolderReady;
  await yes.nodes.sourceFolderChoose.events.click();
  assert.deepEqual(yes.calls,['list','confirm','choose','list']);
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
  const panel=mount({listing:{folder:'C:/Excel',files:null,rules:{}}});
  await panel.window.nativeFolderReady;
  assert.match(panel.nodes.sourceFolderStatus.textContent,/Не удалось проверить/);
  assert.equal(panel.nodes.sourceFolderRows.children.length,0);
  const error=mount({failList:true});
  await error.window.nativeFolderReady;
  assert.match(error.nodes.sourceFolderStatus.textContent,/disk unavailable/);
});
