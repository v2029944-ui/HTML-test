// Presentation-only filtering. Never feeds totals, completeness, or SQL exports.
(function(){
'use strict';
function confirmedZero(p){
 return p&&typeof p.start==='string'&&/^\d{4}-\d{2}(?:-\d{2})?$/.test(p.start)&&
  Number.isSafeInteger(p.periods)&&p.periods>0&&p.missing===0&&
  p.total?.count===0&&p.total?.amount===0;
}
function visiblePoints(points,hide){
 return hide&&Array.isArray(points)&&points.length<=124?points.filter(p=>!confirmedZero(p)):points;
}
function visibleSeries(series,hide){
 if(!hide||!series.length||!series.every(s=>Array.isArray(s.points)&&s.points.length<=124&&s.points.length===series[0].points.length))return series;
 const keep=series[0].points.map((p,i)=>!series.every(s=>s.points[i].start===p.start&&confirmedZero(s.points[i])));
 return series.map(s=>({...s,points:s.points.filter((_,i)=>keep[i])}));
}
const api={visiblePoints,visibleSeries};
if(typeof module!=='undefined')module.exports=api;
if(typeof window!=='undefined')window.nativePeriodVisibility=Object.freeze(api);
})();
