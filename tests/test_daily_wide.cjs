'use strict';
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),{test}=require('node:test');
const html=fs.readFileSync('index.html','utf8'),app=[...html.matchAll(/<script>([\s\S]*?)<\/script>/g)][1][1];
const ctx=vm.createContext({console,Date,crypto:require('node:crypto').webcrypto,window:{matchMedia:()=>({matches:false}),XLSX:{utils:{sheet_to_json:sheet=>sheet}}},document:{}});
vm.runInContext(app.replace(/\n(?:window\.appReady=)?main\(\);\s*$/,'\n'),ctx);
const run=(code,values={})=>{Object.assign(ctx,values);return vm.runInContext(code,ctx);};
const date=(start,n)=>new Date(Date.parse(start+'T12:00:00Z')+n*86400000).toISOString().slice(0,10);
function matrix(start,n,{title='Отчёт',short=false,prefix=0}={}){
 const rows=Array.from({length:prefix},()=>['Дополнительный заголовок']);
 rows.push([title]);const dates=['Работа'],labels=[''];
 for(let i=0;i<n;i++){dates.push(short?date(start,i).slice(8)+'.'+date(start,i).slice(5,7):date(start,i),null);labels.push('Количество','Сумма');}
 dates.push('Итого за период',null);labels.push('Количество','Сумма');rows.push(dates,labels);
 for(const [name,q,a] of [['СМУ16',1.5,15],['Альфа',.5,5],['Бета',1,10],['Итого',1.5,15]]){
  const row=[name];for(let i=0;i<n;i++)row.push(q,a);row.push(q*n,a*n);rows.push(row);
 }
 const rr=prefix+2;rows['!merges']=[];
 for(let i=0;i<n+1;i++)rows['!merges'].push(run('excelColumnName(c)',{c:i*2+1})+rr+':'+run('excelColumnName(c)',{c:i*2+2})+rr);
 rows['!merges'].push('A'+rr+':A'+(rr+1));return rows;
}
const book=(sheets)=>({SheetNames:Object.keys(sheets),Sheets:sheets});
const parse=(workbook,options={})=>run('parseDailyWorkbook(workbook,options)',{workbook,options:{fileName:'Synthetic.xlsx',...options}});
for(const [year,n] of [[2025,365],[2024,366]])test('complete year '+year+' imports '+n+' days beyond 500 columns without importing totals',()=>{
 const sheet=matrix(year+'-01-01',n,{title:'Годовой отчёт '+year,prefix:18});assert.equal(sheet[20].length,n*2+3);
 const result=parse(book({Год:sheet}));assert.equal(result.reports.length,n);assert.equal(result.reports.at(-1).start,year+'-12-31');
 assert.equal(result.reports.reduce((s,r)=>s+r.count,0),1.5*n);assert.equal(result.reports.reduce((s,r)=>s+r.amount,0),15*n);
 assert.ok(result.reports.every(r=>r.rows.length===2&&r.warnings.length===0));
});
for(const [year,n] of [[2025,90],[2024,91]])test('first quarter '+year+' respects leap days and merged headers',()=>{
 const result=parse(book({'I квартал':matrix(year+'-01-01',n,{title:'Отчёт '+year,short:true})}));assert.equal(result.reports.length,n);assert.equal(result.sheets[0].coverage.end,year+'-03-31');assert.equal(result.reports[0].issues.length,0);
});
test('four quarterly sheets form one year; a duplicate day in another sheet is rejected',()=>{
 const sheets={Q1:matrix('2024-01-01',91),Q2:matrix('2024-04-01',91),Q3:matrix('2024-07-01',92),Q4:matrix('2024-10-01',92)};
 assert.equal(parse(book(sheets)).reports.length,366);
 assert.throws(()=>parse(book({...sheets,Duplicate:matrix('2024-01-01',1)})),/Повторный день/);
 assert.equal(parse(book(sheets),{sheets:['Q2']}).reports.length,91);
});
test('Excel serial dates, textual dates, and reordered days preserve actual dates',()=>{
 const sheet=matrix('2024-01-01',3);sheet[1][1]=45292;sheet[1][3]='3 января 2024';sheet[1][5]='02.01.2024';
 assert.deepEqual(Array.from(parse(book({Лист:sheet})).reports,r=>r.start),['2024-01-01','2024-01-03','2024-01-02']);
});
test('ambiguous short dates require explicit year and invalid dates are not guessed',()=>{
 const sheet=matrix('2024-01-01',3,{short:true});assert.throws(()=>parse(book({Лист:sheet})),/Год не определён/);
 assert.equal(parse(book({Лист:sheet}),{year:2024}).reports[0].start,'2024-01-01');
 sheet[1][1]='30.02.2024';assert.throws(()=>parse(book({Лист:sheet})),/Некорректная дата.*B2/);
});
test('blank daily pair is absent while explicit zero stays a genuine report',()=>{
 const blank=matrix('2024-01-01',3);for(let r=3;r<blank.length;r++){blank[r][3]=null;blank[r][4]=null;blank[r][7]-=blank[r][1];blank[r][8]-=blank[r][2];}
 const result=parse(book({Лист:blank}));assert.equal(result.reports.length,2);assert.ok(result.reports[0].issues.some(i=>i.type==='missing-day'));
 const zero=matrix('2024-01-01',3);for(let r=3;r<zero.length;r++){zero[r][3]=0;zero[r][4]=0;zero[r][7]-=zero[r][1];zero[r][8]-=zero[r][2];}
 const day=parse(book({Лист:zero})).reports.find(r=>r.start==='2024-01-02');assert.equal(day.count,0);assert.equal(day.amount,0);assert.equal(day.warnings.length,0);
});
test('zero department and total controls confirm an empty detail day, not a missing day',()=>{
 const sheet=matrix('2024-01-01',1);for(let r=3;r<sheet.length;r++){sheet[r][1]=r===3||r===6?0:null;sheet[r][2]=r===3||r===6?0:null;sheet[r][3]=0;sheet[r][4]=0;}
 const result=parse(book({Лист:sheet}));assert.equal(result.reports.length,1);assert.equal(result.reports[0].explicitZero,true);assert.equal(result.reports[0].warnings.length,0);
});
test('invalid cells and period total mismatches retain exact real Excel coordinates',()=>{
 const sheet=matrix('2024-01-01',3);sheet[4][1]='bad';sheet[4][7]=77;
 const report=parse(book({Лист:sheet})).reports[0];assert.ok(report.issues.some(i=>i.type==='invalid'&&i.cells.includes('B5')));assert.ok(report.issues.some(i=>i.scope==='source-period'&&i.cells.includes('H5')));assert.equal(report.hasMismatch,true);
});
test('manual shared-column mapping works when metric labels cannot be detected',()=>{
 const sheet=matrix('2024-01-01',3);sheet[2]=sheet[2].map((v,i)=>i===0?'':i%2?'Штуки':'Рубли');
 assert.equal(run('findDailySheets(workbook).length',{workbook:book({Лист:sheet})}),0);
 const parsed=parse(book({Лист:sheet}),{sheets:['Лист'],mappings:{Лист:{workCol:0,headerRow:2,firstDayCol:1,totalCountCol:7,totalAmountCol:8}}});assert.equal(parsed.reports.length,3);
});
test('missing header dates in a declared quarter do not produce zero reports',()=>{
 const result=parse(book({Q1:matrix('2024-01-01',2)}));assert.equal(result.reports.length,2);assert.ok(result.reports[0].issues.some(i=>i.type==='missing-day'&&i.date==='2024-01-03'));
});
test('large annual matrix does not repeatedly parse the XLSX archive',()=>{
 const sheet=matrix('2024-01-01',366),summary=sheet[3],overall=sheet[6],detail=sheet[4];sheet.splice(4,2,...Array.from({length:500},(_,i)=>{const row=detail.slice();row[0]='Работа '+i;return row;}));
 for(let c=1;c<summary.length;c++){summary[c]=detail[c]*500;overall[c]=detail[c]*500;}
 const started=performance.now(),result=parse(book({Год:sheet}));assert.equal(result.reports.length,366);assert.equal(result.reports[0].rows.length,500);assert.ok(performance.now()-started<15000,'Annual parse should remain practical on this test fixture');
});
module.exports={matrix,book};

