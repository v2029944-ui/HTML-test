'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {userError}=require('../native-errors.js');
const {prepare}=require('../prepare.cjs');

test('native error messages translate known Rust and XLSX failures into actions',()=>{
 const cases=[
  ['Management session disk quota exceeded','Превышен допустимый объём временных данных'],
  ['Session size overflow','Объём временных данных слишком велик'],
  ['Management numeric overflow','Получено слишком большое числовое значение'],
  ['No active organization','Не выбрано подразделение'],
  ['XLSX header mismatch','Заголовки Excel не соответствуют выбранному шаблону'],
  ['Metric aggregation overflow','При расчёте показателей получено слишком большое значение'],
  ['source changed','Файл изменился после проверки'],
  ['A verified SQLite backup destination is required','резервной копии базы'],
  ['database is locked','База данных занята'],
  ['mismatch','Данные не совпадают'],
 ];
 for(const [technical,expected] of cases){
  const message=userError(new Error(technical));
  assert.match(message,new RegExp(expected));
  assert.ok(!message.includes(technical),'internal error must not reach UI: '+technical);
 }
});
test('native error messages keep validated cell references and safe Russian context',()=>{
 const message=userError(new Error('Error: XLSX header mismatch at Data!E27'));
 assert.match(message,/Ячейка Excel: E27/);
 assert.ok(!message.includes('Data!'),'untrusted sheet text must not be echoed');
 const original='Ревизия базы изменилась во время резервного копирования';
 assert.equal(userError(new Error(original)),original);
});
test('unknown and sensitive technical errors fail closed',()=>{
 for(const error of [new Error(''),new Error('sqlite internals with secret=abc'),new Error('SQL SELECT * FROM reports'),null,{},'Stack trace: /secret/path']){
  const translated=userError(error);
  assert.match(translated,/Операция не выполнена/);
  assert.ok(!/secret=abc|SELECT \*|Stack trace/i.test(translated));
 }
});
test('native workspace bundles the error translator before its consumers, not in standalone HTML',()=>{
 const root=path.resolve(__dirname,'../..'),out=fs.mkdtempSync(path.join(os.tmpdir(),'native-errors-'));
 try{
  prepare({base:root,output:out});
  const html=fs.readFileSync(path.join(out,'native-workspace.html'),'utf8');
  const translator=html.indexOf('tauri-native-errors.js');
  const workspace=html.indexOf('tauri-native-workspace.js');
  const wide=html.indexOf('tauri-native-wide-import.js');
  const metrics=html.indexOf('tauri-native-metric-kinds.js');
  assert.ok(translator>=0&&translator<workspace&&workspace<wide&&wide<metrics);
  assert.ok(fs.existsSync(path.join(out,'tauri-native-errors.js')));
  assert.ok(!fs.readFileSync(path.join(root,'index.html'),'utf8').includes('tauri-native-errors.js'));
 }finally{fs.rmSync(out,{recursive:true,force:true});}
});
