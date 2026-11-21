/* Shared presentation and in-flight reads for the shell and the detailed overview. */
(function () {
  'use strict';
  const titles = Object.freeze({amount:'Сумма', count:'Количество / объём', average:'Сумма / количество (объём)'});
  let current = null;
  function read(api, scope) {
    const {revision,type,source,displayType,from,to,baseFrom,baseTo,organizations} = scope;
    const key = JSON.stringify([window.sharedWorkspace?.state?.workspace_token, revision, source, type, displayType,
      from, to, baseFrom, baseTo, organizations, scope.allNames, scope.selectedNames, scope.selectedKeys]);
    if (current?.key === key) return current.result;
    // These two dashboards both show totals over all works. The work selection
    // is still part of the identity so different filter actions cannot share a pending result.
    const result = Promise.all([api.nativeAnalysisCompare(revision,type,baseFrom,baseTo,from,to,organizations,null,null),
      api.nativeAnalysisOrganizations(revision,type,from,to,organizations,null,null)])
      .finally(() => { if (current?.result === result) current = null; });
    current = {key,result};
    return result;
  }
  function displayedChange(delta,metric,format){
   if(!Number.isFinite(delta))return {value:'—',label:'—',direction:'neutral'};
   const value=format(Math.abs(delta),metric);
   if(Math.abs(delta)<1e-9)return {value,label:'Без изменений',direction:'neutral'};
   if(!/[1-9]/.test(value))return {value,label:'≈ без изменений',direction:'neutral'};
   const signed=(delta>0?'+':'−')+value;
   return {value:signed,label:(delta>0?'▲ ':'▼ ')+signed,direction:delta>0?'up':'down'};
  }
  function itemLabel(row) { return row.name + (row.kind === 'group' ? ' (группа)' : ''); }
  function timelinePeriods(points, granularity, scope, periodEnd) {
    const type = granularity === 'period' ? scope.displayType : granularity;
    return points.map(point => {
      const start = point.start.length === 7 ? point.start + '-01' : point.start;
      const calendarEnd = periodEnd(start,type);
      const actualStart = point.first_start || (scope.source === 'daily' && scope.from > start ? scope.from : start);
      const end = point.last_start ? periodEnd(point.last_start,scope.source === 'daily' ? 'day' : scope.displayType) :
        scope.source === 'daily' && scope.to < calendarEnd ? scope.to : calendarEnd;
      return {type,start,actualStart,end,partial:actualStart !== start || end !== calendarEnd};
    });
  }
  window.analysisSummary = Object.freeze({titles,read,timelinePeriods,displayedChange,itemLabel});
})();
