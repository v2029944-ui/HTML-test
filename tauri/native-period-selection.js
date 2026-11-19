// Native calendar picker adapter: input type=date / week / month.
// SQLite keeps exact period-start YYYY-MM-DD bounds, not display literals.
(function(){
'use strict';
const reDate=/^\d{4}-\d{2}-\d{2}$/,reMonth=/^\d{4}-(0[1-9]|1[0-2])$/,reWeek=/^(\d{4})-W(\d{2})$/;
const $=id=>document.getElementById(id);
function dateUTC(year,month,day){
 if(year<1||year>9999)return null;
 const d=new Date(0);
 d.setUTCFullYear(year,month-1,day);
 d.setUTCHours(0,0,0,0);
 if(d.getUTCFullYear()!==year||d.getUTCMonth()!==month-1||d.getUTCDate()!==day)return null;
 return d;
}
function serial(d){
 if(!d||d.getUTCFullYear()<1||d.getUTCFullYear()>9999)return null;
 return String(d.getUTCFullYear()).padStart(4,'0')+'-'+
   String(d.getUTCMonth()+1).padStart(2,'0')+'-'+
   String(d.getUTCDate()).padStart(2,'0');
}
function dateValue(raw){
 if(!reDate.test(raw))return null;
 return serial(dateUTC(Number(raw.slice(0,4)),Number(raw.slice(5,7)),Number(raw.slice(8,10))));
}
function weekStart(isoYear,week){
 if(week<1||week>53)return null;
 const jan4=dateUTC(isoYear,1,4);if(!jan4)return null;
 const monday=new Date(jan4.getTime());
 monday.setUTCDate(monday.getUTCDate()-((monday.getUTCDay()+6)%7)+(week-1)*7);
 const thursday=new Date(monday.getTime());thursday.setUTCDate(monday.getUTCDate()+3);
 if(thursday.getUTCFullYear()!==isoYear)return null;
 return serial(monday);
}
function normalize(kind,raw){
 if(kind==='day')return dateValue(raw);
 if(kind==='year')return /^\d{4}$/.test(raw)&&Number(raw)>0?raw+'-01-01':null;
 if(kind==='month')return reMonth.test(raw)?raw+'-01':null;
 if(kind==='week'){
   const m=reWeek.exec(raw);
   return m?weekStart(Number(m[1]),Number(m[2])):null;
 }
 return null;
}
function fromIsoDate(date,kind){
 const valid=dateValue(date);
 if(!valid)return '';
 if(kind==='day')return valid;
 if(kind==='year')return valid.slice(0,4);
 if(kind==='month')return valid.slice(0,7);
 if(kind!=='week')return '';
 const d=new Date(valid+'T00:00:00Z');
 const thu=new Date(d.getTime());
 thu.setUTCDate(d.getUTCDate()+(3-(d.getUTCDay()+6)%7));
 const isoYear=thu.getUTCFullYear(),jan4=dateUTC(isoYear,1,4);
 if(!jan4)return '';
 const janThursday=new Date(jan4.getTime());
 janThursday.setUTCDate(jan4.getUTCDate()+(3-(jan4.getUTCDay()+6)%7));
 const week=1+Math.round((thu-janThursday)/(7*86400000));
 return String(isoYear).padStart(4,'0')+'-W'+String(week).padStart(2,'0');
}
let previousKind='day';
const ids=['analysisFrom','analysisTo','comparisonBeforeFrom','comparisonBeforeTo'];
function apply(kind){
 if(!['day','week','month','year'].includes(kind))return;
 const raw=ids.map(id=>$(id).value);
 const oldDates=raw.map(v=>v?normalize(previousKind,v):null);
 const nextType={day:'date',week:'week',month:'month',year:'number'}[kind];
 ids.forEach((id,i)=>{
   const input=$(id);
   input.type=nextType;
   input.min=kind==='year'?'1':'';input.max=kind==='year'?'9999':'';
   input.value=oldDates[i]?fromIsoDate(oldDates[i],kind):'';
 });
 previousKind=kind;
 $('analysisPeriodHelp').textContent=
   kind==='day'?'Выбирайте даты в календаре.':
   kind==='week'?'Выбирайте календарные недели (понедельник — воскресенье).':
   kind==='year'?'Выбирайте календарные годы. Для года используется дневной источник.':
   'Выбирайте месяцы: границы диапазона рассчитываются от первого числа месяца.';
}
function range(kind,start,end,source='imported'){
 const from=normalize(kind,start),last=normalize(kind,end);
 if(!from||!last||from>last||!['daily','imported'].includes(source)||kind==='year'&&source!=='daily')return null;
 let to=last;
 if(source==='daily'&&kind!=='day'){
   const d=new Date(last+'T00:00:00Z');
   if(kind==='week')d.setUTCDate(d.getUTCDate()+6);
   else if(kind==='month'){d.setUTCMonth(d.getUTCMonth()+1);d.setUTCDate(0);}
   else{d.setUTCFullYear(d.getUTCFullYear()+1,0,1);d.setUTCDate(0);}
   to=serial(d);if(!to)return null;
 }
 return {type:source==='daily'?'day':kind,from,to,displayType:kind,source};
}
window.nativePeriodSelection=Object.freeze({apply,normalize,fromIsoDate,range});
})();