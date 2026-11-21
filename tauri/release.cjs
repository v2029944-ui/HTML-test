'use strict';
const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {spawnSync}=require('node:child_process');
const {releaseNotesFile}=require('./publish.cjs');
const ARTIFACT='Analiz-rabot-Tauri-Windows-x64-setup.exe';
function manifest(version,tag,signature){
  if(!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version))throw Error('Некорректная версия выпуска.');
  if(!/^[0-9A-Za-z._-]+$/.test(tag)||tag==='tauri-updates')throw Error('Укажите отдельный тег версии Tauri.');
  if(!signature.trim())throw Error('Отсутствует подпись установщика.');
  const notes=fs.readFileSync(releaseNotesFile(version),'utf8').trim();
  return {version,notes,pub_date:new Date().toISOString(),platforms:{
    'windows-x86_64':{signature:signature.trim(),url:'https://github.com/v2029944-ui/HTML-test/releases/download/'+tag+'/'+ARTIFACT}
  }};
}
function build(){
  if(process.platform!=='win32')throw Error('Подписанный Windows-выпуск собирается на Windows.');
  const key=process.env.ANALIZ_UPDATER_PUBLIC_KEY?.trim();
  if(!key||!process.env.TAURI_SIGNING_PRIVATE_KEY)throw Error('Задайте ANALIZ_UPDATER_PUBLIC_KEY и TAURI_SIGNING_PRIVATE_KEY. Закрытый ключ не помещайте в репозиторий.');
  const config=JSON.parse(fs.readFileSync(path.join(__dirname,'src-tauri/tauri.conf.json'),'utf8'));
  const pkg=JSON.parse(fs.readFileSync(path.join(__dirname,'package.json'),'utf8'));
  const cargo=fs.readFileSync(path.join(__dirname,'src-tauri/Cargo.toml'),'utf8').match(/^version\s*=\s*"([^"]+)"/m);
  if(pkg.version!==config.version||cargo?.[1]!==config.version)throw Error('Согласуйте версии package.json, Cargo.toml и tauri.conf.json.');
  const tag=process.env.ANALIZ_RELEASE_TAG||'tauri-v'+config.version;
  manifest(config.version,tag,'validation');
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'analiz-release-'));
  try{
    const override=path.join(temp,'release.json');
    fs.writeFileSync(override,JSON.stringify({bundle:{createUpdaterArtifacts:true},plugins:{updater:{pubkey:key}}}));
    require('./prepare.cjs').prepare();
    const cli=path.join(path.dirname(require.resolve('@tauri-apps/cli')),'tauri.js');
    for(const args of [['icon','assets/icon.svg','--output','src-tauri/icons'],['build','--config',override]]){
      const result=spawnSync(process.execPath,[cli,...args],{cwd:__dirname,stdio:'inherit',env:{...process.env,ANALIZ_UPDATER_PUBLIC_KEY:key}});
      if(result.error)throw result.error;
      if(result.status!==0)throw Error('Сборка Tauri завершилась с ошибкой.');
    }
    const bundle=path.join(__dirname,'src-tauri/target/release/bundle/nsis');
    const installers=fs.readdirSync(bundle).filter(name=>name.endsWith('.exe')&&fs.existsSync(path.join(bundle,name+'.sig')));
    if(installers.length!==1)throw Error('Ожидался один подписанный NSIS-установщик. Очистите старые установщики из каталога сборки.');
    const source=path.join(bundle,installers[0]),signature=fs.readFileSync(source+'.sig','utf8');
    const out=path.join(__dirname,'release',tag);fs.mkdirSync(out,{recursive:true});
    fs.copyFileSync(source,path.join(out,ARTIFACT));
    fs.writeFileSync(path.join(out,ARTIFACT+'.sig'),signature);
    fs.writeFileSync(path.join(out,'tauri-latest.json'),JSON.stringify(manifest(config.version,tag,signature),null,2)+'\n');
    process.stdout.write('Подписанный выпуск подготовлен: '+out+'\nПубликация в GitHub не выполнялась.\n');
  }finally{fs.rmSync(temp,{recursive:true,force:true});}
}
if(require.main===module){try{build();}catch(err){console.error(err.message);process.exitCode=1;}}
module.exports={manifest,build,ARTIFACT};