test('wide rule identity is stable across Rust mapping key order, old rule strings remain compatible',()=>{
 const result=run(`(()=>{const a={type:'day',start:'',sheet:'',format:'daily-wide',sheets:['Q2','Q1'],mappings:{Q1:{workCol:0,headerRow:2,firstDayCol:1,totalCountCol:null},Q2:{workCol:0,headerRow:2,firstDayCol:1}}};const b={mappings:{Q2:{firstDayCol:1,headerRow:2,workCol:0},Q1:{firstDayCol:1,workCol:0,headerRow:2}},sheets:['Q1','Q2'],format:'daily-wide',sheet:'',start:'',type:'day'};return {equal:importRuleKey(a)===importRuleKey(b),legacy:importRuleKey({type:'week',start:'2026-10-19',sheet:''})};})()`);
 assert.equal(result.equal,true);assert.equal(result.legacy,JSON.stringify({type:'week',start:'2026-10-19',sheet:''}));
});

test('legacy repeated work columns with blank captions are never classified as shared daily columns',()=>{
 const legacy=[['Работа','Количество','Сумма',null,null,'Количество','Сумма'],['СМУ16',1,10,null,'СМУ16',1,10],['Работа А',1,10,null,'Работа Б',1,10]];
 assert.equal(run('findDailySheets(workbook).length',{workbook:book({Лист:legacy})}),0);assert.equal(run('readTripleColumns(sheet).length',{sheet:legacy}),2);
});
test('two-digit years require explicit century and numeric date style honors Excel 1904 system',()=>{
 const short=matrix('2026-01-01',1);short[1][1]='01.01.26';assert.throws(()=>parse(book({Лист:short})),/Год не определён/);assert.equal(parse(book({Лист:short}),{year:2026}).reports[0].start,'2026-01-01');
 const early=matrix('1904-01-01',1);early[1][1]=0;early['!date1904']=true;early['!dateCells']=new Set(['1:1']);assert.equal(parse(book({Лист:early})).reports[0].start,'1904-01-01');
});

