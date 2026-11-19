(function(){
'use strict';
const byId=id=>document.getElementById(id);
const months=byId('reportCalendar'),details=byId('calendarDetails');
const previous=byId('calendarPrev'),next=byId('calendarNext');
const range=byId('calendarRange'),status=byId('calendarStatus');
const monthNames=['Январь','Февраль','Март','Апрель','Май','Июнь','Июль','Август','Сентябрь','Октябрь','Ноябрь','Декабрь'];
const weekdays=['Пн','Вт','Ср','Чт','Пт','Сб','Вс'];
const validYear=value=>Number.isInteger(value)&&value>=1&&value<=9999;
let revision=null,year=null,snapshot=null,ticket=0,pending=null;

function element(tag,text,style){
 const node=document.createElement(tag);
 if(text!==null)node.textContent=text;
 if(style)node.className=style;
 return node;
}
function button(text,style){
 const node=element('button',text,style);
 node.type='button';
 return node;
}
function daysInMonth(y,m){
 const leap=y%4===0&&(y%100!==0||y%400===0);
 return [31,leap?29:28,31,30,31,30,31,31,30,31,30,31][m];
}
function monthPrefix(m){return String(year).padStart(4,'0')+'-'+String(m+1).padStart(2,'0');}
function describe(counts){
 return 'Дневных: '+counts.dayReports+' · Недельных: '+counts.weekReports+' · Месячных: '+counts.monthReports;
}
function clear(){
 snapshot=null;
 months.replaceChildren();details.replaceChildren();details.hidden=true;
 range.textContent='';previous.disabled=true;next.disabled=true;
}
function openMonth(m,trigger,days){
 const former=snapshot;
 for(const node of months.children)node.setAttribute('aria-expanded',String(node===trigger));
 const close=button('Закрыть дни месяца','button secondary');
 close.addEventListener('click',()=>{
  if(snapshot!==former)return;
  details.replaceChildren();details.hidden=true;
  trigger.setAttribute('aria-expanded','false');trigger.focus();
 });
 const grid=element('div',null,'calendar-day-grid');
 grid.setAttribute('aria-label','Дни месяца');
 for(const name of weekdays)grid.append(element('span',name,'calendar-weekday'));
 const prefix=monthPrefix(m);
 const start=new Date(prefix+'-01T12:00:00Z');
 for(let i=0;i<(start.getUTCDay()+6)%7;i++){
  const spacer=element('span',null);spacer.setAttribute('aria-hidden','true');grid.append(spacer);
 }
 for(let d=1;d<=daysInMonth(year,m);d++){
  const date=prefix+'-'+String(d).padStart(2,'0'),counts=days.get(date);
  const day=button(null,'calendar-day '+(counts?'calendar-has-reports':'calendar-empty'));
  day.dataset.date=date;
  day.append(element('strong',String(d)),element('small',counts?'Отчётов: '+(counts.dayReports+counts.weekReports+counts.monthReports):'0'));
  day.setAttribute('aria-label',date+' · '+(counts?describe(counts):'Отчётов с этой датой начала нет'));
  day.addEventListener('click',()=>{if(snapshot===former)window.nativeReportList.selectRange(date,date);});
  grid.append(day);
 }
 details.replaceChildren(element('h3',monthNames[m]+' '+year),close,grid);
 details.hidden=false;close.focus();
}
function render(data){
 const days=new Map(data.days.map(day=>[day.date,day]));
 const totals=monthNames.map(()=>({dayReports:0,weekReports:0,monthReports:0}));
 for(const day of days.values()){
  const sum=totals[Number(day.date.slice(5,7))-1];
  for(const key of ['dayReports','weekReports','monthReports'])sum[key]+=day[key];
 }
 range.textContent=year+' год';
 status.textContent=(data.firstYear===null?'Сохранённых отчётов нет. ':'')+'Количество отчётов указано по дате начала периода.';
 for(let m=0;m<12;m++){
  const hasReports=totals[m].dayReports+totals[m].weekReports+totals[m].monthReports>0;
  const month=button(null,'calendar-period '+(hasReports?'calendar-has-reports':'calendar-empty'));
  month.dataset.month=monthPrefix(m);
  month.append(element('strong',monthNames[m]),element('small',describe(totals[m])));
  month.setAttribute('aria-controls','calendarDetails');month.setAttribute('aria-expanded','false');
  month.addEventListener('click',()=>{if(snapshot===data)openMonth(m,month,days);});
  months.append(month);
 }
 previous.disabled=data.firstYear===null||year<=data.firstYear;
 next.disabled=data.lastYear===null||year>=data.lastYear;
}
async function load(requested,preferFirst=false){
 const current=++ticket,expected=revision;
 clear();status.textContent='Загрузка календаря…';
 try{
  let data=await window.desktopAPI.nativeReportCalendar(expected,requested);
  if(ticket!==current)return;
  if(data.firstYear!==null){
   const bounded=preferFirst?data.firstYear:Math.min(data.lastYear,Math.max(data.firstYear,requested));
   if(bounded!==requested){
    requested=bounded;
    data=await window.desktopAPI.nativeReportCalendar(expected,requested);
    if(ticket!==current)return;
   }
  }
  if(data.year!==requested||!validYear(data.year)||!Array.isArray(data.days)||data.days.length>366)throw Error('calendar');
  year=requested;render(data);snapshot=data;
 }catch{
  if(ticket===current){
   clear();status.textContent='Не удалось загрузить календарь отчётов. Обновите данные.';
  }
 }finally{if(ticket===current)pending=null;}
}
function setContext(value,initialDate){
 if(!Number.isSafeInteger(value)||value<0){
  ticket++;revision=null;year=null;pending=null;clear();status.textContent='';return;
 }
 if(revision===value&&(pending||snapshot))return pending;
 revision=value;
 const initial=typeof initialDate==='string'&&/^\d{4}-\d{2}-\d{2}$/.test(initialDate)?Number(initialDate.slice(0,4)):null;
 const requested=year??(validYear(initial)?initial:new Date().getUTCFullYear());
 pending=load(requested,year===null&&!validYear(initial));
 return pending;
}
previous.addEventListener('click',()=>{if(!previous.disabled) return pending=load(year-1);});
next.addEventListener('click',()=>{if(!next.disabled) return pending=load(year+1);});
clear();
window.nativeReportCalendar=Object.freeze({setContext});
})();
