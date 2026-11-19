// Tauri-only calendar summaries from indexed daily reports; bounded by a SQL
// keyset page. Never joins separately imported weekly/monthly source totals.
(function(){
'use strict';
const $=id=>document.getElementById(id);
const api=window.desktopAPI;
const adapter=window.nativePeriodSelection;
const issue=window.analizUserError||(()=>'Операция не выполнена. Обновите данные и повторите попытку.');
const make=(tag,value)=>{const e=document.createElement(tag);if(value!==undefined)e.textContent=String(value);return e;};
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const number=n=>finite(n)?n.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
let revision=null,organizations=[],ready=false,busy=false;
let generation=0,previousKind='day',cursor=null,back=[],next=null;
let selection=null;
let detailScope=null,detailCursor=null,detailNext=null,detailBack=[],detailRequest=0,detailBusy=false;
const pageSize=60;
function status(message,bad=false){
 const e=$('dailyRollupStatus');e.textContent=message;
 e.className='status '+(bad?'error':'muted');
}
function controls(){
 const valid=Number.isSafeInteger(revision)&&organizations.length>0;
 const from=$('dailyRollupFrom').value,to=$('dailyRollupTo').value;
 $('dailyRollupPrepare').disabled=busy||!valid;
 $('dailyRollupRun').disabled=busy||!valid||!ready||!from||!to;
 $('dailyRollupBack').disabled=busy||back.length===0;
 $('dailyRollupNext').disabled=busy||!next;
 $('dailyRollupPageInfo').textContent='Страница '+(back.length+1);
 for(const id of ['dailyRollupFrom','dailyRollupTo','dailyRollupGranularity'])
   $(id).disabled=busy||!valid;
}
function clear(message='Выберите даты и подготовьте дневные индексы.'){
 closeDetails();
 ++generation;
 cursor=null;next=null;back=[];selection=null;
 $('dailyRollupRows').replaceChildren();
 status(message);
 controls();
}
function canonical(kind,value,ending=false){
 if(!value)return null;
 if(kind==='year'){
   if(!/^\d{4}$/.test(value)||Number(value)<1)return null;
   return value+(ending?'-12-31':'-01-01');
 }
 const start=adapter.normalize(kind,value);
 if(!start)return null;
 if(!ending||kind==='day')return start;
 const d=new Date(start+'T00:00:00Z');
 if(kind==='week')d.setUTCDate(d.getUTCDate()+6);
 else if(kind==='month'){d.setUTCMonth(d.getUTCMonth()+1);d.setUTCDate(0);}
 return adapter.normalize('day',[
  String(d.getUTCFullYear()).padStart(4,'0'),
  String(d.getUTCMonth()+1).padStart(2,'0'),
  String(d.getUTCDate()).padStart(2,'0')
 ].join('-'));
}
function calendarValue(date,kind){
 if(!date)return '';
 if(kind==='year')return date.slice(0,4);
 return adapter.fromIsoDate(date,kind);
}
function changeKind(){
 const kind=$('dailyRollupGranularity').value;
 const inputs=['dailyRollupFrom','dailyRollupTo'];
 const dates=inputs.map(id=>canonical(previousKind,$(id).value));
 const type={day:'date',week:'week',month:'month',year:'number'}[kind];
 if(!type)return;
 inputs.forEach((id,i)=>{
   const input=$(id);input.type=type;
   if(kind==='year'){input.min='1';input.max='9999';}
   input.value=calendarValue(dates[i],kind);
 });
 previousKind=kind;
 clear('Выбрана сводка по '+({day:'дням',week:'неделям',month:'месяцам',year:'годам'}[kind])+
  '. Рассчитываются только дневные отчёты SQLite.');
}
function setContext(rev,names){
 const accepted=Number.isSafeInteger(rev)&&rev>=0&&Array.isArray(names)&&
   names.length>0&&names.length<=100&&names.every(n=>typeof n==='string'&&n.length>0);
 const changed=revision!==rev||JSON.stringify(organizations)!==JSON.stringify(names);
 if(!changed)return;
 const revisionChanged=revision!==rev;
 revision=accepted?rev:null;organizations=accepted?names.slice():[];
 if(revisionChanged)ready=false;
 $('dailyRollupScope').textContent=accepted?
   'Дневные отчёты SQLite · подразделения: '+organizations.join(', ')+
   '. Отдельные недельные и месячные отчёты не прибавляются.':
   'Нет доступной выбранной базы или подразделений.';
 clear(revisionChanged?
   'База обновлена. Подготовьте индексы дневных отчётов и повторите расчёт.':
   'Подразделения изменены. Повторите расчёт календарной сводки.');
}
function validate(page,kind){
 if(!page||page.granularity!==kind||!Array.isArray(page.points)||
    page.points.length>pageSize||
    (page.next_cursor!==null&&typeof page.next_cursor!=='string'))
   throw Error('Некорректная страница календарной сводки');
 let last='';
 for(const p of page.points){
   if(typeof p.start!=='string'||!/^\d{4}-\d{2}-\d{2}$/.test(p.start)||
     p.start<=last||!Number.isSafeInteger(p.days)||p.days<1||
     !Number.isSafeInteger(p.missing)||p.missing<0||p.missing>p.days*organizations.length||
     !Number.isSafeInteger(p.available_reports)||p.available_reports<0||
     p.available_reports>p.days*organizations.length||
     typeof p.complete!=='boolean'||
     p.complete!==(p.missing===0&&p.available_reports>0))
       throw Error('Несогласованные данные календаря');
   const tot=p.total||{},partial=p.available_total||{};
   const complete=p.complete,available=p.available_reports>0;
   for(const field of ['count','amount']){
     if(complete&&!finite(tot[field])||!complete&&tot[field]!==null||
        available&&!finite(partial[field])||!available&&partial[field]!==null)
       throw Error('Неполные данные нельзя считать нулями');
   }
   last=p.start;
 }
 if(page.next_cursor!==null&&page.next_cursor!==last)
   throw Error('Некорректный курсор следующей страницы');
}
function detailControls(){
 $('dailyRollupDetailBack').disabled=detailBusy||!detailBack.length;
 $('dailyRollupDetailNext').disabled=detailBusy||!detailNext;
 $('dailyRollupDetailPage').textContent='Страница '+(detailBack.length+1);
}
function closeDetails(){
 ++detailRequest;
 detailScope=null;detailCursor=null;detailNext=null;detailBack=[];
 $('dailyRollupDetailPanel').hidden=true;
 $('dailyRollupDetailRows').replaceChildren();
 $('dailyRollupDetailStatus').textContent='';
 detailControls();
}
function detailEnd(start,kind){
 const d=adapter.normalize('day',start);
 if(!d)return null;
 if(kind==='year')return d.slice(0,4)+'-12-31';
 const day=new Date(d+'T00:00:00Z');
 if(kind==='week')day.setUTCDate(day.getUTCDate()+6);
 else if(kind==='month'){day.setUTCMonth(day.getUTCMonth()+1);day.setUTCDate(0);}
 else return null;
 const out=String(day.getUTCFullYear()).padStart(4,'0')+'-'+
  String(day.getUTCMonth()+1).padStart(2,'0')+'-'+
  String(day.getUTCDate()).padStart(2,'0');
 return adapter.normalize('day',out);
}
function detailStatus(message,bad=false){
 const el=$('dailyRollupDetailStatus');el.textContent=message;
 el.className='status '+(bad?'error':'muted');
}
async function detailPage(after,reset=false){
 const scope=detailScope;
 if(detailBusy||!scope||selection!==scope.parent)return false;
 detailBusy=true;detailControls();
 const ticket=++detailRequest;
 try{
  const result=await api.nativeDailyRollup(scope.rev,scope.from,scope.to,
   'day',scope.orgs,after,pageSize);
  if(ticket!==detailRequest||detailScope!==scope||selection!==scope.parent)return false;
  validate(result,'day');
  for(const point of result.points){
   if(point.days!==1||point.start<scope.from||point.start>scope.to)
     throw Error('Детализация содержит чужие дни');
  }
  const tbody=$('dailyRollupDetailRows');tbody.replaceChildren();
  let incomplete=0;
  for(const point of result.points){
   const partial=!point.complete&&point.available_reports>0;
   if(!point.complete)incomplete++;
   const values=point.complete?point.total:point.available_total;
   const row=make('tr');
   for(const value of [
    point.start,point.available_reports,point.missing,
    number(values.count),number(values.amount),
    point.complete?'Полный':partial?'По имеющимся данным':'Отчётов нет'
   ])row.append(make('td',value));
   tbody.append(row);
  }
  detailCursor=after;detailNext=result.next_cursor;
  if(reset)detailBack=[];
  detailStatus('Дней на странице: '+result.points.length+
   '. Неполных: '+incomplete+'. Пропуски не считаются нулевыми отчётами.',incomplete>0);
  return true;
 }catch(error){
  if(ticket===detailRequest){
   detailNext=null;detailStatus('Не удалось получить дни: '+issue(error),true);
  }
  return false;
 }finally{detailBusy=false;detailControls();}
}
async function openDetails(point,kind){
 const parent=selection;
 if(busy||detailBusy||!parent||kind==='day')return;
 const from=adapter.normalize('day',point.start),to=detailEnd(point.start,kind);
 if(!from||!to||from<parent.from||to>parent.to){
  status('Ошибка границ дневной детализации: период выходит за пределы выбранного диапазона.',true);
  return;
 }
 closeDetails();
 detailScope={
  from,to,rev:parent.rev,orgs:parent.orgs.slice(),parent
 };
 $('dailyRollupDetailTitle').textContent='Дни периода '+(
  kind==='year'?from.slice(0,4):kind==='month'?from.slice(0,7):from+' — '+to);
 $('dailyRollupDetailPanel').hidden=false;
 detailStatus('Загружаем дни из SQLite…');
 await detailPage(null,true);
}

function display(page){
 validate(page,$('dailyRollupGranularity').value);
 const tbody=$('dailyRollupRows');tbody.replaceChildren();
 let incomplete=0,partly=0;
 for(const p of page.points){
   if(!p.complete)incomplete++;
   const partial=!p.complete&&p.available_reports>0;
   if(partial)partly++;
   const name=page.granularity==='year'?p.start.slice(0,4):
     page.granularity==='month'?p.start.slice(0,7):p.start;
   const values=p.complete?p.total:p.available_total;
   const tr=make('tr');
   for(const value of [name,p.days,p.available_reports,p.missing,
     number(values.count),number(values.amount),
     p.complete?'Полный':partial?'Неполный · по имеющимся данным':'Нет подтверждённых отчётов'])
      tr.append(make('td',value));
   const cell=make('td');
   if(page.granularity!=='day'){
    const button=make('button','Показать дни');
    button.type='button';
    button.setAttribute('aria-label','Открыть дни периода '+name);
    button.addEventListener('click',()=>{void openDetails(p,page.granularity);});
    cell.append(button);
   }else cell.textContent='—';
   tr.append(cell);
   tbody.append(tr);
 }
 status('Показано '+page.points.length+' периодов. Неполных: '+incomplete+
   (partly?'; с итогами по имеющимся данным: '+partly:'')+
   '. Отсутствующий день не равен нулю. Неизвестные дни не включены в показанные суммы.',
   incomplete>0);
}
function inputScope(){
 const kind=$('dailyRollupGranularity').value;
 const from=canonical(kind,$('dailyRollupFrom').value);
 const to=canonical(kind,$('dailyRollupTo').value,true);
 if(!from||!to||from>to)return null;
 return {kind,from,to,rev:revision,orgs:organizations.slice()};
}
async function page(after,reset=false){
 if(busy||!ready||revision===null)return false;
 const scope=selection;
 if(!scope)return false;
 closeDetails();
 busy=true;controls();
 const ticket=++generation;
 try{
   const result=await api.nativeDailyRollup(scope.rev,scope.from,scope.to,
     scope.kind,scope.orgs,after,pageSize);
   if(ticket!==generation||!selection||scope!==selection)return false;
   validate(result,scope.kind);
   display(result);
   cursor=after;next=result.next_cursor;
   if(reset)back=[];
   return true;
 }catch(err){
   if(ticket===generation){next=null;status('Сводка недоступна: '+issue(err),true);}
   return false;
 }finally{busy=false;controls();}
}
async function run(){
 if(busy||!ready)return;
 const scope=inputScope();
 if(!scope){clear('Выберите корректные даты календарного диапазона.');return;}
 clear('Рассчитываем сводку по дневным отчётам…');
 selection=scope;
 await page(null,true);
}
async function prepare(){
 if(busy||revision===null)return;
 busy=true;ready=false;controls();
 const ticket=++generation,rev=revision;
 let cursor=null,indexed=0;
 try{
   while(true){
     if(ticket!==generation||rev!==revision)throw Error('Изменились исходные данные');
     const chunk=await api.nativeAnalysisIndexPage(rev,'day',cursor,20);
     if(!chunk||!Number.isSafeInteger(chunk.processed)||chunk.processed<0||
        chunk.processed>20)throw Error('Некорректная порция индексации');
     indexed+=chunk.processed;
     status('Подготовлено дневных отчётов: '+indexed);
     if(chunk.done)break;
     if(!chunk.next||chunk.processed!==20)throw Error('Отсутствует курсор индекса');
     cursor=chunk.next;
   }
   if(ticket!==generation||rev!==revision)throw Error('Изменились исходные данные');
   await api.nativeAnalysisIndexSettings(rev);
   ready=true;status('Дневные индексы готовы. Выберите дни, недели, месяцы или годы.');
 }catch(err){ready=false;status('Не удалось подготовить дневные индексы: '+issue(err),true);}
 finally{busy=false;controls();}
}
$('dailyRollupDetailClose').addEventListener('click',closeDetails);
$('dailyRollupDetailNext').addEventListener('click',async()=>{
 if(!detailNext)return;
 const prior=detailCursor;
 if(await detailPage(detailNext))detailBack.push(prior);
 detailControls();
});
$('dailyRollupDetailBack').addEventListener('click',async()=>{
 if(!detailBack.length)return;
 const prior=detailBack.at(-1);
 if(await detailPage(prior))detailBack.pop();
 detailControls();
});
$('dailyRollupPrepare').addEventListener('click',()=>{void prepare();});
$('dailyRollupRun').addEventListener('click',()=>{void run();});
$('dailyRollupGranularity').addEventListener('change',changeKind);
for(const id of ['dailyRollupFrom','dailyRollupTo'])
 $(id).addEventListener('change',()=>{clear('Выбран другой диапазон. Повторите расчёт календаря.');});
$('dailyRollupNext').addEventListener('click',async()=>{
 if(!next)return;
 const prior=cursor;
 if(await page(next))back.push(prior);
 controls();
});
$('dailyRollupBack').addEventListener('click',async()=>{
 if(!back.length)return;
 const prior=back.at(-1);
 if(await page(prior))back.pop();
 controls();
});
const year=String(new Date().getFullYear());
$('dailyRollupFrom').value=year+'-01-01';
$('dailyRollupTo').value=year+'-12-31';
controls();
window.nativeDailyCalendar=Object.freeze({setContext,clear,busy:()=>busy});
})();