'use strict';
const test = require('node:test'), assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const code = fs.readFileSync(path.join(__dirname, '../database-backups.js'), 'utf8');
const markup = fs.readFileSync(path.join(__dirname, '../database-panel.html'), 'utf8');
function mount(mode = 'personal') {
  const nodes = new Map(), calls = [], messages = [];
  for (const [, id] of markup.matchAll(/id="([^"]+)"/g)) nodes.set(id, {
    textContent: '', addEventListener(event, fn) { this.click = fn; }
  });
  const state = {mode, available: true, pending: false, workspace_token: 'first'};
  const inputs = {source: '/test/seed.sqlite3', destination: '/test/out.sqlite3', accept: false, scheduled: false};
  const hash = 'a'.repeat(64);
  const api = {
    nativeRestorePending: async () => inputs.scheduled,
    nativeRestoreLaunchInfo: async () => null,
    chooseSqliteBackupPath: async name => { calls.push(['choose', name]); return inputs.destination; },
    nativeBackupSqlite: async file => { calls.push(['backup', file]); return 2; },
    nativeOpenBackupFolder: async file => calls.push(['open', file]),
    chooseRestoreBackupPath: async () => inputs.source,
    nativeRestorePreview: async file => { calls.push(['preview', file]); return {verification: hash, reportCount: 17}; },
    nativeRestoreSchedule: async (...args) => { calls.push(['schedule', ...args]); inputs.scheduled = true; return {restartRequired: true}; },
    nativeRestoreCancel: async () => { calls.push(['cancel']); inputs.scheduled = false; return true; }
  };
  let refresh;
  const ui = {api, $: id => nodes.get(id), snapshot: {reports: 3}, revision: 2, generation: 1,
    onSnapshot(fn) { refresh = fn; }, notice(...args) {messages.push(args);} };
  const window = {unifiedUI: ui, sharedWorkspace: {state}, confirm: async question => { calls.push(['confirm', question]); return inputs.accept; }};
  vm.runInNewContext(code, {window});
  window.databaseBackups.init();
  return {ui, api, inputs, state, calls, messages, hash, refresh, node: ui.$, click: id => nodes.get(id).click()};
}
test('backup is labelled for the selected base; reveal is bound to a successfully saved path', async () => {
  const f = mount('shared'); await f.refresh();
  assert.match(f.node('exportBackup').textContent, /общей/);
  assert.equal(f.node('importBackup').disabled, true);
  assert.equal(f.node('openBackupFolder').hidden, true);
  await f.click('exportBackup'); await f.click('openBackupFolder');
  assert.deepEqual(f.calls, [['choose','analiz-shared.sqlite3'],['backup','/test/out.sqlite3'],['open','/test/out.sqlite3']]);
  assert.match(f.node('databaseBackupStatus').textContent, /общей базы.*\/test\/out.sqlite3/);
  f.inputs.destination = null; await f.click('exportBackup');
  assert.equal(f.node('openBackupFolder').hidden, false);
  f.state.mode='personal'; f.state.workspace_token='second'; await f.refresh();
  assert.equal(f.node('openBackupFolder').hidden, true);
  assert.match(f.node('exportBackup').textContent, /личной/);
});
test('verified preview compares counts, retains a cancelled choice, confirms scheduling and can cancel it', async () => {
  const f = mount(); await f.refresh();
  assert.equal(f.node('scheduleDatabaseRestore').hidden, true);
  await f.click('importBackup'); const summary = f.node('databaseRestoreStatus').textContent;
  assert.match(summary, /базе: 3.*восстановления: 17/);
  f.inputs.source = null; await f.click('importBackup');
  assert.equal(f.node('databaseRestoreStatus').textContent, summary);
  await f.click('scheduleDatabaseRestore');
  assert.equal(f.calls.filter(c=>c[0]==='schedule').length, 0);
  f.inputs.accept = true; await f.click('scheduleDatabaseRestore');
  assert.deepEqual(f.calls.at(-1), ['schedule','/test/seed.sqlite3',f.hash,2]);
  assert.equal(f.node('cancelDatabaseRestore').hidden, false);
  await f.refresh(); assert.equal(f.node('cancelDatabaseRestore').hidden, false);
  await f.click('cancelDatabaseRestore'); assert.match(f.node('databaseRestoreStatus').textContent, /отменено/);
  assert.equal(f.node('cancelDatabaseRestore').hidden, true);
});
test('invalid verification, changed revision and an equal-revision workspace switch invalidate all restore actions', async () => {
  const f = mount(); await f.refresh(); const preview = f.api.nativeRestorePreview;
  f.api.nativeRestorePreview=async()=>({verification:'bad',reportCount:17});
  await f.click('importBackup'); assert.equal(f.node('scheduleDatabaseRestore').hidden,true);
  assert.equal(f.messages.at(-1)[1],true);
  f.api.nativeRestorePreview=preview; await f.click('importBackup');
  f.ui.revision++; await f.refresh(); assert.equal(f.node('scheduleDatabaseRestore').hidden,true);
  let release; f.api.nativeRestorePreview=()=>new Promise(resolve=>release=resolve);
  const waiting=f.click('importBackup'); await new Promise(resolve=>setImmediate(resolve));
  f.state.mode='shared'; f.state.workspace_token='second'; await f.refresh();
  release({verification:f.hash,reportCount:17}); await waiting;
  await f.click('scheduleDatabaseRestore'); assert.equal(f.calls.filter(c=>c[0]==='schedule').length,0);
  assert.equal(f.node('importBackup').disabled,true);
});
test('failed backup or a changed workspace inside the chooser cannot expose a new path', async () => {
  const f=mount(); await f.refresh();
  f.api.nativeBackupSqlite=async()=>{ throw Error('Файл уже существует'); };
  await f.click('exportBackup'); assert.equal(f.node('openBackupFolder').hidden,true);
  f.api.chooseSqliteBackupPath=async()=>{f.state.workspace_token='different';return '/test/other.sqlite3';};
  f.api.nativeBackupSqlite=async()=>assert.fail('stale chooser wrote');
  await f.click('exportBackup'); assert.equal(f.node('openBackupFolder').hidden,true);
  assert.match(f.messages.at(-1)[0],/изменились/);
});
