'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),vm=require('node:vm'),path=require('node:path'),os=require('node:os');
const {prepareHtml,prepare}=require('../prepare.cjs');
const file=path.join(__dirname,'../native-reader.js');

test('Tauri-only SQLite viewer is prepared without changing root HTML',()=>{
  const root=path.resolve(__dirname,'../..');
  const source=fs.readFileSync(path.join(root,'index.html'),'utf8');
  const built=prepareHtml(source);
  assert.ok(!source.includes('tauri-native-reader.js'));
  assert.ok(built.includes('<script src="tauri-native-reader.js"></script>'));
  assert.ok(built.indexOf('tauri-native-reader.js')>built.indexOf('window.appReady=main();'));
  assert.throws(()=>prepareHtml(built),/уже присутствуют/);
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'tauri-paged-'));
  try{
    prepare({base:root,output:directory});
    assert.equal(fs.readFileSync(path.join(directory,'tauri-native-reader.js'),'utf8'),fs.readFileSync(file,'utf8'));
  }finally{fs.rmSync(directory,{force:true,recursive:true});}
});
class Element{
  constructor(tag){this.tag=tag;this.children=[];this.events={};this.value='';this.hidden=false;this.disabled=false;}
  append(...children){this.children.push(...children);}
  insertBefore(child){this.children.unshift(child);}
  replaceChildren(...children){this.children=[...children];}
  setAttribute(key,value){this[key]=value;}
  addEventListener(name,callback){this.events[name]=callback;}
}
const turn=()=>new Promise(resolve=>setImmediate(resolve));
function mount(api){
  const upload=new Element('div');
  const document={getElementById:id=>id==='upload'?upload:null,createElement:tag=>new Element(tag)};
  vm.runInNewContext(fs.readFileSync(file,'utf8'),{window:{desktopAPI:api},document});
  const panel=upload.children[0],toggle=panel.children[3],body=panel.children[4];
  const controls=body.children[0],status=body.children[1],items=body.children[2];
  const type=controls.children[0].children[1];
  return {panel,toggle,body,controls,status,items,type,refresh:controls.children[1],back:controls.children[2],next:controls.children[3]};
}
test('viewer fetches only keyset pages and updates cursors without accumulating rows',async()=>{
  const requests=[];
  const make=(index)=>({id:'r'+index,report_type:'day',period_start:'2026-11-'+String(1+Math.floor(index/10)).padStart(2,'0'),period_end:'2026-11-18',source_name:'<b>Untrusted</b>'});
  const ui=mount({
    nativeRevision:async()=>9,
    nativeReportPage:async(revision,filter,after,limit)=>{
      requests.push({revision,filter,after,limit});
      return !after?Array.from({length:100},(_,n)=>make(n)):
        [make(100),make(101)];
    }
  });
  assert.equal(requests.length,0,'Panel must be lazy');
  await ui.toggle.events.click();
  assert.equal(requests[0].limit,100);
  assert.equal(requests[0].revision,9);
  assert.equal(ui.items.children[0].children[1].children.length,100);
  assert.equal(ui.items.children[0].children[1].children[0].children[3].textContent,'<b>Untrusted</b>');
  assert.equal(ui.next.disabled,false);
  await ui.next.events.click();
  assert.equal(requests[1].after.id,'r99');
  assert.equal(ui.items.children[0].children[1].children.length,2);
  assert.equal(ui.next.disabled,true);
  await ui.back.events.click();
  assert.equal(requests[2].after,null);
  assert.equal(ui.items.children[0].children[1].children.length,100);
  ui.type.value='week';
  ui.type.events.change();await turn();
  assert.equal(requests[3].filter.report_type,'week');
  assert.equal(requests[3].after,null);
});
test('viewer fails closed when a SQLite revision becomes stale',async()=>{
  let hit=0;
  const make=(id)=>({id,report_type:'day',period_start:'2026-11-01',period_end:'2026-11-01'});
  const ui=mount({
    nativeRevision:async()=>hit===0?1:2,
    nativeReportPage:async(rev,filter,after)=>{
      hit++;
      if(after)throw Error('Stale storage revision');
      return Array.from({length:100},(_,i)=>make('r'+i));
    }
  });
  await ui.toggle.events.click();
  await ui.next.events.click();
  assert.match(ui.status.textContent,/Stale storage revision/);
  assert.equal(ui.next.disabled,true);
  assert.equal(ui.back.disabled,true);
  ui.refresh.events.click();await turn();
  assert.equal(ui.next.disabled,false);
});
