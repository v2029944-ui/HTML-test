'use strict';
// Always-on PR check: blocks accidental upload of local data, keys and EXEs.
const {execFileSync}=require('node:child_process');
const path=require('node:path');
function blocked(file){
 const p=String(file).replace(/\\/g,'/').toLowerCase();
 const name=path.posix.basename(p);
 const fixture=/^(?:tests|tauri\/tests)\/fixtures\//.test(p);
 if(name==='.env'||(name.startsWith('.env.')&&name!=='.env.example'))return true;
 if(/\.(?:sqlite3?|db)(?:-(?:wal|shm))?$/.test(name))return true;
 if(/\.(?:pem|p12|pfx|key|exe|msi|msix|sig)$/.test(name))return true;
 if(/\.(?:xlsx|xlsm|xls)$/.test(name)&&!fixture)return true;
 if(p.startsWith('tauri/release/')&&name!=='readme.md')return true;
 if(p.startsWith('tauri/web/'))return true;
 return false;
}
function files(base,head){
 if(!/^[a-f0-9]{40}$/.test(base)||!/^[a-f0-9]{40}$/.test(head))
  throw Error('Expected exact commit SHAs');
 return execFileSync('git',['diff','--name-only','--diff-filter=ACMR','-z',base+'...'+head],
  {encoding:'utf8',maxBuffer:10*1024*1024}).split('\0').filter(Boolean);
}
if(require.main===module){
 const changed=files(process.env.PR_BASE_SHA||'',process.env.PR_HEAD_SHA||'');
 const forbidden=changed.filter(blocked);
 if(forbidden.length){
  console.error('Forbidden data, signing credentials or release artifacts:\n'+forbidden.join('\n'));
  process.exitCode=1;
 }else console.log('PR safety: '+changed.length+' changed files, no forbidden artifacts.');
}
module.exports={blocked,files};
