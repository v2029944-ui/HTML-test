// SQL-only ranking renderer. Receives at most 12 aggregate rows, no source report JSON.
(function(){
'use strict';
const $=id=>document.getElementById(id);
let data=null;
const finite=x=>typeof x==='number'&&Number.isFinite(x);
const format=x=>x.toLocaleString('ru-RU',{maximumFractionDigits:3});
function status(message,bad=false){
 const el=$('rankingStatus');el.textContent=message;el.className='status '+(bad?'error':'muted');
}
function clear(message='Рейтинг появится после расчёта.'){
 data=null;$('rankingBars').replaceChildren();status(message);
}
function draw(){
 const host=$('rankingBars');host.replaceChildren();
 if(!data)return;
 if(typeof data.complete!=='boolean'||!Array.isArray(data.rows)||data.rows.length>12)
   throw Error('Некорректная выборка рейтинга');
 if(!data.complete){
   if(data.rows.length)throw Error('Неполный рейтинг не может содержать числовые строки');
   status('Рейтинг скрыт: есть пропуски периодов или подразделений. Неполные значения не ранжируются.',true);
   return;
 }
 const metric=$('rankingMetric').value==='count'?'count':'amount';
 const category=$('rankingCategory').value;
 const shown=data.rows;
 const expectedType=category==='all'?null:category;
 const validated=[];
 for(const row of shown){
   if(typeof row.name!=='string'||typeof row.key!=='string'||
      !['work','group'].includes(row.kind)||
      (expectedType!==null&&row.kind!==expectedType)||
      !finite(row.count)||!finite(row.amount))throw Error('Некорректное значение рейтинга');
   validated.push({name:row.name,kind:row.kind,value:row[metric]});
 }
 const max=Math.max(0,...validated.map(x=>Math.abs(x.value)));
 for(const entry of validated){
   const li=document.createElement('li');
   const heading=document.createElement('div');heading.className='ranking-heading';
   const name=document.createElement('span');name.className='ranking-name';
   name.textContent=entry.name+' ('+(entry.kind==='group'?'группа':'работа')+')';
   const number=document.createElement('span');number.className='ranking-value';
   number.textContent=format(entry.value);
   heading.append(name,number);
   const track=document.createElement('div');track.className='ranking-track';
   track.setAttribute('aria-hidden','true');
   const bar=document.createElement('div');
   bar.className='ranking-fill'+(entry.value<0?' negative':'');
   bar.style.width=max?Math.min(100,Math.abs(entry.value)/max*100)+'%':'0%';
   track.append(bar);li.append(heading,track);host.append(li);
 }
 status(shown.length
   ?'Показано '+shown.length+' позиций по модулю '+(metric==='count'?'количества/объёма':'суммы')+
    '. Длина полосы — относительный размер, не доля общего итога. Единицы исходного отчёта не подтверждены.'
   :'За выбранный подтверждённый диапазон нет позиций этой категории.');
}
function show(result){
 data=result;
 try{draw();}catch(error){clear('Рейтинг не отображён: '+error.message);status('Рейтинг не отображён: '+error.message,true);}
}
function error(message){clear(message);status(message,true);}
window.nativeRanking=Object.freeze({show,clear,error});
})();