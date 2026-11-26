'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const {blocked,files}=require('./pr-safety.cjs');
test('rejects sensitive and distributable files',()=>{
 for(const p of ['data.sqlite3','data.sqlite3-wal','data.db-shm','profile/active.sqlite-journal','.env.local','secret.pem',
  'credentials/id_ed25519','store.keystore','key.ppk','private.jks',
  'share/analiz-generation-00000000000000000001.snapshot','share/pending.snapshot',
  'reports.xlsx','tauri/release/setup.exe','downloads/SETUP.EXE','setup.exe.sig','tauri/web/generated.js'])
  assert.equal(blocked(p),true,p);
});
test('allows code and synthetic test fixtures',()=>{
 for(const p of ['AGENTS.md','docs/ROADMAP.md','tauri/src-tauri/src/main.rs','.env.example',
 'credentials/id_ed25519.pub','tests/fixtures/synthetic.xlsx','tauri/tests/fixtures/synthetic.xlsm','tauri/release/README.md'])
  assert.equal(blocked(p),false,p);
});
test('validates exact SHA strings',()=>assert.throws(()=>files('main','HEAD'),/exact commit SHAs/));
