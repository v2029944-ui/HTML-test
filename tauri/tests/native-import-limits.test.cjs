'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {prepare}=require('../prepare.cjs');
const root=path.resolve(__dirname,'../..');
const read=file=>fs.readFileSync(path.join(root,file),'utf8');

test('native import limits are visible before file choices and match bounded Rust limits',()=>{
 const html=read('tauri/native-workspace.html');
 const limits=read('tauri/import-core/src/lib.rs');
 const kinds=read('tauri/src-tauri/src/reports/metric_reports.rs');
 const mapping=read('tauri/src-tauri/src/reports/metric_xlsx.rs');

 assert.match(limits,/NATIVE_XLSX_ARCHIVE_LIMIT:\s*u64\s*=\s*100\s*\*\s*1024\s*\*\s*1024/);
 assert.match(limits,/archive_bytes:\s*NATIVE_XLSX_ARCHIVE_LIMIT/);
 assert.match(limits,/rows:\s*100_000/);
 assert.match(limits,/columns:\s*2048/);
 assert.match(limits,/cells:\s*15_000_000/);
 assert.match(kinds,/MAX_IMPORT_BYTES:\s*u64\s*=\s*8\s*\*\s*1024\s*\*\s*1024/);
 assert.match(kinds,/MAX_ROWS:\s*usize\s*=\s*10_000/);
 assert.match(mapping,/out\.len\(\)\s*>=\s*metric_reports::MAX_ROWS/);

 const wide=html.slice(html.indexOf('id="wideImportLimits"'),html.indexOf('id="wideChoose"'));
 assert.ok(wide.length>100);
 for(const required of ['100 МиБ','100 000 строк','2 048 столбцов','15 000 000 ячеек','До выбора файла']){
  assert.ok(wide.includes(required),'missing wide import limit '+required);
 }
 const metrics=html.slice(html.indexOf('id="metricImportLimits"'),html.indexOf('id="metricKindFile"'));
 assert.ok(metrics.length>100);
 for(const required of ['30 МиБ','8 МиБ','10 000 строк','До выбора файла']){
  assert.ok(metrics.includes(required),'missing metric import limit '+required);
 }
 assert.ok(html.indexOf('id="metricImportLimits"')>html.indexOf('id="metricKindChooseXlsx"'),
  'limit note is visible in the same import section');
});

test('native limit failures give concrete Russian actions without reducing existing guardrails',()=>{
 const xlsx=read('tauri/src-tauri/src/reports/metric_xlsx.rs');
 const json=read('tauri/src-tauri/src/reports/metric_reports.rs');
 for(const item of [
  [xlsx,/не более 30 МиБ.*уменьшите его размер/],
  [xlsx,/более 10 000 строк.*Уменьшите число строк/],
  [json,/не более 8 МиБ.*уменьшите его размер/],
  [json,/более 10 000 строк.*Уменьшите их число/]
 ])assert.match(item[0],item[1]);
 assert.match(json,/if document\.rows\.len\(\)\s*>\s*MAX_ROWS/);
});

// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
