'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const filename=path.join(__dirname,'../unified-coverage.js');
class Element{
 constructor(tag='div',text=''){this.tagName=tag.toUpperCase();this.children=[];this._text=text??'';this.className='';this.attributes={};this.listeners={};this.hidden=false;this.open=false;this.disabled=false;}
 append(...children){this.children.push(...children);}replaceChildren(...children){this.children=children;this._text='';}
 get textContent(){return this._text+this.children.map(n=>typeof n==='object'?n.textContent:String(n)).join('');}
 set textContent(value){this._text=String(value);this.children=[];}
 setAttribute(name,value){this.attributes[name]=String(value);}addEventListener(name,callback){(this.listeners[name]??=[]).push(callback);}
 dispatch(name,extra={}){for(const callback of this.listeners[name]||[])callback({target:this,...extra});}focus(){this.focused=true;}
}
const all=node=>[node,...node.children.flatMap(n=>typeof n==='object'?all(n):[])];
const find=(node,tag,text)=>all(node).find(n=>n.tagName===tag&&n.textContent===text);
const settle=()=>new Promise(resolve=>setImmediate(resolve));
const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return{promise,resolve};};
function cell(start,patch={}){return{start,available:true,matching_reports:1,invalid_reports:0,duplicate_reports:0,overlap_reports:0,shifted_reports:0,warned_reports:0,count:1,amount:2,numeric_zero:false,has_activity:true,...patch};}
function page(from,to,organizations,patch=()=>({})){
 const periods=[];for(let date=new Date(from+'T00:00:00Z');date.toISOString().slice(0,10)<=to;date.setUTCDate(date.getUTCDate()+1)){
  const start=date.toISOString().slice(0,10);periods.push({start,end:start,unassigned_invalid:0});
 }
 return{revision:1,report_type:'day',periods,rows:organizations.map(organization=>({organization,cells:periods.map(p=>cell(p.start,patch(organization,p.start)))}))};
}
function fixture(overrides={}){
 const calls=[],root=new Element(),scope={revision:1,type:'day',displayType:'day',source:'daily',from:'2024-02-01',to:'2024-02-01',organizations:['A']};let active=true;
 const api=new Proxy({nativeQualityMatrixPage:async(_rev,_kind,from,to,orgs)=>page(from,to,orgs),...overrides},{get(target,key){if(!(key in target))throw Error('Unexpected API '+key);return(...args)=>{calls.push({key,args});return target[key](...args);};}});
 const UI={api,el:(tag,text,cls)=>{const n=new Element(tag,text);n.className=cls||'';return n;},fmt:n=>n===null?'—':String(n)};
 const window={unifiedUI:UI};vm.runInNewContext(fs.existsSync(filename)?fs.readFileSync(filename,'utf8'):'',{window,Date,Map,Set});
 assert.equal(typeof window.unifiedCoverage?.render,'function');
 return{root,calls,scope,render:()=>window.unifiedCoverage.render(root,scope,()=>active),invalidate:()=>active=false};
}
test('quality labels distinguish missing, invalid, conflicts, warnings, zeros and cancellation',async()=>{
 const patches=[{available:false,matching_reports:0,count:null,amount:null,has_activity:false},{available:false,invalid_reports:1,count:null,amount:null},{available:false,duplicate_reports:2,count:null,amount:null},{available:false,overlap_reports:1,count:null,amount:null},{warned_reports:1},{count:0,amount:0,numeric_zero:true,has_activity:false},{count:0,amount:0,numeric_zero:true,has_activity:true}];
 const f=fixture({nativeQualityMatrixPage:async(_r,_k,from,to,orgs)=>page(from,to,orgs,(_o,start)=>patches[Number(start.slice(-2))-1])});
 f.scope.to='2024-02-07';await f.render();const buttons=all(f.root).filter(n=>n.className==='coverage-cell-action');
 assert.equal(buttons.length,7);assert.match(buttons[0].textContent,/Нет отчёта/);assert.match(buttons[1].textContent,/Некорректный/);
 assert.match(buttons[2].textContent,/Повтор/);assert.match(buttons[3].textContent,/Пересечение/);assert.match(buttons[4].textContent,/Есть данные.*замечания/);
 assert.match(buttons[5].textContent,/Нулевые значения/);assert.match(buttons[6].textContent,/Нулевой итог/);assert.doesNotMatch(buttons[6].textContent,/Нулевые значения/);
 assert.match(f.root.textContent,/не подтверждает полноту источника/);
});
test('matrix requests bounded day pages and all organizations without reading diagnostics eagerly',async()=>{
 const f=fixture();f.scope.from='2024-01-01';f.scope.to='2024-02-11';f.scope.organizations=Array.from({length:10},(_,i)=>'Org '+i);await f.render();
 assert.equal(f.calls.length,2);assert.ok(f.calls.every(c=>c.key==='nativeQualityMatrixPage'&&c.args[4].length<=8));
 assert.deepEqual(f.calls.map(c=>Array.from(c.args.slice(2,4))),[['2024-01-01','2024-01-31'],['2024-01-01','2024-01-31']]);
 assert.equal(all(f.root).filter(n=>n.className==='coverage-cell-action').length,310);
 find(f.root,'BUTTON','Позднее →').dispatch('click');await settle();
 assert.deepEqual(Array.from(f.calls[2].args.slice(2,4)),['2024-02-01','2024-02-11']);
 assert.equal(all(f.root).filter(n=>n.className==='coverage-cell-action').length,110);
});
test('source evidence and saved cells are read only after selecting a report',async()=>{
 const f=fixture({
  nativeQualityMatrixReports:async()=>({revision:1,rows:[{id:'r1',start:'2024-02-01',end:'2024-02-01',source_name:'<img src=x>.xlsx',valid:true,warned:true,issue:null,duplicate:false,overlap:false,shifted:false}],next:null}),
  nativeReportQualityPage:async(_rev,_id,scope,offset)=>({issues:[{reason:scope==='report'?'Проверить сумму':'Проверить источник',sheet:'Q1',organization:'B',date:null,cells:offset?['C18']:['B18']}],next_offset:scope==='report'&&offset===0?50:null,source_anchor:'shared',warning_fallback:false,invalid_entries:0})
 });await f.render();assert.equal(f.calls.length,1);
 all(f.root).find(n=>n.className==='coverage-cell-action').dispatch('click');await settle();
 assert.equal(f.calls.at(-1).key,'nativeQualityMatrixReports');assert.equal(f.calls.at(-1).args.at(-1),20);assert.match(f.root.textContent,/<img src=x>\.xlsx/);
 const summary=find(f.root,'SUMMARY','Замечания отчёта');summary.dispatch('click');summary.open=true;
 const details=all(f.root).find(n=>n.tagName==='DETAILS');details.open=true;details.dispatch('toggle');await settle();
 assert.equal(f.calls.at(-1).key,'nativeReportQualityPage');assert.deepEqual(Array.from(f.calls.at(-1).args),[1,'r1','report',0,50]);
 assert.match(f.root.textContent,/Q1/);assert.match(f.root.textContent,/B18/);assert.match(f.root.textContent,/Подразделение: B/);
 find(f.root,'BUTTON','Следующие замечания →').dispatch('click');await settle();assert.equal(f.calls.at(-1).args[3],50);assert.match(f.root.textContent,/C18/);
 find(f.root,'BUTTON','Замечания источника').dispatch('click');await settle();assert.equal(f.calls.at(-1).args[2],'source');assert.match(f.root.textContent,/Проверить источник/);
 const close=find(f.root,'BUTTON','Закрыть подробности');close.dispatch('click');assert.equal(all(f.root).find(n=>n.className==='coverage-cell-action').focused,true);
});
test('unassigned invalid reports are separate evidence and do not become organization errors',async()=>{
 const f=fixture({nativeQualityMatrixPage:async(_r,_k,from,to,orgs)=>{const p=page(from,to,orgs,()=>({available:false,matching_reports:0,count:null,amount:null}));p.periods[0].unassigned_invalid=2;return p;},nativeQualityMatrixReports:async()=>({revision:1,rows:[],next:null})});
 await f.render();assert.match(f.root.textContent,/Без распознанного подразделения: 2/);assert.match(all(f.root).find(n=>n.className==='coverage-cell-action').textContent,/Нет отчёта/);
 find(f.root,'BUTTON','Показать отчёты без подразделения').dispatch('click');await settle();assert.equal(f.calls.at(-1).args[3],null);
});
test('late matrix and report responses cannot populate a changed selection',async()=>{
 const pending=deferred();const f=fixture({nativeQualityMatrixPage:()=>pending.promise});const rendered=f.render();f.invalidate();pending.resolve(page('2024-02-01','2024-02-01',['A']));await rendered;
 assert.equal(all(f.root).filter(n=>n.tagName==='TABLE').length,0);
 const reports=deferred();const g=fixture({nativeQualityMatrixReports:()=>reports.promise});await g.render();all(g.root).find(n=>n.className==='coverage-cell-action').dispatch('click');g.invalidate();reports.resolve({revision:1,rows:[{source_name:'STALE'}],next:null});await settle();assert.doesNotMatch(g.root.textContent,/STALE/);
});
test('omitted period rows are an API error rather than invented missing cells',async()=>{
 const f=fixture({nativeQualityMatrixPage:async()=>({revision:1,report_type:'day',periods:[],rows:[]})});await f.render();
 assert.match(f.root.textContent,/Не удалось показать/);assert.equal(all(f.root).filter(n=>n.className==='coverage-cell-action').length,0);
});
test('report evidence pages retain the cell and late notes cannot replace another cell',async()=>{
 const notes=deferred();const f=fixture({
  nativeQualityMatrixReports:async(_r,_k,start,_o,after)=>({revision:1,rows:[{id:after?'second':start,source_name:after?'second.xlsx':start+'.xlsx',start,end:start,valid:true}],next:after?null:'bound-cursor'}),
  nativeReportQualityPage:()=>notes.promise
 });f.scope.to='2024-02-02';await f.render();let actions=all(f.root).filter(n=>n.className==='coverage-cell-action');actions[0].dispatch('click');await settle();
 find(f.root,'BUTTON','Следующие отчёты →').dispatch('click');await settle();assert.deepEqual(Array.from(f.calls.at(-1).args),[1,'day','2024-02-01','A','bound-cursor',20]);assert.match(f.root.textContent,/second.xlsx/);
 find(f.root,'BUTTON','← Предыдущие отчёты').dispatch('click');await settle();const details=all(f.root).find(n=>n.tagName==='DETAILS');details.open=true;details.dispatch('toggle');await settle();assert.equal(f.calls.at(-1).key,'nativeReportQualityPage');
 const oldNotes=details.children[1];actions[1].dispatch('click');await settle();notes.resolve({issues:[{reason:'STALE-NOTES'}],next_offset:null,evidence:evidence({sheet_name:'STALE-SHEET'})});await settle();assert.doesNotMatch(oldNotes.textContent,/STALE-NOTES|STALE-SHEET/);assert.match(f.root.textContent,/2024-02-02.xlsx/);
});
function evidence(patch={}){return {document_id:'r1',source_format:null,sheet_name:null,coverage:null,explicit_zero:null,invalid_fields:0,missing_day_entries:0,deferred_day_entries:0,unclassified_day_entries:0,omitted_row_entries:0,...patch};}
async function openReportNotes(f){await f.render();all(f.root).find(n=>n.className==='coverage-cell-action').dispatch('click');await settle();const details=all(f.root).find(n=>n.tagName==='DETAILS');details.open=true;details.dispatch('toggle');await settle();return details;}
function evidenceFixture(read){return fixture({nativeQualityMatrixReports:async()=>({revision:1,rows:[{id:'r1',start:'2024-02-01',end:'2024-02-01',source_name:'source.xlsx',valid:true,warned:false,issue:null,duplicate:false,overlap:false,shifted:false}],next:null}),nativeReportQualityPage:async(_revision,_id,scope,offset)=>({issues:[],next_offset:null,source_anchor:'shared',warning_fallback:false,invalid_entries:0,...read(scope,offset)})});}
test('saved zero provenance preserves unknown false and true without certifying source completeness',async()=>{
 for(const [saved,expected] of [[null,/Признак явного нуля не сохранён/],[false,/Признак явного нуля: нет/],[true,/Сохранён признак явного нуля/]]){
  const f=evidenceFixture(()=>({evidence:evidence({explicit_zero:saved,sheet_name:'<Лист Q1>',coverage:{start:'2024-01-01',end:'2024-03-31'}})}));await openReportNotes(f);
  assert.match(f.root.textContent,expected);assert.match(f.root.textContent,/Лист отчёта: <Лист Q1>/);assert.match(f.root.textContent,/Заявленный охват листа: 01\.01\.2024 — 31\.03\.2024/);assert.match(f.root.textContent,/Полнота источника не установлена/);assert.doesNotMatch(f.root.textContent,/Подтверждённый ноль|Источник полностью заполнен/);
 }
});
test('source evidence counts all saved entries without assigning anchor zero or missing days to selected cell',async()=>{
 const issue=patch=>({kind:'missing-day',reason:'Saved day gap',sheet:'Q1',organization:null,date:'2024-02-01',source_row:null,work_cell:null,count_cell:null,amount_cell:null,cells:[],relevant:true,scope:'source-period',skipped:null,...patch});
 const sourceIssues=[...Array.from({length:55},()=>issue({})),issue({relevant:false}),issue({relevant:false}),issue({relevant:null}),issue({kind:'invalid',scope:'report',skipped:true}),issue({kind:'invalid',scope:'report',skipped:true})];
 const f=evidenceFixture((scope,offset)=>scope==='source'?{issues:sourceIssues.slice(offset,offset+50),next_offset:offset===0?50:null,evidence:evidence({document_id:'shared',explicit_zero:true,missing_day_entries:55,deferred_day_entries:2,unclassified_day_entries:1,invalid_fields:3,omitted_row_entries:2})}:{evidence:evidence()});await openReportNotes(f);find(f.root,'BUTTON','Замечания источника').dispatch('click');await settle();
 assert.match(f.root.textContent,/Записи о пропусках дней источника: 55/);assert.match(f.root.textContent,/не устанавливает пропуск у выбранного подразделения/);assert.match(f.root.textContent,/Отложенные замечания о пропусках дней: 2/);assert.match(f.root.textContent,/Записи о пропусках без достаточных сведений об актуальности или дате: 1/);assert.match(f.root.textContent,/Некорректных полей сохранённых сведений: 3/);assert.match(f.root.textContent,/Признак нуля отчёта с общими замечаниями не доказывает ноль выбранной ячейки/);assert.match(f.root.textContent,/Исключённые строки в общих замечаниях: 2/);assert.doesNotMatch(f.root.textContent,/Явно исключённые строки отчёта/);assert.doesNotMatch(f.root.textContent,/Сохранён признак явного нуля/);find(f.root,'BUTTON','Следующие замечания →').dispatch('click');await settle();assert.match(f.root.textContent,/Записи о пропусках дней источника: 55/);assert.match(f.root.textContent,/Исключённые строки в общих замечаниях: 2/);
});
test('diagnostic scope relevance and explicit omitted rows remain visible as saved facts',async()=>{
 const f=evidenceFixture(()=>({issues:[{kind:'invalid',reason:'Не удалось прочитать',sheet:'Q1',organization:'B',date:'2024-02-03',source_row:18,work_cell:null,count_cell:'B18',amount_cell:'C18',cells:[],relevant:false,scope:'source-period',skipped:true},{kind:'invalid',reason:'Детальная строка исключена',sheet:'Q1',organization:'B',date:null,source_row:19,work_cell:'A19',count_cell:'B19',amount_cell:'C19',cells:[],relevant:null,scope:null,skipped:true}],evidence:evidence({omitted_row_entries:1})}));await openReportNotes(f);
 assert.match(f.root.textContent,/Явно исключённые строки отчёта: 1/);assert.match(f.root.textContent,/Замечание периода источника/);assert.match(f.root.textContent,/Сохранённая пометка: неактуально/);assert.match(f.root.textContent,/Строка помечена как исключённая/);assert.match(f.root.textContent,/Строка: 18/);assert.match(f.root.textContent,/Подразделение: B/);
});
