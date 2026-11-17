'use strict';
const fs=require('node:fs');
const path=require('node:path');
const {manifest,ARTIFACT}=require('./release.cjs');
const BASE='https://storage.yandexcloud.net/html-test-updates-23727';

function backupManifest(version,signature) {
  const payload=manifest(version,'tauri-v'+version,signature);
  payload.platforms['windows-x86_64'].url=BASE+'/tauri-v'+version+'/'+ARTIFACT;
  return payload;
}

function create(args) {
  if(args.length!==3)throw Error(
    'Использование: node tauri/backup-manifest.cjs <версия> <путь-к-setup.exe.sig> <путь-к-tauri-latest.json>'
  );
  const [version,sigPath,output]=args;
  const signature=fs.readFileSync(sigPath,'utf8');
  const payload=backupManifest(version,signature);
  // The signature must come from the exact, unmodified release installer.
  // Do not copy the GitHub manifest unchanged: its download URL points back to GitHub.
  fs.mkdirSync(path.dirname(output),{recursive:true});
  fs.writeFileSync(output,JSON.stringify(payload,null,2)+'\n');
  process.stdout.write('Резервный манифест создан: '+output+'\n');
}
if(require.main===module){try{create(process.argv.slice(2));}catch(err){console.error(err.message);process.exitCode=1;}}
module.exports={BASE,backupManifest,create};
