const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const {test}=require('node:test');
const html=fs.readFileSync('index.html','utf8');
const begin=html.indexOf('// Explicit ordinary-period evidence');
assert.ok(begin>=0,'explicit ordinary period evidence helper exists');
const ctx={Date};vm.createContext(ctx);
vm.runInContext(html.slice(html.indexOf('function validReportDate('),html.indexOf('function reportIssue('))+
  html.slice(html.indexOf('function excelColumnName('),html.indexOf('function parseSheet('))+
  html.slice(html.indexOf('function addDays('),html.indexOf('function displayDay('))+
  html.slice(begin,html.indexOf('function importBlockMappings(')),ctx);
const infer=(matrix,type='week',mappings=[{col:0,headerRow:matrix.length-1}])=>JSON.parse(JSON.stringify(ctx.ordinaryDateEvidence(matrix,mappings,type)));
test('explicit first-block label yields date and source; arbitrary dates and filename are not inputs',()=>{
 const e=infer([['Начало периода: 09.11.2026'],['Работа','Количество','Сумма']]);
 assert.deepEqual(e.candidates.map(x=>[x.date,x.labelCell,x.dateCell]),[['2026-11-09','A1','A1']]);
 assert.equal(infer([['Дата печати: 19.10.2026'],['Работа','Количество','Сумма']]).candidates.length,0);
});
test('adjacent typed Excel date, monthly date and explicit range',()=>{
 assert.equal(infer([['Дата начала недели',new Date('2026-11-09T00:00:00Z')],[]]).candidates[0].date,'2026-11-09');
 assert.equal(infer([['Начало месяца','2024-02'],[]],'month').candidates[0].date,'2024-02-01');
 assert.equal(infer([['Период с 09.11.2026 по 15.11.2026'],[]]).candidates[0].date,'2026-11-09');
});
test('contradictory starts remain multiple and later blocks never choose first start',()=>{
 const matrix=[['Начало периода: 09.11.2026','','','Начало периода: 16.11.2026'],['Начало периода: 10.11.2026'],[]];
 const e=infer(matrix,'week',[{col:0,headerRow:2},{col:3,headerRow:2}]);
 assert.deepEqual(e.candidates.map(x=>x.date),['2026-11-09','2026-11-10']);
 assert.equal(e.conflict,true);
});
test('short/invalid dates, unstyled numbers and wrong period type are not trusted',()=>{
 for(const value of ['09.11.26','2026-02-30',45000])assert.equal(infer([['Начало периода',value],[]]).candidates.length,0);
 assert.equal(infer([['Начало месяца: 2026-11-01'],[]],'week').candidates.length,0);
 assert.equal(infer([['Период с 09.11.2026 по 30.11.2026'],[]]).candidates.length,0);
});
test('header work and result lists stay bounded and advertise truncation',()=>{
 const matrix=Array.from({length:100},()=>['Начало периода: 09.11.2026']);matrix.push([]);
 const e=infer(matrix);
 assert.equal(e.truncated,true);assert.ok(e.candidates.length<=16);
});
test('ordinary date evidence spans a reordered first column block and stops before another block',()=>{
 const first={col:2,countCol:0,amountCol:1,headerRow:1},second={col:3,countCol:5,amountCol:4,headerRow:1};
 const evidence=infer([['Начало периода','2026-11-09','','Начало периода','2026-11-16'],[]],'week',[first,second]);
 assert.deepEqual(evidence.candidates.map(c=>[c.date,c.dateCell]),[['2026-11-09','B1']]);
});
test('interleaved manual blocks keep their own labels and never borrow adjacent dates from another block',()=>{
 const first={col:4,countCol:0,amountCol:5,headerRow:2},second={col:1,countCol:2,amountCol:3,headerRow:2};
 const evidence=infer([
  ['Начало периода','2026-11-02','Начало периода: 2026-11-16','','Начало периода','2026-11-09'],
  ['','','','Начало периода','2026-11-23'],[]
 ],'week',[first,second]);
 assert.deepEqual(evidence.candidates.map(c=>[c.labelCell,c.dateCell,c.date]),[['E1','F1','2026-11-09']]);
});
