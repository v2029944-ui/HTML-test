'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
test('installed and GUI-test applications open the canonical index directly',()=>{
 for(const file of ['src-tauri/tauri.conf.json','tests/ci-webview2.json'])assert.equal(JSON.parse(fs.readFileSync(path.join(__dirname,'..',file),'utf8')).app.windows[0].url,'index.html');
});
test('clean rebuild removes obsolete routes and startup preference without touching unrelated files',()=>{
 const output=fs.mkdtempSync(path.join(os.tmpdir(),'single-startup-'));
 try{
  for(const file of ['startup.html','native-workspace.html','tauri-startup.js','tauri-startup-preference.js','tauri-native-reader.js'])fs.writeFileSync(path.join(output,file),'obsolete');
  fs.writeFileSync(path.join(output,'keep.txt'),'unrelated');fs.writeFileSync(path.join(output,'user-file.js'),'unrelated script');prepare({output});
  assert.deepEqual(fs.readdirSync(output).filter(file=>file.endsWith('.html')),['index.html']);
  assert.ok(!fs.existsSync(path.join(output,'tauri-startup.js')));assert.ok(!fs.existsSync(path.join(output,'tauri-native-reader.js')));
  assert.equal(fs.readFileSync(path.join(output,'keep.txt'),'utf8'),'unrelated');assert.equal(fs.readFileSync(path.join(output,'user-file.js'),'utf8'),'unrelated script');
  assert.doesNotMatch(fs.readFileSync(path.join(output,'index.html'),'utf8'),/sqlStartupPreference|startup-preference|native-workspace|legacyWorkspaceLink/);
 }finally{fs.rmSync(output,{recursive:true,force:true});}
});