test('quantity and amount date headers must agree before values can be paired',()=>{
 const sheet=matrix('2024-01-01',2);sheet['!merges']=[];sheet[1][2]='2024-01-02';sheet[1][4]='2024-01-01';
 assert.throws(()=>parse(book({Лист:sheet})),/Даты количества и суммы различаются/);
});
test('an empty historical sheet with nonzero controls cannot silently vanish from a source replacement',()=>{
 const good=matrix('2024-01-01',1),empty=matrix('2024-01-02',1);for(let row=3;row<empty.length;row++){empty[row][1]=null;empty[row][2]=null;}
 assert.throws(()=>parse(book({Good:good,Empty:empty})),/лист «Empty».*не содержит дневных отчётов/);
 assert.equal(parse(book({Good:good,Empty:empty}),{sheets:['Good']}).reports.length,1);
});

test('separate month/day header rows are recognized and mismatching day numbers are rejected',()=>{
 const sheet=matrix('2024-01-01',2,{title:'Отчёт 2024'});sheet.splice(1,0,[null,'Январь']);sheet[2]=['Работа',1,null,2,null,'Итого за период',null];
 sheet['!merges']=['A3:A4','B2:E2','B3:C3','D3:E3','F3:G3'];
 assert.equal(parse(book({Лист:sheet})).reports[1].start,'2024-01-02');sheet[2][2]=2;assert.throws(()=>parse(book({Лист:sheet})),/Даты количества и суммы различаются/);
});
test('a wide report below many title rows is not imported as a service catalog',()=>{
 const workbook=book({Год:matrix('2024-01-01',366,{prefix:18})});assert.equal(run('detectCatalogWorkbook(workbook).length',{workbook}),0);
});

test('quantity and amount dates on different header rows cannot silently disagree',()=>{
 const sheet=matrix('2024-01-01',1);sheet['!merges']=[];sheet[0][2]='2024-01-02';
 assert.throws(()=>parse(book({Лист:sheet})),/Даты количества и суммы различаются/);
});

test('single-row date and metric captions are recognized; totals are not another day',()=>{
 const sheet=matrix('2024-01-01',2);sheet[1]=['Работа','Количество 01.01.2024','Сумма 01.01.2024','Количество 02.01.2024','Сумма 02.01.2024','Итоговое количество','Итоговая сумма'];sheet.splice(2,1);sheet['!merges']=[];
 assert.equal(parse(book({Лист:sheet})).reports.length,2);
});
test('generic overall metric captions with explicit dates are daily values, not annual totals',()=>{
 const sheet=matrix('2024-01-01',2);sheet[2][1]='Общее количество';sheet[2][3]='Общее количество';
 assert.equal(parse(book({Лист:sheet})).reports.length,2);
});
