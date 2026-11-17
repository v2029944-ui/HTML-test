'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const {backupManifest,BASE}=require('../backup-manifest.cjs');
test('Yandex manifest includes the exact signature and Yandex-hosted versioned EXE',()=>{
  const m=backupManifest('0.1.2','actual-signature\n');
  assert.equal(m.version,'0.1.2');
  assert.equal(m.platforms['windows-x86_64'].signature,'actual-signature');
  assert.equal(m.platforms['windows-x86_64'].url,
    BASE+'/tauri-v0.1.2/Analiz-rabot-Tauri-Windows-x64-setup.exe');
  assert.ok(!m.platforms['windows-x86_64'].url.includes('github.com'));
});
test('malformed version and missing signature are rejected',()=>{
  assert.throws(()=>backupManifest('not a version','valid signature'));
  assert.throws(()=>backupManifest('0.1.2','   '));
});
