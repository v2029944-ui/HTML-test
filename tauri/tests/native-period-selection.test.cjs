'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const source=fs.readFileSync(path.join(__dirname,'../native-period-selection.js'),'utf8');
function mount(){
 const ids=['analysisFrom','analysisTo','comparisonBeforeFrom','comparisonBeforeTo','analysisPeriodHelp'];
 const nodes=Object.fromEntries(ids.map(id=>[id,{value:'',type:'date',textContent:''}]));
 const window={},document={getElementById:id=>nodes[id]};
 vm.runInNewContext(source,{window,document});
 return {picker:window.nativePeriodSelection,nodes};
}
test('native calendar selector is packaged and used only by the Tauri SQL workspace',()=>{
 const base=path.resolve(__dirname,'../..'),output=fs.mkdtempSync(path.join(os.tmpdir(),'sql-calendar-picker-'));
 try{
  prepare({base,output});
  const html=fs.readFileSync(path.join(output,'native-workspace.html'),'utf8');
  assert.equal(fs.readFileSync(path.join(output,'tauri-native-period-selection.js'),'utf8'),source);
  assert.ok(html.indexOf('tauri-native-period-selection.js')<html.indexOf('tauri-native-workspace.js'));
  assert.match(html,/id="analysisPeriodHelp"/);
  assert.ok(!fs.readFileSync(path.join(base,'index.html'),'utf8').includes('tauri-native-period-selection.js'));
 }finally{fs.rmSync(output,{recursive:true,force:true});}
});
test('calendar months and ISO calendar weeks resolve to actual period starts',()=>{
 const {picker}=mount();
 assert.equal(picker.normalize('month','2024-02'),'2024-02-01');
 assert.equal(picker.normalize('month','2024-13'),null);
 assert.equal(picker.normalize('day','2024-02-29'),'2024-02-29');
 assert.equal(picker.normalize('day','2023-02-29'),null);
 assert.equal(picker.normalize('day','2024-02-30'),null);
 assert.equal(picker.normalize('week','2024-W09'),'2024-02-26');
 assert.equal(picker.normalize('week','2025-W01'),'2024-12-30');
 assert.equal(picker.normalize('week','2020-W53'),'2020-12-28');
 assert.equal(picker.normalize('week','2021-W53'),null);
 assert.equal(picker.normalize('week','2024-W00'),null);
 assert.equal(picker.normalize('week','2024-W99'),null);
 assert.equal(picker.normalize('year','2024'),'2024-01-01');
 assert.equal(picker.fromIsoDate('2024-12-30','week'),'2025-W01');
 assert.equal(picker.fromIsoDate('2024-02-29','month'),'2024-02');
});
test('switching calendar granularity retains equivalent dates and baseline controls',()=>{
 const {picker,nodes}=mount();
 nodes.analysisFrom.value='2024-02-29';
 nodes.analysisTo.value='2024-03-31';
 nodes.comparisonBeforeFrom.value='2024-01-02';
 nodes.comparisonBeforeTo.value='2024-02-15';
 picker.apply('week');
 assert.equal(nodes.analysisFrom.type,'week');
 assert.equal(nodes.analysisFrom.value,'2024-W09');
 assert.equal(nodes.analysisTo.value,'2024-W13');
 assert.equal(nodes.comparisonBeforeFrom.value,'2024-W01');
 assert.equal(picker.normalize('week',nodes.analysisFrom.value),'2024-02-26');
 picker.apply('month');
 assert.equal(nodes.analysisFrom.type,'month');
 assert.equal(nodes.analysisFrom.value,'2024-02');
 assert.equal(nodes.analysisTo.value,'2024-03');
 assert.equal(nodes.comparisonBeforeTo.value,'2024-02');
 assert.match(nodes.analysisPeriodHelp.textContent,/Выбирайте месяцы/);
 picker.apply('day');
 assert.equal(nodes.analysisFrom.type,'date');
 assert.equal(nodes.analysisFrom.value,'2024-02-01');
 assert.equal(nodes.analysisTo.value,'2024-03-01');
});
test('daily-source ranges cover full selected calendar periods and do not use other source types',()=>{
 const {picker}=mount();
 assert.equal(picker.range('month','2024-02','2024-02','daily').type,'day');
 assert.equal(picker.range('month','2024-02','2024-02','daily').to,'2024-02-29');
 assert.equal(picker.range('week','2025-W01','2025-W01','daily').from,'2024-12-30');
 assert.equal(picker.range('week','2025-W01','2025-W01','daily').to,'2025-01-05');
 assert.equal(picker.range('year','2024','2024','daily').to,'2024-12-31');
 assert.equal(picker.range('month','2024-02','2024-02','imported').type,'month');
 assert.equal(picker.range('month','2024-02','2024-02','imported').to,'2024-02-01');
 assert.equal(picker.range('year','2024','2024','imported'),null);
 assert.equal(picker.range('day','2024-02-30','2024-03-01','daily'),null);
});
