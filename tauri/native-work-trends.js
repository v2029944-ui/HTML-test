// SQL work/group trends: render only bounded aggregate points, never source report JSON.
(function(){
'use strict';
const $=id=>document.getElementById(id),ns='http://www.w3.org/2000/svg';
let snapshot=null;
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const format=n=>finite(n)?n.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
function svg(tag,attrs={},label){
 const node=document.createElementNS(ns,tag);
 for(const [key,value] of Object.entries(attrs))node.setAttribute(key,String(value));
 if(label!==undefined)node.textContent=String(label);
 return node;
}
function status(message,bad=false){
 const element=$('workTrendStatus');element.textContent=message;
 element.className='status '+(bad?'error':'muted');
}
function clear(message='Выберите работу или группу из таблицы выше.'){
 snapshot=null;
 $('workTrendChart').replaceChildren();
 $('workTrendRows').replaceChildren();
 $('workTrendSelected').textContent='Позиция не выбрана.';
 status(message);
}
function draw(){
 const host=$('workTrendChart'),rows=$('workTrendRows');
 host.replaceChildren();rows.replaceChildren();
 if(!snapshot)return;
 if(typeof snapshot.key!=='string'||typeof snapshot.name!=='string'||
    !['group','work'].includes(snapshot.kind)||
    !['period','month'].includes(snapshot.granularity)||
    !Array.isArray(snapshot.points)||snapshot.points.length>124)
   throw Error('Некорректный ответ графика по работе');
 $('workTrendSelected').textContent=(snapshot.kind==='group'?'Группа: ':'Работа: ')+snapshot.name;
 const metric=$('workTrendMetric').value==='count'?'count':'amount';
 const title=metric==='count'?'Количество / объём':'Сумма';
 const valid=[];
 let missing=0;
 for(const point of snapshot.points){
   if(typeof point.start!=='string'||!/^\d{4}-\d{2}(?:-\d{2})?$/.test(point.start)||
      !Number.isSafeInteger(point.periods)||point.periods<1||
      !Number.isSafeInteger(point.missing)||point.missing<0||point.missing>point.periods*100)
     throw Error('Некорректные периоды SQL-графика');
   const t=point.total||{},ok=point.missing===0;
   if(ok&&(!finite(t.count)||!finite(t.amount))||
      !ok&&(t.count!==null||t.amount!==null))
     throw Error('Неизвестные суммы не могут быть нулями');
   const count=ok?t.count:null,amount=ok?t.amount:null;
   if(!ok)missing++;
   valid.push(metric==='count'?count:amount);
   const row=document.createElement('tr');
   for(const value of [point.start,point.periods,point.missing,format(count),format(amount)]){
     const cell=document.createElement('td');cell.textContent=String(value);row.append(cell);
   }
   rows.append(row);
 }
 if(!snapshot.points.length){
   status('Для этой позиции нет периодов выбранного диапазона.');return;
 }
 const numbers=valid.filter(finite);
 if(!numbers.length){
   status('Нет полностью подтверждённых периодов. Неизвестные значения показаны «—», а не нулями.',true);
   return;
 }
 const min=Math.min(0,...numbers),max=Math.max(0,...numbers);
 const delta=max-min;
 if(!Number.isFinite(delta))throw Error('Числовой диапазон слишком велик для оси');
 const target=(delta||1)/4,unit=Math.pow(10,Math.floor(Math.log10(target)));
 const step=[1,2,2.5,5,10].find(n=>n*unit>=target)*unit;
 if(!Number.isFinite(step)||step<=0)throw Error('Невозможно подобрать деление оси');
 const axisMin=Math.min(0,Math.floor(min/step)*step);
 const axisMax=Math.max(step,Math.ceil(max/step)*step);
 if(!Number.isFinite(axisMax-axisMin)||axisMin>=axisMax)
   throw Error('Невозможно показать диапазон значений');
 const width=960,height=300,left=78,right=24,top=24,bottom=55;
 const w=width-left-right,h=height-top-bottom;
 const x=i=>left+(i+.5)*w/valid.length;
 const y=v=>top+(axisMax-v)/(axisMax-axisMin)*h;
 const root=svg('svg',{viewBox:'0 0 960 300',role:'img',
   'aria-label':title+' — '+snapshot.name+', по периодам'});
 root.append(svg('title',{},title+' — '+snapshot.name));
 const ticks=Math.round((axisMax-axisMin)/step);
 for(let i=0;i<=Math.min(12,ticks);i++){
   const v=axisMin+i*step,py=y(v);
   root.append(svg('line',{x1:left,x2:width-right,y1:py,y2:py,
     stroke:v===0?'#8595a8':'#e5eaf0','stroke-width':v===0?1.5:1}));
   root.append(svg('text',{x:left-9,y:py+4,'text-anchor':'end'},format(v)));
 }
 const useBars=valid.length===1;
 const baseline=y(0),barW=Math.max(2,Math.min(38,w/valid.length*.7));
 let segment=[];
 const flush=()=>{
   if(segment.length>1)root.append(svg('path',{fill:'none',stroke:'#24639a',
     'stroke-width':2.6,'stroke-linecap':'round','stroke-linejoin':'round',
     d:segment.map(([px,py],i)=>(i?'L':'M')+px.toFixed(2)+' '+py.toFixed(2)).join(' ')}));
   segment=[];
 };
 for(let i=0;i<valid.length;i++){
   const v=valid[i];
   if(v===null){flush();continue;}
   const px=x(i),py=y(v);
   const mark=useBars
     ?svg('rect',{x:px-barW/2,y:Math.min(baseline,py),width:barW,
       height:Math.max(1,Math.abs(py-baseline)),fill:v<0?'#926da1':'#347db2'})
     :svg('circle',{cx:px,cy:py,r:3.8,fill:'#24639a',stroke:'#fff','stroke-width':1});
   mark.append(svg('title',{},snapshot.points[i].start+': '+format(v)));
   root.append(mark);
   if(!useBars)segment.push([px,py]);
 }
 flush();
 const labelEvery=Math.max(1,Math.ceil(valid.length/8));
 for(let i=0;i<valid.length;i++){
   if(i%labelEvery!==0&&i!==valid.length-1)continue;
   root.append(svg('text',{x:x(i),y:height-bottom+22,'text-anchor':'middle'},
     snapshot.points[i].start.slice(5)));
 }
 host.append(root);
 status(title+' · подтверждено '+numbers.length+' из '+valid.length+' точек'+
   (missing?'; '+missing+' с пропусками (линия разорвана)':'')+
   '. Отсутствующая работа в полном отчёте учитывается как ноль. Единицы измерения не определены.',missing>0);
}
function show(data){
 snapshot=data;
 try{draw();}catch(error){clear('Динамика скрыта: '+error.message);status('Динамика скрыта: '+error.message,true);}
}
function error(message){clear(message);status(message,true);}
$('workTrendMetric').addEventListener('change',()=>{if(snapshot)draw();});
window.nativeWorkTrends=Object.freeze({show,clear,error});
})();