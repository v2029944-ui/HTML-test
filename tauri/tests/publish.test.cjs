'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
function fixture({existing=false,signature='signature',failInstaller=false,missingNotes=false,placeholderNotes=false}={}){
  const commands=[],repo='v2029944-ui/HTML-test',version='0.1.1',installer='Analiz-rabot-Tauri-Windows-x64-setup.exe';
  const api={readFileSync(file){
    if(file.endsWith('tauri.conf.json'))return JSON.stringify({version});
    if(file.endsWith('tauri-latest.json'))return JSON.stringify({version,platforms:{'windows-x86_64':{signature:'signature',url:`https://github.com/${repo}/releases/download/tauri-v${version}/${installer}`}}});
    if(file.endsWith('/0.1.1.md')){
      if(missingNotes)throw Error('ENOENT');
      return placeholderNotes?'# TODO\\n- заполнить':'# Что изменилось\\n- Исправлена обработка повторного ввода Excel.\\n- Улучшена проверка резервного копирования пользовательской базы.';
    }
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

test('release publication refuses missing notes before contacting GitHub',()=>{
 const f=fixture({missingNotes:true});
 assert.throws(f.run,/необходим tauri\\/release-notes/);
 assert.equal(f.commands.length,0);
});
test('release publication refuses placeholder-only notes',()=>{
 const f=fixture({placeholderNotes:true});
 assert.throws(f.run,/Release notes должны/);
 assert.equal(f.commands.length,0);
});
test('versioned release uses the exact changelog file rather than generic notes',()=>{
 const f=fixture();f.run();
 const creation=f.commands.find(x=>x[1]==='create'&&x[2]==='tauri-v0.1.1');
 assert.ok(creation);
 const index=creation.indexOf('--notes-file');
 assert.ok(index>0);
 assert.match(creation[index+1],/release-notes[\\/]0\\.1\\.1\\.md$/);
 assert.equal(creation.includes('--notes'),false);
});
