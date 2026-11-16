'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(require('node:path').join(__dirname,'../tauri/folder-importer.js'),'utf8');
const functions=source.slice(source.indexOf('  function parseExcel('),source.indexOf('  function scheduleSync('));
const draft=(date='2024-01-01')=>({type:'day',start:date,end:date,fileName:'year.xlsx',sheetName:'Q1',rows:[{work:'A',count:0,amount:0}],issues:[],warnings:[],hasMismatch:false,count:0,amount:0});
function setup(options={}){
 const state={reports:options.previous||[],folderImport:null};
 const rule=options.rule||{format:'daily-wide',type:'day',start:'',year:2024,sheets:['Q1','Q2'],mappings:{}};
 const files=options.files||[{name:'year.xlsx',signature:'new'}];
 const calls=[];
 const ctx={window:{XLSX:{read:()=>({SheetNames:['Q1','Q2'],Sheets:{Q1:{},Q2:{}}})}},state,
 importRuleKey:rule=>JSON.stringify(rule),findDailySheets:()=>['Q1','Q2'],parseDailyWorkbook:(book,opts)=>{calls.push(opts);return options.result||{reports:[draft(),draft('2024-04-01')]};},
 validReportDate:x=>/^\d{4}-\d{2}-\d{2}$/.test(x),uuid:()=>Math.random().toString(),
 running:false,rerun:false,choose:{},refresh:{},log:{replaceChildren(){},append(){}},
 message(){},node(){return {};},displayPeriod:r=>r.start,renderAll(){},scheduleSync(){},
 api:{list:async()=>({folder:'/reports',files,rules:{'year.xlsx':rule}}),readExcel:async()=>({signature:'new',sha256:'hash',bytes:[]}),checkExcel:async()=>options.stable!==false},
 storeReports:async reports=>{state.reports=reports;ctx.saved=reports;},
 Date,Set,JSON,Uint8Array};
 vm.createContext(ctx);vm.runInContext(functions,ctx);return {ctx,state,rule,calls};
}
test('daily-wide passes selected sheets/year/mappings and attaches source metadata',()=>{
 const {ctx,rule,calls}=setup();const result=ctx.parseExcel({name:'year.xlsx'},rule,[],'sig','hash');
 assert.equal(result.length,2);assert.deepEqual(calls[0].sheets,['Q1','Q2']);assert.equal(calls[0].year,2024);
 assert.equal(result[0].count,0);assert.equal(result[0].desktopSignature,'sig');assert.equal(result[0].desktopHash,'hash');
 assert.equal(result[0].desktopRule,JSON.stringify(rule));assert.ok(result[0].importedAt);
});
test('structural detection enables daily route only in automatic mode',()=>{
 const {ctx}=setup();assert.equal(ctx.parseExcel({name:'year.xlsx'},{format:'auto',type:'week',start:'2024-01-01'},[],'sig','hash').length,2);
});
for(const result of [{reports:[{...draft(),warnings:['blank amount']}]},{reports:[draft()],issues:['source total']},{reports:[draft()],hasMismatch:true},{reports:[draft(),draft()]},{reports:[]}]){
 test('unsafe daily result is rejected atomically: '+JSON.stringify(result).slice(0,80),()=>{
  const {ctx,rule}=setup({result});assert.throws(()=>ctx.parseExcel({name:'year.xlsx'},rule,[],'sig','hash'));
 });
}
test('sync replaces all days of one source together',async()=>{
 const previous=[{...draft('2023-01-01'),desktopSource:'year.xlsx'}];const {ctx,state}=setup({previous});
 await ctx.syncFiles();assert.equal(state.reports.length,2);assert.equal(state.reports[0].start,'2024-01-01');
});
test('conflict with another source preserves prior complete snapshot',async()=>{
 const previous=[{...draft('2023-01-01'),desktopSource:'year.xlsx'},{...draft(),desktopSource:'other.xlsx'}];
 const {ctx,state}=setup({previous});await ctx.syncFiles();assert.equal(ctx.saved,undefined);assert.equal(state.reports,previous);
});
test('file changed during copy preserves prior snapshot',async()=>{
 const previous=[{...draft('2023-01-01'),desktopSource:'year.xlsx'}];const {ctx,state}=setup({previous,stable:false});
 await ctx.syncFiles();assert.equal(ctx.saved,undefined);assert.equal(state.reports,previous);
});
test('missing source preserves reports',async()=>{
 const previous=[{...draft(),desktopSource:'year.xlsx'}];const {ctx,state}=setup({previous,files:[]});
 await ctx.syncFiles();assert.equal(ctx.saved,undefined);assert.equal(state.reports,previous);
});
test('unchanged source is not reparsed',async()=>{
 const rule={format:'daily-wide',type:'day',start:''};
 const previous=[{...draft(),desktopSource:'year.xlsx',desktopSignature:'new',desktopRule:JSON.stringify(rule)}];
 const {ctx,calls}=setup({previous,rule});await ctx.syncFiles();assert.equal(calls.length,0);assert.equal(ctx.saved,undefined);
});

test('same file name in a different folder cannot replace another source without permission',async()=>{
 const previous=[{...draft(),desktopSource:'year.xlsx',desktopFolder:'/other-folder'}];const {ctx,state}=setup({previous});await ctx.syncFiles();
 assert.equal(ctx.saved,undefined);assert.equal(state.reports,previous);
});
test('proven zero day without detail rows is accepted as a report',()=>{
 const {ctx,rule}=setup({result:{reports:[{...draft(),rows:[],explicitZero:true}]}});assert.equal(ctx.parseExcel({name:'year.xlsx'},rule,[],'sig','hash')[0].count,0);
});

for(const format of [undefined,'legacy'])test('legacy selection wins in a mixed workbook: '+format,()=>{
 const {ctx,calls}=setup();let parsedSheet;
 ctx.window.XLSX.read=()=>({SheetNames:['Wide','Legacy'],Sheets:{Wide:{name:'Wide'},Legacy:{name:'Legacy'}}});
 ctx.parseSheet=sheet=>{parsedSheet=sheet.name;return [{rows:[{work:'A',count:2,amount:10}],warnings:[],issues:[],count:2,amount:10}];};
 ctx.shiftPeriod=(_type,start)=>start;ctx.addMonths=start=>start;ctx.periodEnd=()=> '2024-01-07';
 const rule={type:'week',start:'2024-01-01',sheet:'Legacy',...(format?{format}:{})};
 const reports=ctx.parseExcel({name:'mixed.xlsx'},rule,[],'sig','hash');
 assert.equal(calls.length,0);assert.equal(parsedSheet,'Legacy');assert.equal(reports.length,1);assert.equal(reports[0].type,'week');
});
