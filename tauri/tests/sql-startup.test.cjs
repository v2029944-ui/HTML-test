'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const os=require('node:os');
const {prepare,prepareHtml}=require('../prepare.cjs');

const root=path.resolve(__dirname,'../..');
const startupCode=fs.readFileSync(path.join(__dirname,'../startup.js'),'utf8');
const preferenceCode=fs.readFileSync(path.join(__dirname,'../startup-preference.js'),'utf8');

function storage(){
 const records=new Map();
 return {
   getItem:key=>records.has(key)?records.get(key):null,
   setItem:(key,value)=>records.set(key,String(value)),
   removeItem:key=>records.delete(key)
 };
}
function runStartup(localStorage){
 const routes=[],message={textContent:'loading'};
 const window={localStorage,location:{replace:target=>routes.push(target)}};
 vm.runInNewContext(startupCode,{window,document:{getElementById:()=>message}});
 return {routes,message};
}
function runPreference(localStorage){
 const events={};
 const checkbox={checked:false,disabled:false,addEventListener:(name,callback)=>events[name]=callback};
 const status={textContent:''};
 vm.runInNewContext(preferenceCode,{
   window:{localStorage},
   document:{getElementById:id=>id==='sqlStartupPreference'?checkbox:id==='sqlStartupPreferenceStatus'?status:null}
 });
 return {checkbox,status,change:()=>events.change()};
}

test('Tauri entry is a lightweight launcher, without booting the report history',()=>{
 const config=JSON.parse(fs.readFileSync(path.join(root,'tauri/src-tauri/tauri.conf.json'),'utf8'));
 const gui=JSON.parse(fs.readFileSync(path.join(root,'tauri/tests/ci-webview2.json'),'utf8'));
 assert.equal(config.app.windows[0].url,'startup.html');
 assert.equal(gui.app.windows[0].url,'startup.html');
 const output=fs.mkdtempSync(path.join(os.tmpdir(),'sql-startup-'));
 try{
   prepare({base:root,output});
   const startup=fs.readFileSync(path.join(output,'startup.html'),'utf8');
   const sql=fs.readFileSync(path.join(output,'native-workspace.html'),'utf8');
   const browserHtml=fs.readFileSync(path.join(root,'index.html'),'utf8');
   const classic=fs.readFileSync(path.join(output,'index.html'),'utf8');
   assert.match(startup,/src="tauri-startup\.js"/);
   assert.match(startup,/href="index\.html"/);
   assert.match(startup,/href="native-workspace\.html"/);
   assert.doesNotMatch(startup,/appReady=main\(\)|tauri-bridge\.js|load_data/);
   assert.equal(classic,prepareHtml(browserHtml));
   assert.match(sql,/id="sqlStartupPreference"/);
   assert.match(sql,/src="tauri-startup-preference\.js"/);
   assert.ok(fs.existsSync(path.join(output,'tauri-startup.js')));
   assert.ok(fs.existsSync(path.join(output,'tauri-startup-preference.js')));
   assert.ok(!browserHtml.includes('tauri-startup.js'),'Standalone HTML must remain independent');
 }finally{fs.rmSync(output,{recursive:true,force:true});}
});

test('missing preference launches SQL even when classic Excel view is remembered',()=>{
 const store=storage();store.setItem('work-reports-view-v1','upload');
 assert.deepEqual(runStartup(store).routes,['native-workspace.html']);
 assert.equal(runPreference(store).checkbox.checked,true);
});

test('explicit classic choice persists and SQL can be restored as the startup screen',()=>{
 const store=storage(),ui=runPreference(store);
 assert.equal(ui.checkbox.checked,true);
 ui.checkbox.checked=false;ui.change();
 assert.equal(store.getItem('analiz-sql-startup-v1'),'classic');
 assert.deepEqual(runStartup(store).routes,['index.html']);
 const reopened=runPreference(store);assert.equal(reopened.checkbox.checked,false);
 reopened.checkbox.checked=true;reopened.change();
 assert.equal(store.getItem('analiz-sql-startup-v1'),'sql');
 assert.deepEqual(runStartup(store).routes,['native-workspace.html']);
 assert.equal(runPreference(store).checkbox.checked,true);
});

test('existing SQL and unknown preferences select the primary SQL screen',()=>{
 for(const value of ['sql','unexpected']){
  const store=storage();store.setItem('analiz-sql-startup-v1',value);
  assert.deepEqual(runStartup(store).routes,['native-workspace.html']);
  assert.equal(runPreference(store).checkbox.checked,true);
 }
});

test('storage rejection retains SQL startup and disables the unavailable preference',()=>{
 const forbidden={getItem(){throw Error('disabled')},setItem(){throw Error('disabled')}};
 assert.deepEqual(runStartup(forbidden).routes,['native-workspace.html']);
 const ui=runPreference(forbidden);
 assert.equal(ui.checkbox.disabled,true);assert.equal(ui.checkbox.checked,true);
 assert.match(ui.status.textContent,/недоступна/);
});

test('failed preference write restores the checkbox and keeps the previous route',()=>{
 for(const previous of [null,'classic']){
  const forbidden={getItem(){return previous},setItem(){throw Error('no write')}};
  const ui=runPreference(forbidden),was=ui.checkbox.checked;
  ui.checkbox.checked=!was;ui.change();
  assert.equal(ui.checkbox.checked,was);
  assert.match(ui.status.textContent,/Не удалось сохранить/);
  assert.deepEqual(runStartup(forbidden).routes,[previous==='classic'?'index.html':'native-workspace.html']);
 }
});
