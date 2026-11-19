'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const root=path.resolve(__dirname,'..');
const report={id:'selected',period_start:'2025-02-03',period_end:'2025-02-03',source_name:'test.xlsx'};
class Element{
 constructor(){
  this.children=[];this.textContent='';this.events={};this.attrs={};this.isConnected=true;
  this.classList={add(){},toggle(){}};
 }
 setAttribute(key,value){this.attrs[key]=value;}
 append(child){this.children.push(child);}
 replaceChildren(){this.children=[];}
 addEventListener(type,fn){this.events[type]=fn;}
 fire(type='click'){this.events[type]({preventDefault(){}});}
 showModal(){this.open=true;}
 close(){this.open=false;this.fire('close');}
 focus(){this.focused=true;}
}

function mount(read){
 const html=fs.readFileSync(path.join(root,'native-workspace.html'),'utf8');
 const e=Object.fromEntries([...html.matchAll(/id="reportQuality([^"]+)"/g)].map(([,id])=>[id,new Element()]));
 const calls=[];
 const window={desktopAPI:{nativeReportQualityPage(...args){calls.push(args);return read(...args);}}};
 const document={getElementById:id=>e[id.replace(/^reportQuality/,'')],createElement:()=>new Element()};
 vm.runInNewContext(fs.readFileSync(path.join(root,'native-report-quality.js'),'utf8'),{window,document});
 const quality=window.nativeReportQuality;
 quality.setRevision(5);
 const box=new Element();
 quality.addReportAction(box,report,5);
 return {e,calls,quality,action:box.children[0]};
}

const tick=()=>new Promise(resolve=>setImmediate(resolve));
const result=(issues,next_offset=null,source_anchor=null)=>({issues,next_offset,source_anchor});

test('report quality renders literal cells and reasons, pages at 50, and labels the shared source anchor',async()=>{
 const sample={kind:'invalid',reason:'<img src=x>',sheet:'Блок ©',source_row:3,date:'2025-02-03',
  work_cell:'A3',count_cell:'F3',amount_cell:'AA3',cells:['A3','F3','AA3']};
 const {e,calls,action}=mount(async(revision,id,scope,offset)=>{
  if(scope==='source')return result([{reason:'Общий контроль',cells:['B91','C91'],relevant:false}],null,'another-report');
  if(offset)return result([{reason:'Строка 51'}],null,'another-report');
  return result(Array.from({length:50},()=>sample),50,'another-report');
 });
 assert.equal(calls.length,0);
 action.fire();
 await tick();
 assert.deepEqual(calls[0],[5,'selected','report',0,50]);
 assert.equal(e.Close.focused,true);
 assert.equal(e.Rows.children.length,50);
 assert.equal(e.Rows.children[0].children[1].textContent,'<img src=x>');
 assert.equal(e.Rows.children[0].children[6].textContent,'Работа A3 · Количество F3 · Сумма AA3');
 e.Next.fire();
 await tick();
 assert.deepEqual(calls[1],[5,'selected','report',50,50]);
 assert.equal(e.Rows.children.length,1);
 assert.equal(e.Page.textContent,'Страница 2');
 assert.equal(e.Next.attrs['aria-disabled'],'true');
 e.Source.fire();
 await tick();
 assert.deepEqual(calls[2],[5,'selected','source',0,50]);
 assert.match(e.Anchor.textContent,/по всему источнику.*another-report/);
 assert.equal(e.Rows.children[0].children[6].textContent,'B91 · C91');
 e.Dialog.fire('cancel');
 assert.equal(e.Dialog.open,false);
 assert.equal(action.focused,true);
});

test('discarded reads cannot repaint a dismissed dialog or an updated revision',async()=>{
 let resolve;
 const {e,calls,quality,action}=mount(()=>new Promise(done=>{resolve=done;}));
 action.fire();
 e.Close.fire();
 resolve(result([{reason:'outdated'}]));
 await tick();
 assert.equal(e.Rows.children.length,0);
 action.fire();
 quality.setRevision(6);
 resolve(result([{reason:'stale'}]));
 await tick();
 assert.equal(e.Rows.children.length,0);
 action.fire();
 assert.equal(e.Dialog.open,false);
 assert.equal(calls.length,2);
});

test('empty saved quality never claims complete data',async()=>{
 const {e,action}=mount(async()=>result([]));
 action.fire();
 await tick();
 assert.match(e.Status.textContent,/Сохранённых замечаний нет.*Полнота данных этим не подтверждается/);
 assert.equal(e.Table.hidden,true);
 assert.equal(e.Pager.hidden,true);
});

test('malformed entries are disclosed alongside readable saved issues',async()=>{
 const {e,action}=mount(async()=>({...result([{reason:'usable'}]),invalid_entries:2}));
 action.fire();
 await tick();
 assert.equal(e.Rows.children.length,1);
 assert.match(e.Status.textContent,/Записей неподдерживаемого формата: 2/);
});
