// Render only bounded SQL aggregates. Never load full reports into WebView.
(function(){
'use strict';
const $=id=>document.getElementById(id),ns='http://www.w3.org/2000/svg';
let snapshot=null;
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const format=n=>finite(n)?n.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
function node(tag,attributes={},text){
 const el=document.createElementNS(ns,tag);
 for(const [key,value] of Object.entries(attributes))el.setAttribute(key,String(value));
 if(text!==undefined)el.textContent=String(text);
 return el;
}
function state(message,error=false){
 const el=$('timelineStatus');
 el.textContent=message;el.className='status '+(error?'error':'muted');
}
function clear(message='График появится после расчёта.'){
 snapshot=null;$('timelineChart').replaceChildren();$('timelineRows').replaceChildren();
 state(message);
}
function draw(){
 const view=snapshot&&{...snapshot,points:window.nativePeriodVisibility?.visiblePoints(snapshot.points,Boolean($('hideEmptyPeriods')?.checked))||snapshot.points};
 const container=$('timelineChart'),table=$('timelineRows');
 container.replaceChildren();table.replaceChildren();
 if(!view)return;
 if(snapshot.points?.length&&view.points?.length===0){state('Все периоды подтверждены как нулевые и скрыты. Итоги не изменены.');return;}
 const points=view.points;
 if(!Array.isArray(points)||points.length>124||!['period','month','week','year'].includes(view.granularity))
   throw Error('Некорректные данные SQL-графика');
 const measure=$('timelineMetric').value==='count'?'count':'amount';
 const label=measure==='count'?'Количество / объём':'Сумма';
 const values=[];
 for(const p of points){
   if(typeof p.start!=='string'||!/^\d{4}-\d{2}(?:-\d{2})?$/.test(p.start)||
      !Number.isSafeInteger(p.periods)||p.periods<1||
      !Number.isSafeInteger(p.missing)||p.missing<0||
      p.missing>p.periods*100)throw Error('Некорректный период SQL-графика');
   const ok=p.missing===0&&finite(p.total?.count)&&finite(p.total?.amount);
   const count=ok?p.total.count:null,amount=ok?p.total.amount:null;
   values.push(measure==='count'?count:amount);
   const tr=document.createElement('tr');
   for(const item of [p.start,p.periods,p.missing,format(count),format(amount)]){
     const td=document.createElement('td');td.textContent=String(item);tr.append(td);
   }
   table.append(tr);
 }
 if(!points.length){state('Нет периодов в выбранном диапазоне.');return;}
 const numbers=values.filter(finite),missing=points.length-numbers.length;
 if(!numbers.length){
   state('Ни одной полной точки: исходные пропуски не показаны нулями.',true);return;
 }
 const minimum=Math.min(0,...numbers),maximum=Math.max(0,...numbers);
 const stepBase=(maximum-minimum||1)/4;
 const scale=Math.pow(10,Math.floor(Math.log10(stepBase)));
 const step=[1,2,2.5,5,10].find(x=>x*scale>=stepBase)*scale;
 const axisMin=Math.min(0,Math.floor(minimum/step)*step);
 const axisMax=Math.max(step,Math.ceil(maximum/step)*step);
 const left=78,right=22,top=25,bottom=54,width=960,height=300;
 const chartWidth=width-left-right,chartHeight=height-top-bottom;
 const y=v=>top+(axisMax-v)/(axisMax-axisMin)*chartHeight;
 const x=i=>left+(i+0.5)*chartWidth/points.length;
 const mode=$('timelineMode').value==='bar'||points.length===1?'bar':'line';
 const svg=node('svg',{viewBox:'0 0 960 300',role:'img',
   'aria-label':label+'. График по выбранным периодам, '+points.length+' точек.'});
 svg.append(node('title',{},label+' по периодам'));
 const tickCount=Math.round((axisMax-axisMin)/step);
 for(let i=0;i<=Math.min(tickCount,12);i++){
   const v=axisMin+i*step,py=y(v);
   svg.append(node('line',{x1:left,y1:py,x2:width-right,y2:py,
     class:v===0?'timeline-zero':'timeline-grid'}));
   svg.append(node('text',{x:left-10,y:py+4,'text-anchor':'end'},format(v)));
 }
 if(mode==='line'){
   let segment=[];
   const flush=()=>{
     if(segment.length>1)svg.append(node('path',{class:'timeline-line',
       d:segment.map(([px,py],i)=>(i?'L':'M')+px.toFixed(2)+' '+py.toFixed(2)).join(' ')}));
     segment=[];
   };
   for(let i=0;i<points.length;i++){
     if(values[i]===null){flush();continue;}
     segment.push([x(i),y(values[i])]);
   }
   flush();
 }
 const base=y(0),barWidth=Math.max(2,Math.min(34,chartWidth/points.length*0.7));
 for(let i=0;i<points.length;i++){
   const value=values[i];
   if(value===null)continue;
   const px=x(i),py=y(value);
   const shape=mode==='bar'
     ?node('rect',{class:'timeline-bar'+(value<0?' negative':''),
       x:px-barWidth/2,y:Math.min(py,base),width:barWidth,height:Math.max(1,Math.abs(py-base))})
     :node('circle',{class:'timeline-point',cx:px,cy:py,r:4});
   shape.append(node('title',{},points[i].start+': '+format(value)));
   svg.append(shape);
 }
 const every=Math.max(1,Math.ceil(points.length/8));
 for(let i=0;i<points.length;i++){
   if(i%every!==0&&i!==points.length-1)continue;
   svg.append(node('text',{x:x(i),y:height-bottom+22,'text-anchor':'middle'},(view.granularity==='year'?points[i].start.slice(0,4):view.granularity==='month'?points[i].start:points[i].start.slice(5))));
 }
 container.append(svg);
 state(label+' · '+({month:'по месяцам',week:'по неделям',year:'по годам',period:'по исходным периодам'}[view.granularity])+
   ' · полных точек '+numbers.length+' из '+points.length+
   (missing?'; '+missing+' с пропусками (без соединяющих линий)':'')+
   '. Единицы исходного отчёта не подтверждены.',missing>0);
}
function show(data){snapshot=data;try{draw();}catch(e){clear('Невозможно построить достоверный график: '+e.message);}}
function error(message){clear(message);state(message,true);}
$('timelineMetric').addEventListener('change',()=>{if(snapshot)draw();});
$('timelineMode').addEventListener('change',()=>{if(snapshot)draw();});
$('hideEmptyPeriods')?.addEventListener('change',()=>{if(snapshot)show(snapshot);});
window.nativeTimeline=Object.freeze({show,clear,error});
})();