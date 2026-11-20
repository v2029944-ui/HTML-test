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
function navButton(text,disabled,action){
 const node=document.createElement('button');
 node.type='button';node.className='button secondary';node.textContent=text;
 node.disabled=disabled;node.addEventListener('click',action);return node;
}
function horizontalBars(periods,shown,data,metric,root,options,color,redraw){
 if(!periods.length||!shown.length)return;
 const pageSize=12,pageCount=Math.ceil(periods.length/pageSize);
 const page=Math.max(0,Math.min(pageCount-1,Number.isSafeInteger(options.periodPage)?options.periodPage:0));
 const start=page*pageSize,visible=periods.slice(start,start+pageSize);
 const presentation=window.unifiedOriginal;
 if(pageCount>1){
  const nav=document.createElement('div');nav.className='horizontal-bar-pager';
  nav.append(navButton('← Предыдущие периоды',page===0,()=>{options.periodPage=page-1;redraw();}),
    document.createTextNode('Периоды '+(start+1)+'–'+(start+visible.length)+' из '+periods.length),
    navButton('Следующие периоды →',page===pageCount-1,()=>{options.periodPage=page+1;redraw();}));
  root.append(nav);
 }
 const values=[];
 for(let i=start;i<start+visible.length;i++){
  if(!data[i])continue;
  for(const name of shown){const v=data[i].get(name)?.[metric];if(Number.isFinite(v))values.push(v);}
 }
 const bottomValue=Math.min(0,...values),topValue=Math.max(0,...values);
 const widthRange=bottomValue===topValue?Math.max(1,Math.abs(topValue)):topValue-bottomValue;
 const min=bottomValue===topValue?bottomValue-widthRange:bottomValue;
 const max=bottomValue===topValue?topValue+widthRange:topValue;
 const left=160,right=30,top=36,bottom=38,width=Math.max(740,root.clientWidth||740);
 const plotWidth=width-left-right,groupHeight=shown.length*19+35,height=top+visible.length*groupHeight+bottom;
 const px=v=>left+(v-min)/(max-min)*plotWidth;
 const svg=make('svg',{viewBox:'0 0 '+width+' '+height,role:'img','aria-label':'Горизонтальные столбцы по отчётным периодам'});
 svg.style.minWidth=width+'px';
 for(let t=0;t<=4;t++){
  const v=min+(max-min)*t/4,x=px(v);
  svg.append(make('line',{x1:x,y1:top-14,x2:x,y2:height-bottom+6,stroke:'var(--grid)'}));
  svg.append(make('text',{x,y:16,'text-anchor':'middle',fill:'var(--plot-label)','font-size':11},presentation.fmt(v,metric)));
 }
 const zero=px(0);svg.append(make('line',{x1:zero,y1:top-14,x2:zero,y2:height-bottom+6,stroke:'var(--axis)'}));
 visible.forEach((period,index)=>{
  const originalIndex=start+index,y=top+index*groupHeight,point=data[originalIndex];
  const caption=presentation.displayPeriod(period);
  const heading=make('text',{x:8,y:y+14,fill:'var(--plot-label)','font-size':12},
    caption.length>23?caption.slice(0,22)+'…':caption);
  heading.append(make('title',{},caption));svg.append(heading);
  if(!point){
   svg.append(make('text',{x:left+8,y:y+32,fill:'var(--warning)','font-size':11},
    typeof options.pointNotice==='function'&&options.pointNotice(originalIndex)||'Нет полного отчёта'));
   return;
  }
  shown.forEach((name,j)=>{
   const v=point.get(name)?.[metric],ybar=y+21+j*19;
   if(!Number.isFinite(v)){
    svg.append(make('text',{x:left+8,y:ybar+11,fill:'var(--warning)','font-size':11},'— нет данных'));
    return;
   }
   const x=px(v);
   const mark=make('rect',{x:Math.min(x,zero),y:ybar,width:Math.max(1,Math.abs(x-zero)),height:13,rx:2,fill:color(name),tabindex:0});
   const accessible=(options.label?options.label(name):name)+' · '+caption+': '+presentation.fmt(v,metric);
   mark.setAttribute('aria-label',accessible);mark.append(make('title',{},accessible));svg.append(mark);
  });
 });
 const wrap=document.createElement('div');wrap.className='horizontal-bar-scroll';wrap.append(svg);root.append(wrap);
 const legend=document.createElement('div');legend.className='chart-legend';
 for(const name of shown){
  const item=document.createElement('span');item.textContent=options.label?options.label(name):name;item.title=item.textContent;
  item.style.setProperty('--series',color(name));legend.append(item);
 }
 root.append(legend);
 if(!values.length){const message=document.createElement('p');message.className='muted small';message.textContent='Нет полных данных за показанные периоды. Пропуски не заменены нулями.';root.append(message);}
}
window.unifiedHorizontalBars=horizontalBars;
if(typeof module==='object'&&module?.exports)module.exports={horizontalBars};
})();