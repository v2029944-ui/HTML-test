'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
function fixture({existing=false,signature='signature',failInstaller=false}={}){
  const commands=[],repo='v2029944-ui/HTML-test',version='0.1.1',installer='Analiz-rabot-Tauri-Windows-x64-setup.exe';
  const api={readFileSync(file){
    if(file.endsWith('tauri.conf.json'))return JSON.stringify({version});
    if(file.endsWith('tauri-latest.json'))return JSON.stringify({version,platforms:{'windows-x86_64':{signature:'signature',url:`https://github.com/${repo}/releases/download/tauri-v${version}/${installer}`}}});
    return signature;
  },statSync:()=>({size:1}),mkdtempSync:()=>'/tmp/test-release',rmSync(){}};
  const spawnSync=(_bin,args)=>{
    commands.push(args);
    if(failInstaller&&args[0]==='release'&&args[1]==='create'&&args[2]==='tauri-v'+version)return {status:1,stderr:'Upload failed'};
    let value='';
    if(args[0]==='api')value=JSON.stringify(existing?[[{tag_name:'tauri-v'+version},{tag_name:'tauri-updates'}]]:[[]]);
    if(args[0]==='release'&&args[1]==='view')value=JSON.stringify({assets:[{name:installer},{name:installer+'.sig'}]});
    return {status:0,stdout:value};
  };
  const module={exports:{}},ctx={module,__dirname:'/repo/tauri',console:{log(){}},process:{env:{GITHUB_SHA:'commit'}},require:name=>({
    'node:fs':api,'node:path':path,'node:os':{tmpdir:()=>'/tmp'},'node:child_process':{spawnSync}
  })[name]};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../publish.cjs'),'utf8'),ctx);
  return {run:module.exports.publish,commands};
}
test('publish versioned installer before advancing the separate update channel',()=>{
  const f=fixture();f.run();
  const create=f.commands.findIndex(x=>x[1]==='create'&&x[2]==='tauri-v0.1.1');
  const advance=f.commands.findIndex(x=>x[1]==='upload'&&x[2]==='tauri-updates');assert.ok(create<advance);
  assert.ok(f.commands.filter(x=>x[1]==='create').every(x=>x.includes('--prerelease')&&x.includes('--latest=false')));
});
test('failed installer publication must not change the update channel',()=>{
  const f=fixture({failInstaller:true});assert.throws(f.run,/Upload failed/);
  assert.equal(f.commands.some(x=>x[1]==='upload'),false);
});
test('retry never replaces an installer already published with another signature',()=>{
  const f=fixture({existing:true,signature:'different'});assert.throws(f.run,/другой подписью/);
  assert.equal(f.commands.some(x=>x[1]==='upload'),false);
  const same=fixture({existing:true});same.run();assert.equal(same.commands.filter(x=>x[1]==='create').length,0);
});

test('current Tauri version is synchronized and has concrete versioned release notes',()=>{
 const config=JSON.parse(fs.readFileSync(path.join(__dirname,'../src-tauri/tauri.conf.json'),'utf8'));
 const pkg=JSON.parse(fs.readFileSync(path.join(__dirname,'../package.json'),'utf8'));
 const cargo=fs.readFileSync(path.join(__dirname,'../src-tauri/Cargo.toml'),'utf8');
 const v=cargo.match(/^version\\s*=\\s*"([^"]+)"/m);
 assert.ok(v,'Cargo package version must be defined');
 assert.equal(config.version,pkg.version);
 assert.equal(config.version,v[1]);
 const filename=path.join(__dirname,'../release-notes',config.version+'.md');
 assert.ok(fs.existsSync(filename),'Every candidate needs version-specific changelog');
 const notes=fs.readFileSync(filename,'utf8');
 assert.match(notes,/^#\\s+.+/m);
 assert.match(notes,/^[-*]\\s+.+/m);
 assert.match(notes,/## Что добавлено/);
 assert.match(notes,/## Обновление и ограничения/);
 assert.ok(notes.length>400,'No generic placeholder is allowed for a release');
});
