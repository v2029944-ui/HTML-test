'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const {buildUnified} = require('../unified-build.cjs');

const original = fs.readFileSync(path.resolve(__dirname, '../../index.html'), 'utf8');
const {html, script} = buildUnified(original);

test('installed reference screens and detail tools share the same native forms', () => {
  for (const id of ['s1','s2','reportShell','refNative','toolsPage','sheet','upload','groups','storage']) {
    assert.equal(html.split('id="' + id + '"').length - 1, 1, id);
  }
  assert.match(html, /id="refNative"><header class="topbar">/);
  assert.doesNotMatch(html, /id="refData"|id="refWorkspaceDialog"|Исходный макет|<iframe\b|ref-day-2026|Демоданные|3 772 233,26|mountEngine\(/);
  const boot = html.indexOf('src="tauri-unified-app.js"');
  for (const name of ['reference-calendar','reference-detail','reference-results','reference-filters','reference-desktop']) {
    const offset = html.indexOf('src="tauri-' + name + '.js"');
    assert.ok(offset !== -1 && offset < boot);
  }
});

test('desktop starts in reference colors and honors previously selected appearance', () => {
  const context = {window:{}};
  vm.runInNewContext(script, context);
  const theme = args => vm.runInNewContext('appearancePreferences(' + args + ').theme', context);
  assert.equal(theme('null,null,null'), 'dark');
  assert.equal(theme('"system","minimal","modern"'), 'system');
  assert.equal(theme('"light","paper","compact"'), 'light');
});


test('main change metric is read only and neighboring periods have a distinct heading',()=>{
 assert.doesNotMatch(html,/data-change-metric=/);assert.match(html,/data-current-change-metric=/);
 assert.match(html,/<h2 id="changesHeading">Изменения между соседними периодами<\/h2>/);
 assert.match(html,/<div class="changes-heading"><h2[^>]*>Значимые изменения<\/h2>/);
});

test('desktop compact scales match the picker and keep existing preference values', () => {
  const context = {window:{}};
  vm.runInNewContext(script, context);
  const options = html.match(/<select id="appearanceSize">([\s\S]*?)<\/select>/)[1];
  const values = Array.from(options.matchAll(/value="([^"]+)"/g), match => match[1]);
  assert.deepEqual(values, ['80', '90', 'standard', 'large', 'xlarge']);
  for (const value of values) {
    assert.equal(vm.runInNewContext(`uiSizePreference(${JSON.stringify(value)})`, context), value);
  }
  assert.equal(vm.runInNewContext('uiSizePreference(null)', context), 'standard');
  assert.equal(html.split('id="appVersion"').length - 1, 1);
  assert.match(html, /id="onlineUpdates"[^>]*>[\s\S]*?Приложение и обновления<\/h2>\s*<span id="appVersion"/);
});
