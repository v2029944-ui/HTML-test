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

test('default remains classic and preference changes launch to SQL without loading classic first',()=>{
 const store=storage();
 assert.deepEqual(runStartup(store).routes,['index.html']);
 const ui=runPreference(store);
 assert.equal(ui.checkbox.checked,false);
 ui.checkbox.checked=true;ui.change();
 assert.equal(store.getItem('analiz-sql-startup-v1'),'sql');
 assert.deepEqual(runStartup(store).routes,['native-workspace.html']);
 const reopened=runPreference(store);
 assert.equal(reopened.checkbox.checked,true);
 reopened.checkbox.checked=false;reopened.change();
 assert.equal(store.getItem('analiz-sql-startup-v1'),null);
 assert.deepEqual(runStartup(store).routes,['index.html']);
});

test('storage rejection falls back to the classic UI and disables the unsafe preference',()=>{
 const forbidden={getItem(){throw Error('disabled')},setItem(){throw Error('disabled')},removeItem(){throw Error('disabled')}};
 assert.deepEqual(runStartup(forbidden).routes,['index.html']);
 const ui=runPreference(forbidden);
 assert.equal(ui.checkbox.disabled,true);
 assert.match(ui.status.textContent,/недоступна/);
});

test('failed preference write leaves the previous route and reports the error',()=>{
 const forbidden={getItem(){return null},setItem(){throw Error('no write')},removeItem(){throw Error('no write')}};
 const ui=runPreference(forbidden);
 ui.checkbox.checked=true;ui.change();
 assert.equal(ui.checkbox.checked,false);
 assert.match(ui.status.textContent,/Не удалось сохранить/);
 assert.deepEqual(runStartup(forbidden).routes,['index.html']);
});
