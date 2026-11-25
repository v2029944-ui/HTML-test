// Read-only, bounded SQLite comparison output. Does not load historic reports.
(function(){
'use strict';
const $=id=>document.getElementById(id);
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const numeric=n=>finite(n)?n.toLocaleString('ru-RU',{maximumFractionDigits:3}):'—';
const change=m=>m&&finite(m.delta)?(m.delta>0?'+':'')+numeric(m.delta):'—';
function percentage(m){
 return m&&finite(m.percent)?(m.percent>0?'+':'')+numeric(m.percent)+' %':'—';
}
const elt=(tag,text)=>{
 const el=document.createElement(tag);
 if(text!==undefined)el.textContent=String(text);
 return el;
};
let data=null;
function status(id,message,bad=false){
 const el=$(id);el.textContent=message;el.className='status '+(bad?'error':'muted');
}
function clear(message='Укажите базовые даты и выполните сравнение.'){
 data=null;
 $('comparisonMetrics').replaceChildren();
 $('comparisonDivisionRows').replaceChildren();
 $('comparisonRankRows').replaceChildren();
 status('comparisonStatus',message);
 status('comparisonRankStatus','Подборка появится после подтверждения обоих диапазонов.');
}
function validMeasure(m){
 return m===null||(m&&finite(m.base)&&finite(m.current)&&finite(m.delta)&&
   (m.percent===null||finite(m.percent)));
}
const hasReports=period=>period.complete||period.available_reports>0;
function showSummary(host,label,measure){
 const card=elt('div');card.className='comparison-metric';
 card.append(elt('strong',label));
 for(const line of [
   'Было: '+(measure?numeric(measure.base):'—'),
   'Стало: '+(measure?numeric(measure.current):'—'),
   'Изменение: '+change(measure),
   'Изменение, %: '+percentage(measure)
 ])card.append(elt('p',line));
 host.append(card);
}
function renderRanking(){
 const host=$('comparisonRankRows');host.replaceChildren();
 if(!data)return;
 const metric=$('comparisonRankMetric').value==='count'?'count':'amount';
 const list=metric==='count'?data.top_count:data.top_amount;
 if(!Array.isArray(list)||list.length>12)throw Error('Некорректный объём рейтинга');
 if(!hasReports(data.summary.before)||!hasReports(data.summary.after)){
   if(list.length)throw Error('Рейтинг требует отчётов за оба диапазона');
   status('comparisonRankStatus','Подборка скрыта: за один из диапазонов нет пригодных отчётов.',true);
   return;
 }
 let max=0;
 for(const item of list){
   if(!item||typeof item.name!=='string'||!['work','group'].includes(item.kind)||
      !validMeasure(item[metric])||item[metric]===null)
     throw Error('Некорректные значения изменения работ');
   max=Math.max(max,Math.abs(item[metric].delta));
 }
 for(const item of list){
   const li=elt('li'),head=elt('div');head.className='comparison-rank-row';
   const name=elt('span',item.name+' ('+(item.kind==='group'?'группа':'работа')+')');
   name.className='comparison-rank-name';
   const number=elt('span',change(item[metric])+' · '+percentage(item[metric]));
   number.className='comparison-rank-value';
   head.append(name,number);
   const track=elt('div');track.className='comparison-rank-track';
   track.setAttribute('aria-hidden','true');
   const fill=elt('div');
   fill.className='comparison-rank-fill'+(item[metric].delta<0?' decrease':'');
   fill.style.width=max?(Math.abs(item[metric].delta)/max*100)+'%':'0%';
   track.append(fill);li.append(head,track);host.append(li);
 }
 status('comparisonRankStatus',list.length?
   'Крупнейшие '+list.length+' изменений по модулю. Полосы показывают относительную величину изменения, не долю суммы. Рост и снижение не означают улучшение или ухудшение.':
   'Нет работ или групп с изменениями в выбранных диапазонах.');
}
function draw(){
 const metrics=$('comparisonMetrics'),rows=$('comparisonDivisionRows');
 metrics.replaceChildren();rows.replaceChildren();$('comparisonRankRows').replaceChildren();
 if(!data)return;
 const s=data.summary;
 if(!s||!s.before||!s.after||typeof s.before.complete!=='boolean'||
    typeof s.after.complete!=='boolean'||!Array.isArray(data.divisions)||
    data.divisions.length>100||!Array.isArray(data.top_amount)||
    !Array.isArray(data.top_count)||
    !validMeasure(s.count)||!validMeasure(s.amount))
   throw Error('Некорректный ответ SQL-сравнения');
 const both=s.before.complete&&s.after.complete;
 if((!hasReports(s.before)||!hasReports(s.after))&&(s.count!==null||s.amount!==null))
   throw Error('Расчёт изменения требует отчётов за оба диапазона');
 showSummary(metrics,'Количество / объём',s.count);
 showSummary(metrics,'Сумма',s.amount);
 for(const org of data.divisions){
   if(typeof org.organization!=='string'||!org.before||!org.after||
      !validMeasure(org.count)||!validMeasure(org.amount))
     throw Error('Некорректные данные подразделения');
   const before=org.before.amount,after=org.after.amount;
   if((before===null||after===null)&&(org.count!==null||org.amount!==null)||
      org.amount!==null&&(!finite(before)||!finite(after))||
      before!==null&&!finite(before)||after!==null&&!finite(after))
     throw Error('Неполное подразделение содержит недостоверные изменения');
   const tr=elt('tr');
   for(const v of [
     org.organization,numeric(before),numeric(after),
     change(org.amount),change(org.count)
   ])tr.append(elt('td',v));
   rows.append(tr);
 }
 renderRanking();
 status('comparisonStatus',
   'Сравнение выполнено. Базовый диапазон: '+(s.before.complete?'полный':'есть пропуски')+
   '; текущий диапазон: '+(s.after.complete?'полный':'есть пропуски')+
   (both?'.':'. Итоги рассчитаны по доступным отчётам.')+' Единицы исходных отчётов не подтверждены.',
   !both);
}
function show(result){
 data=result;
 try{draw();}catch(error){
   clear('Невозможно достоверно показать сравнение: '+error.message);
   status('comparisonStatus','Невозможно достоверно показать сравнение: '+error.message,true);
 }
}
function error(message){clear(message);status('comparisonStatus',message,true);}
$('comparisonRankMetric').addEventListener('change',()=>{
 if(!data)return;
 try{renderRanking();}catch(err){error('Невозможно показать рейтинг изменений: '+err.message);}
});
window.nativeComparison=Object.freeze({show,clear,error});
})();
