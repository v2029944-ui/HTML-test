'use strict';
const fs=require('node:fs'),path=require('node:path'),{spawnSync}=require('node:child_process');
function publish(){
  const config=JSON.parse(fs.readFileSync(path.join(__dirname,'src-tauri/tauri.conf.json'),'utf8'));
  const tag='tauri-v'+config.version,repo='v2029944-ui/HTML-test';
  const folder=path.join(__dirname,'release',tag);
  const manifest=JSON.parse(fs.readFileSync(path.join(folder,'tauri-latest.json'),'utf8'));
  const installer='Analiz-rabot-Tauri-Windows-x64-setup.exe';
  const artifact=manifest.platforms?.['windows-x86_64'];
  if(manifest.version!==config.version||artifact?.url!==`https://github.com/${repo}/releases/download/${tag}/${installer}`||!artifact.signature)
    throw Error('Манифест не соответствует публикуемому установщику.');
  for(const file of [installer,installer+'.sig'])if(!fs.statSync(path.join(folder,file)).size)throw Error('Пустой артефакт выпуска.');
  const gh=args=>{
    const result=spawnSync('gh',args,{encoding:'utf8',env:process.env});
    if(result.error)throw result.error;
    if(result.status!==0)throw Error(result.stderr||'Ошибка GitHub CLI');
    return result.stdout;
  };
  const releases=JSON.parse(gh(['api','--paginate','--slurp',`repos/${repo}/releases?per_page=100`])).flat();
  if(releases.some(r=>r.tag_name===tag)){
    // Version assets are immutable: retries must never replace code under an existing version.
    const files=JSON.parse(gh(['release','view',tag,'--repo',repo,'--json','assets'])).assets;
    if(!files.some(f=>f.name===installer)||!files.some(f=>f.name===installer+'.sig'))
      throw Error('Неполный выпуск уже существует. Проверьте его вручную или увеличьте версию.');
    const tmp=fs.mkdtempSync(path.join(require('node:os').tmpdir(),'analiz-release-check-'));
    try{
      gh(['release','download',tag,'--repo',repo,'--pattern',installer+'.sig','--dir',tmp]);
      if(fs.readFileSync(path.join(tmp,installer+'.sig'),'utf8').trim()!==artifact.signature.trim())
        throw Error('Версия уже опубликована с другой подписью. Увеличьте версию; прежний выпуск не изменён.');
    }finally{fs.rmSync(tmp,{recursive:true,force:true});}
  }else{
    gh(['release','create',tag,path.join(folder,installer),path.join(folder,installer+'.sig'),
      '--repo',repo,'--target',process.env.GITHUB_SHA||'HEAD','--title','Tauri '+config.version,
      '--notes','Экспериментальная подписанная Windows-версия. Исходные Excel и пользовательская база не включены.',
      '--prerelease','--latest=false']);
  }
  // Advance the channel only after the versioned installer is available.
  if(!releases.some(r=>r.tag_name==='tauri-updates'))
    gh(['release','create','tauri-updates','--repo',repo,'--target',process.env.GITHUB_SHA||'HEAD',
      '--title','Tauri update channel','--notes','Signed update manifest for Tauri Windows builds.','--prerelease','--latest=false']);
  gh(['release','upload','tauri-updates',path.join(folder,'tauri-latest.json'),'--repo',repo,'--clobber']);
  console.log('Published '+tag+' and advanced the Tauri update channel.');
}
if(require.main===module){try{publish();}catch(err){console.error(err.message);process.exitCode=1;}}
module.exports={publish};
