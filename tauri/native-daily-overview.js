// Day-source-only, bounded KPI and timeline. A missing day stays null on
// the chart; an observed-only total is always labelled as partial.
(function(){
'use strict';
const $=id=>document.getElementById(id),ns='http://www.w3.org/2000/svg';
let snapshot=null;
const finite=v=>typeof v==='number'&&Number.isFinite(v);
const integer=v=>Number.isSafeInteger(v)&&v>=0;
const number=v=>finite(v)?v.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
const el=(tag,text)=>{const x=document.createElement(tag);if(text!==undefined)x.textContent=String(text);return x;};
const svg=(tag,props={},label)=>{
 const x=document.createElementNS(ns,tag);
 for(const [key,value]of Object.entries(props))x.setAttribute(key,String(value));
 if(label!==undefined)x.textContent=String(label);
 return x;
};
function status(text,bad=false){
 const x=$('dailySourceStatus');x.textContent=text;x.className='status '+(bad?'error':'muted');
}
function clearOrganizations(message='Сравнение подразделений появится после расчёта календаря.'){
 $('dailySourceDivisionRows').replaceChildren();
 const x=$('dailySourceDivisionStatus');x.textContent=message;x.className='status muted';
}
function organizationsError(message){
 clearOrganizations(message);
 $('dailySourceDivisionStatus').className='status error';
}
function clear(message='Показатели появятся после расчёта календаря.'){
 clearOrganizations();
 snapshot=null;
 $('dailySourceKpis').replaceChildren();
 $('dailySourceRows').replaceChildren();
 $('dailySourceChart').replaceChildren();
 status(message);
}
function validate(s,t){
 if(!s||!t||typeof s.complete!=='boolean'||!integer(s.periods)||
    !integer(s.organizations)||!integer(s.missing)||
    !integer(s.available_reports)||!s.total||!s.available_total||
    !['month','period'].includes(t.granularity)||
    !Array.isArray(t.points)||t.points.length>124)
   throw Error('Некорректная сводка из дневных отчётов');
 const available=s.available_reports>0;
 for(const field of ['count','amount']){
  if(s.complete&&!finite(s.total[field])||
     !s.complete&&s.total[field]!==null||
     available&&!finite(s.available_total[field])||
     !available&&s.available_total[field]!==null)
   throw Error('Неизвестное значение источника нельзя считать нулём');
 }
 if(s.complete&&(s.missing!==0||!available))
   throw Error('Полнота источника не подтверждена отчётами');
 let last='';
 for(const p of t.points){
  if(typeof p.start!=='string'||!/^\d{4}-\d{2}(?:-\d{2})?$/.test(p.start)||
     p.start<=last||!Number.isSafeInteger(p.periods)||p.periods<1||
     !integer(p.missing)||p.missing>p.periods*s.organizations||
     !p.total)
   throw Error('Некорректные периоды графика');
  for(const key of ['count','amount']){
   if(p.missing===0&&!finite(p.total[key])||
      p.missing>0&&p.total[key]!==null)
    throw Error('График не должен подменять пропуски нулём');
  }
  last=p.start;
 }
}
function draw(){
 $('dailySourceKpis').replaceChildren();
 $('dailySourceRows').replaceChildren();
 $('dailySourceChart').replaceChildren();
 if(!snapshot)return;
 const {summary:s,timeline:t}=snapshot;
 validate(s,t);
 const partial=!s.complete&&s.available_reports>0;
 const observed=partial?s.available_total:s.total;
 const metrics=[
  ['Количество / объём'+(partial?' · имеющиеся данные':''),number(observed.count)],
  ['Сумма'+(partial?' · имеющиеся данные':''),number(observed.amount)],
  ['Среднее'+(partial?' · имеющиеся данные':''),number(observed.average)],
  ['Имеющихся дневных отчётов',number(s.available_reports)],
  ['Пропусков по подразделениям',number(s.missing)]
 ];
 for(const [name,val] of metrics){
  const card=el('div');card.className='comparison-metric';
  card.append(el('strong',val),el('p',name));$('dailySourceKpis').append(card);
 }
 const metric=$('dailySourceMetric').value==='count'?'count':'amount';
 const label=metric==='count'?'Количество / объём':'Сумма';
 const values=[];
 for(const p of t.points){
  const total=p.total||{};
  const count=p.missing===0?total.count:null;
  const amount=p.missing===0?total.amount:null;
  const row=el('tr');
  for(const value of [p.start,p.periods,p.missing,number(count),number(amount)])
   row.append(el('td',value));
  $('dailySourceRows').append(row);
  values.push(metric==='count'?count:amount);
 }
 const confirmed=values.filter(finite);
 if(!confirmed.length){
  status('Нет полностью подтверждённых точек графика. '+(
   partial?'KPI показывают только имеющиеся дневные отчёты.':'Полные итоги не подтверждены.'
  ),true);
  return;
 }
 const lo=Math.min(0,...confirmed),hi=Math.max(0,...confirmed);
 const span=hi-lo||1;
 if(!finite(span)||span<=0)throw Error('Слишком большой диапазон чисел');
 const width=960,height=312,left=82,right=22,top=25,bottom=55;
 const plotW=width-left-right,plotH=height-top-bottom;
 const min=lo===hi?lo-span*.15:lo, max=lo===hi?hi+span*.15:hi;
 const axisMin=Math.min(0,min),axisMax=Math.max(0,max);
 if(!finite(axisMax-axisMin)||axisMax<=axisMin)throw Error('Невозможно вычислить ось');
 const x=i=>left+(i+.5)*plotW/values.length;
 const y=v=>top+(axisMax-v)/(axisMax-axisMin)*plotH;
 const root=svg('svg',{viewBox:'0 0 960 312',role:'img',
   'aria-label':label+' из дневных отчётов за выбранный диапазон'});
 root.append(svg('title',{},label+' из дневных отчётов. Пропуски образуют разрывы.'));
 const ticks=4;
 for(let i=0;i<=ticks;i++){
  const v=axisMin+(axisMax-axisMin)*i/ticks,py=y(v);
  root.append(svg('line',{x1:left,x2:width-right,y1:py,y2:py,
   stroke:Math.abs(v)<1e-9?'#92a3b8':'#e5eaf0','stroke-width':1}));
  root.append(svg('text',{x:left-9,y:py+4,'text-anchor':'end'},number(v)));
 }
 const skip=Math.max(1,Math.ceil(values.length/8));
 for(let i=0;i<values.length;i++){
  if(i%skip===0||i===values.length-1)
   root.append(svg('text',{x:x(i),y:height-bottom+22,'text-anchor':'middle'},
     t.points[i].start.slice(5)));
 }
 let segment=[];
 const flush=()=>{
  if(segment.length>1){
   root.append(svg('path',{fill:'none',stroke:'#24639a','stroke-width':2.7,
     'stroke-linejoin':'round','stroke-linecap':'round',
     d:segment.map(([px,py],i)=>(i?'L':'M')+px.toFixed(2)+' '+py.toFixed(2)).join(' ')}));
  }
  segment=[];
 };
 for(let i=0;i<values.length;i++){
  const v=values[i];
  if(v===null){flush();continue;}
  const px=x(i),py=y(v);
  segment.push([px,py]);
  if(values.length<=45){
   const circle=svg('circle',{cx:px,cy:py,r:3.5,fill:'#24639a',stroke:'#fff','stroke-width':1});
   circle.append(svg('title',{},t.points[i].start+': '+number(v)));
   root.append(circle);
  }
 }
 flush();
 $('dailySourceChart').append(root);
 const incomplete=t.points.length-confirmed.length;
 status(label+': '+confirmed.length+' подтверждённых точек из '+t.points.length+
  (incomplete?'; неполные периоды оставлены разрывами':'')+
  (t.granularity==='month'?'. Дневные данные объединены в календарные месяцы.':
   '. Показаны отдельные дневные даты.')+
  (partial?' Итоги охватывают только имеющиеся отчёты, не весь диапазон.':''),
  incomplete>0||!s.complete);
}
function showOrganizations(items,expected){
 const rows=$('dailySourceDivisionRows');
 rows.replaceChildren();
 try{
  if(!Array.isArray(items)||items.length>100||
     !Array.isArray(expected)||expected.length===0||expected.length>100||
     new Set(expected).size!==expected.length||items.length!==expected.length)
    throw Error('Некорректный список подразделений');
  const allowed=new Set(expected),seen=new Set();
  for(const item of items){
   if(!item||typeof item.org!=='string'||!allowed.has(item.org)||seen.has(item.org)||
      !Number.isSafeInteger(item.missing)||item.missing<0||
      typeof item.complete!=='boolean'||item.complete!==(item.missing===0)||
      !item.total||!['count','amount'].every(field=>
       item.complete?finite(item.total[field]):item.total[field]===null))
    throw Error('Неподтверждённые данные подразделения');
   seen.add(item.org);
  }
  const ordered=items.slice().sort((a,b)=>a.org.localeCompare(b.org,'ru'));
  const complete=ordered.filter(x=>x.complete).length;
  for(const item of ordered){
   const row=el('tr');
   for(const value of [
    item.org,item.missing,
    number(item.total.count),number(item.total.amount),
    item.complete?'Полный':'Неполный · полные итоги неизвестны'
   ])row.append(el('td',value));
   rows.append(row);
  }
  const x=$('dailySourceDivisionStatus');
  x.textContent='Подразделений: '+items.length+'. С полным дневным охватом: '+complete+
   '. Для неполных подразделений сумма и количество не подтверждены.';
  x.className='status '+(complete!==items.length?'error':'muted');
 }catch(error){organizationsError('Сравнение дневных подразделений недоступно: '+error.message);}
}
function show(summary,timeline){
 snapshot={summary,timeline};
 try{draw();}catch(e){clear('Дневной источник: '+e.message);status('Дневной источник: '+e.message,true);}
}
function error(message){clear(message);status(message,true);}
$('dailySourceMetric').addEventListener('change',()=>{
 if(!snapshot)return;
 try{draw();}catch(e){error('График: '+e.message);}
});
window.nativeDailyOverview=Object.freeze({show,showOrganizations,organizationsError,clear,error});
})();