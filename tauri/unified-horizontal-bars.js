/* Period-page horizontal bar renderer. SQL values and missing periods remain unchanged. */
(function(){
'use strict';
const NS='http://www.w3.org/2000/svg';
function make(name,attributes={},text){
 const node=document.createElementNS(NS,name);
 for(const [key,value]of Object.entries(attributes))node.setAttribute(key,String(value));
 if(text!==undefined)node.textContent=String(text);
 return node;
}
// drawChart replaces its children. Reuse each control so changing the SVG
// does not discard the page chooser or its keyboard focus.
const chartPagers=new WeakMap();
function chartPager(plot,key,options){
 let pagers=chartPagers.get(plot);
 if(!pagers){pagers=new Map();chartPagers.set(plot,pagers);}
 let host=pagers.get(key);
 if(!host){host=document.createElement('div');host.dataset.sqlScrollHost=key;pagers.set(key,host);}
 plot.append(host);
 window.sqlRowScroll.attach(host,{...options,footer:host,controlsOnly:true});
}
function horizontalBars(periods,shown,data,metric,root,options,color,redraw,seriesNumbers){
 if(!periods.length||!shown.length)return;
 const pageSize=12,pageCount=Math.ceil(periods.length/pageSize);
 const page=Math.max(0,Math.min(pageCount-1,Number.isSafeInteger(options.periodPage)?options.periodPage:0));
 const start=page*pageSize,visible=periods.slice(start,start+pageSize);
 const presentation=window.unifiedOriginal;
 if(pageCount>1){
  chartPager(root,'chart-periods',{index:page,size:pageSize,count:visible.length,total:periods.length,
   axis:'x',label:'Периоды горизонтальной диаграммы',go:index=>{options.periodPage=index;redraw();}});
 }
 const values=data.flatMap(point=>point?shown.map(name=>point.get(name)?.[metric]).filter(Number.isFinite):[]);
 const scale=presentation.chartNiceScale(values),{min,max}=scale;
 const left=160,right=30,top=36,bottom=38,width=Math.max(740,root.clientWidth||740);
 const plotWidth=width-left-right,groupHeight=shown.length*19+35,height=top+visible.length*groupHeight+bottom;
 const px=v=>left+(v-min)/(max-min)*plotWidth;
 const svg=make('svg',{viewBox:'0 0 '+width+' '+height,role:'img','aria-label':'Горизонтальные столбцы по отчётным периодам'});
 svg.style.minWidth=width+'px';
 for(const v of scale.ticks){
  const x=px(v);
  svg.append(make('line',{x1:x,y1:top-14,x2:x,y2:height-bottom+6,stroke:'var(--grid)'}));
  svg.append(make('text',{x,y:16,'text-anchor':'middle',fill:'var(--plot-label)','font-size':11},presentation.chartTickLabel(v,scale.step)));
 }
 const zero=px(0);svg.append(make('line',{x1:zero,y1:top-14,x2:zero,y2:height-bottom+6,stroke:'var(--axis)'}));
 const numbered=new Set();
 let hasCompleteValue=false;
 visible.forEach((period,index)=>{
  const originalIndex=start+index,y=top+index*groupHeight,point=data[originalIndex];
  const caption=presentation.displayPeriod(period);
  const heading=make('text',{x:8,y:y+14,fill:'var(--plot-label)','font-size':12},
    caption.length>23?caption.slice(0,22)+'…':caption);
  heading.append(make('title',{},caption));svg.append(heading);
  if(!point){
   svg.append(make('text',{x:left+8,y:y+32,fill:'var(--warning)','font-size':11},
    typeof options.pointNotice==='function'&&options.pointNotice(originalIndex,shown)||'Нет полного отчёта'));
   return;
  }
  shown.forEach((name,j)=>{
   const item=point.get(name),v=item?.[metric],ybar=y+21+j*19;
   const reason=metric==='average'?presentation.averageQuantityReason(item?.count):'';
   if(Number.isFinite(v)||reason)hasCompleteValue=true;
   if(!Number.isFinite(v)){
    svg.append(make('text',{x:left+8,y:ybar+11,fill:'var(--warning)','font-size':11},reason?'— '+reason:'— нет данных'));
    return;
   }
   if(seriesNumbers&&!numbered.has(name)){
    svg.append(make('text',{x:left-8,y:ybar+11,'text-anchor':'end',fill:'var(--text)','font-size':11,'aria-hidden':'true'},seriesNumbers.get(name)));
    numbered.add(name);
   }
   const x=px(v);
   const mark=make('rect',{x:Math.min(x,zero),y:ybar,width:Math.max(1,Math.abs(x-zero)),height:13,rx:2,fill:color(name),tabindex:0});
   const accessible=(options.label?options.label(name):name)+' · '+caption+': '+presentation.fmt(v,metric);
   mark.dataset.chartTooltip=accessible;mark.setAttribute('aria-describedby','chartTooltip');mark.setAttribute('aria-label',accessible);mark.append(make('title',{},accessible));svg.append(mark);
  });
 });
 const wrap=document.createElement('div');wrap.className='horizontal-bar-scroll';wrap.append(svg);root.append(wrap);
 const legend=document.createElement('div');legend.className='chart-legend';
 for(const name of shown){
  const label=options.label?options.label(name):name,item=document.createElement('span');
  item.textContent=seriesNumbers?seriesNumbers.get(name)+' · '+label:label;item.title=label;
  item.style.setProperty('--series',color(name));legend.append(item);
 }
 root.append(legend);
 if(!hasCompleteValue){const message=document.createElement('p');message.className='muted small';message.textContent='Нет полных данных за показанные периоды. Пропуски не заменены нулями.';root.append(message);}
}
window.unifiedChartPager=chartPager;
window.unifiedHorizontalBars=horizontalBars;
if(typeof module==='object'&&module?.exports)module.exports={horizontalBars};
})();