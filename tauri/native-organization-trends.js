// Render bounded per-organization SQLite aggregates; no report JSON in WebView.
(function(){
'use strict';
const $=id=>document.getElementById(id),ns='http://www.w3.org/2000/svg';
const palette=['#24639a','#ad5c36','#328165','#8a5daa','#bb7832','#617c99','#a94e7c','#65713c'];
let snapshot=null;
const finite=value=>typeof value==='number'&&Number.isFinite(value);
const format=value=>finite(value)?value.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
function svg(tag,attrs={},label){
 const el=document.createElementNS(ns,tag);
 for(const [name,value] of Object.entries(attrs))el.setAttribute(name,String(value));
 if(label!==undefined)el.textContent=String(label);
 return el;
}
function status(message,invalid=false){
 const el=$('organizationTrendsStatus');el.textContent=message;
 el.className='status '+(invalid?'error':'muted');
}
function clear(message='Динамика появится после расчёта.'){
 snapshot=null;
 $('organizationTrendsChart').replaceChildren();
 $('organizationTrendsLegend').replaceChildren();
 $('organizationTrendsRows').replaceChildren();
 status(message);
}
function validate(data){
 if(!data||!['period','month','week','year'].includes(data.granularity)||
    !Number.isSafeInteger(data.selected_organizations)||data.selected_organizations<1||
    data.selected_organizations>100||!Array.isArray(data.series)||
    data.series.length>8||data.series.length>data.selected_organizations)
   throw Error('Некорректный ответ аналитики подразделений');
 const first=data.series[0]?.points||[];
 if(!Array.isArray(first)||first.length>124)
   throw Error('Превышен размер ряда');
 const verified=[];
 const seen=new Set();
 for(const serie of data.series){
   if(typeof serie.organization!=='string'||!serie.organization||seen.has(serie.organization)||
      !Array.isArray(serie.points)||serie.points.length!==first.length)
     throw Error('Несогласованные серии подразделений');
   seen.add(serie.organization);
   const points=[];
   for(let i=0;i<serie.points.length;i++){
     const p=serie.points[i];
     if(typeof p.start!=='string'||!/^\d{4}-\d{2}(?:-\d{2})?$/.test(p.start)||
       !Number.isSafeInteger(p.periods)||p.periods<1||
       !Number.isSafeInteger(p.missing)||p.missing<0||p.missing>p.periods||
       p.start!==first[i].start||p.periods!==first[i].periods)
       throw Error('Расхождение периодов в сериях');
     const available=p.missing===0;
     const count=available?p.total?.count:null;
     const amount=available?p.total?.amount:null;
     if(available&&(!finite(count)||!finite(amount))||
        !available&&(p.total?.count!==null||p.total?.amount!==null))
       throw Error('Нет подтверждённых значений периода');
     points.push({start:p.start,periods:p.periods,missing:p.missing,count,amount});
   }
   verified.push({organization:serie.organization,points});
 }
 return verified;
}
function draw(){
 const chart=$('organizationTrendsChart'),legend=$('organizationTrendsLegend'),table=$('organizationTrendsRows');
 chart.replaceChildren();legend.replaceChildren();table.replaceChildren();
 if(!snapshot)return;
 validate(snapshot);
 const series=validate({...snapshot,series:window.nativePeriodVisibility?.visibleSeries(snapshot.series,Boolean($('hideEmptyPeriods')?.checked))||snapshot.series});
 if(!series.length){status('Не найдены выбранные подразделения.');return;}
 const metric=$('organizationTrendsMetric').value==='count'?'count':'amount';
 const title=metric==='amount'?'Сумма':'Количество / объём';
 const available=series.flatMap(s=>s.points.map(p=>p[metric]).filter(finite));
 let skipped=0;
 series.forEach((s,i)=>{
   const color=palette[i];
   const label=document.createElement('span');
   const swatch=document.createElement('span');
   swatch.className='organization-trends-swatch';swatch.style.backgroundColor=color;
   const name=document.createElement('span');name.textContent=s.organization;
   label.append(swatch,name);legend.append(label);
   for(const point of s.points){
     if(point[metric]===null)skipped++;
     const tr=document.createElement('tr');
     for(const value of [s.organization,point.start,point.periods,point.missing,
                         format(point.count),format(point.amount)]){
       const td=document.createElement('td');td.textContent=String(value);tr.append(td);
     }
     table.append(tr);
   }
 });
 if(!available.length&&snapshot.series.some(s=>s.points.length)&&series.every(s=>s.points.length===0)){status('Все показанные периоды подтверждены как нулевые и скрыты. Итоги не изменены.');return;}
 if(!available.length){
   status('Нет подтверждённых точек: пропуски каждого подразделения указаны в таблице.',true);
   return;
 }
 const min=Math.min(0,...available),max=Math.max(0,...available);
 const span=max-min;
 if(!Number.isFinite(span))throw Error('Слишком большой разброс значений');
 const targetStep=(span||1)/4;
 const unit=Math.pow(10,Math.floor(Math.log10(targetStep)));
 const step=[1,2,2.5,5,10].find(x=>x*unit>=targetStep)*unit;
 if(!Number.isFinite(step)||step<=0)throw Error('Не удалось построить числовую шкалу');
 const low=Math.min(0,Math.floor(min/step)*step);
 const high=Math.max(step,Math.ceil(max/step)*step);
 if(!Number.isFinite(high-low)||high<=low)throw Error('Не удалось построить числовую шкалу');
 const width=960,height=320,left=84,right=24,top=24,bottom=52;
 const w=width-left-right,h=height-top-bottom,count=series[0].points.length;
 const x=i=>left+(i+.5)*w/count,y=v=>top+(high-v)/(high-low)*h;
 const root=svg('svg',{viewBox:'0 0 960 320',role:'img',
   'aria-label':'Динамика подразделений: '+title+'; '+series.length+' серий'});
 root.append(svg('title',{},'Динамика подразделений по периодам'));
 const ticks=Math.round((high-low)/step);
 for(let i=0;i<=Math.min(12,ticks);i++){
   const val=low+i*step,py=y(val);
   root.append(svg('line',{x1:left,y1:py,x2:width-right,y2:py,
     stroke:val===0?'#8595a8':'#e5eaf0','stroke-width':val===0?1.5:1}));
   root.append(svg('text',{x:left-9,y:py+4,'text-anchor':'end'},format(val)));
 }
 series.forEach((serie,i)=>{
   let points=[];
   const flush=()=>{
     if(points.length>1){
       const path=svg('path',{d:points.map(([px,py],j)=>(j?'L':'M')+px.toFixed(2)+' '+py.toFixed(2)).join(' '),
         fill:'none',stroke:palette[i],'stroke-width':2.6,
         'stroke-linejoin':'round','stroke-linecap':'round'});
       path.append(svg('title',{},serie.organization));
       root.append(path);
     }
     points=[];
   };
   serie.points.forEach((p,j)=>{
     const value=p[metric];
     if(value===null){flush();return;}
     const px=x(j),py=y(value);
     points.push([px,py]);
     if(count<=35){
       const dot=svg('circle',{cx:px,cy:py,r:3.5,fill:palette[i],stroke:'#fff','stroke-width':1});
       dot.append(svg('title',{},serie.organization+' · '+p.start+': '+format(value)));
       root.append(dot);
     }
   });
   flush();
 });
 const every=Math.max(1,Math.ceil(count/8));
 for(let i=0;i<count;i++){
   if(i%every!==0&&i!==count-1)continue;
   root.append(svg('text',{x:x(i),y:height-bottom+21,'text-anchor':'middle'},
     (snapshot.granularity==='year'?series[0].points[i].start.slice(0,4):snapshot.granularity==='month'?series[0].points[i].start:series[0].points[i].start.slice(5))));
 }
 chart.append(root);
 const limited=snapshot.selected_organizations-series.length;
 status(title+' · подтверждено '+available.length+' из '+(available.length+skipped)+' точек. '+
   (limited?'Показано первые '+series.length+' из '+snapshot.selected_organizations+
     ' подразделений в порядке настроек. ':'')+
   (skipped?'Неполных точек: '+skipped+'; линии разорваны. ':'')+
   'Единицы измерения исходных данных не подтверждены.',skipped>0||limited>0);
}
function show(result){
 snapshot=result;
 try{draw();}catch(err){clear('График подразделений скрыт: '+err.message);status('График подразделений скрыт: '+err.message,true);}
}
function error(message){clear(message);status(message,true);}
$('organizationTrendsMetric').addEventListener('change',()=>{if(snapshot)show(snapshot);});
$('hideEmptyPeriods')?.addEventListener('change',()=>{if(snapshot)show(snapshot);});
window.nativeOrganizationTrends=Object.freeze({show,clear,error});
})();