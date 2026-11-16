const fs=require('node:fs');
const vm=require('node:vm');
const assert=require('node:assert/strict');
const {test}=require('node:test');

const html=fs.readFileSync('index.html','utf8');
const scripts=[...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)];
assert.equal(scripts.length,2,'The application must remain standalone with two inline scripts');

class MockNode {
  constructor(tag){
    this.tag=tag;
    this.children=[];
    this.dataset={};
    this.attributes={};
    this.clientWidth=900;
    this.style={setProperty(name,value){this[name]=value;}};
  }
  append(...nodes){this.children.push(...nodes);}
  replaceChildren(...nodes){this.children=[...nodes];}
  setAttribute(name,value){this.attributes[name]=value;}
  contains(){return false;}
  querySelector(){return null;}
  focus(){}
  querySelectorAll(){return [];}
}
const context=vm.createContext({
  console,
  window:{matchMedia:()=>({matches:false})},
  document:{
    createElement:tag=>new MockNode(tag),
    createElementNS:(_ns,tag)=>new MockNode(tag)
  }
});
vm.runInContext(scripts[1][1].replace(/\n(?:window\.appReady=)?main\(\);\s*$/,'\n'),context);
const run=(expression,values={})=>{
  Object.assign(context,values);
  return vm.runInContext(expression,context);
};

test('group coverage counts unique names from all sources, including saved groups',()=>{
  const result=run(`(()=>{
    const oldReports=state.reports,oldSettings=state.settings;
    try{
      state.reports=[{rows:[{work:'Альфа',organization:'СМУ16'},{work:'Альфа',organization:'АВТО'},{work:'Бета',organization:'АВТО'}]}];
      state.settings={...oldSettings,catalog:[{work:'Альфа'},{work:'Каталог'}],groups:[{id:'a',name:'Группа',works:['Альфа','Бета']},{id:'b',name:'Архив',works:['Старая работа']}]};
      return groupingCoverage();
    }finally{state.reports=oldReports;state.settings=oldSettings;}
  })()`);
  assert.equal(result.total,4);assert.equal(result.grouped,3);assert.equal(result.ungrouped,1);
});

test('three periods versus three reuse grouped weighted totals for days, weeks and months',()=>{
  for(const type of ['day','week','month']){
    const result=run(`(()=>{
      const organizations=['СМУ16','АВТО'],groups=[{id:'g',name:'Общая группа',works:['Альфа','Бета']}];
      const reports=Array.from({length:6},(_,i)=>{
        const start=shiftPeriod(type,type==='week'?'2026-09-07':'2026-01-01',i),k=i+1;
        return {id:start,type,start,end:periodEnd(start,type),organizations,totals:[],warnings:[],rows:[{organization:organizations[0],work:'Альфа',count:k/2,amount:10*k*k},{organization:organizations[1],work:'Бета',count:1.5*k,amount:20*k*k}]};
      });
      const index=buildReportIndex(reports,groups,{type,organizations});
      const before=aggregateReportRange(index,{from:reports[0].start,to:reports[2].start,organizations,names:new Set(['Общая группа'])});
      const after=aggregateReportRange(index,{from:reports[3].start,to:reports[5].start,organizations,names:new Set(['Общая группа'])});
      const changes=analyzeContributions(before,after);
      return {before,after,changes,groups:before.by.size};
    })()`,{type});
    assert.equal(result.before.complete,true);assert.equal(result.after.complete,true);assert.equal(result.groups,1);
    assert.equal(result.before.total.count,12);assert.equal(result.before.total.amount,420);assert.equal(result.before.total.average,35);
    assert.equal(result.after.total.count,30);assert.equal(result.after.total.amount,2310);assert.equal(result.after.total.average,77);
    assert.equal(result.changes.amount.delta,1890);assert.equal(result.changes.amount.reconciled,true);assert.equal(result.changes.count.delta,18);
  }
});

test('range comparisons do not turn missing, duplicate or overlapping reports into zero',()=>{
  const result=run(`(()=>{
    const organizations=['СМУ16','АВТО'];
    const make=i=>{const start=shiftPeriod('week','2026-09-07',i);return {id:String(i),type:'week',start,end:periodEnd(start,'week'),organizations,rows:organizations.map(organization=>({organization,work:'Работа',count:0,amount:0})),totals:[],warnings:[]};};
    const reports=Array.from({length:6},(_,i)=>make(i));
    const get=items=>{const index=buildReportIndex(items,[],{type:'week',organizations});return aggregateReportRange(index,{from:reports[0].start,to:reports[2].start,organizations});};
    const zero=get(reports),missing=get(reports.filter((_,i)=>i!==1)),duplicate=get([...reports,{...reports[1],id:'copy'}]);
    const start=addDays(reports[1].start,1),overlap=get([...reports,{...reports[1],id:'overlap',start,end:periodEnd(start,'week')}]);
    return {zero,missing,duplicate,overlap,zeroPercent:changeMeasure(0,12,0,0).percent};
  })()`);
  assert.equal(result.zero.complete,true);assert.equal(result.zero.total.count,0);assert.equal(result.zero.total.amount,0);assert.equal(result.zero.total.average,null);assert.equal(result.zeroPercent,null);
  for(const key of ['missing','duplicate','overlap']){assert.equal(result[key].complete,false,key);assert.equal(result[key].total.amount,null,key);}
});

test('name search puts prefixes before substrings for every query length',()=>{
  const names=['Проверка датчика','Датчик Б','Замена датчика','Демонтаж','Датчик А','Диагностика'];
  assert.deepEqual(Array.from(run("searchNames(names,' д ')",{names})),['Датчик А','Датчик Б','Демонтаж','Диагностика','Замена датчика','Проверка датчика']);
  assert.deepEqual(Array.from(run("searchNames(names,'ДИ')",{names})),['Диагностика']);
  assert.deepEqual(Array.from(run("searchNames(names,'ДАТ')",{names})),['Датчик А','Датчик Б','Замена датчика','Проверка датчика']);
  assert.deepEqual(Array.from(run("searchNames(names,'   ')",{names})),names,'Empty query retains the original order');
  assert.equal(names[0],'Проверка датчика','Search never mutates the input');
  assert.equal(run("searchNames(names,'нет совпадений').length",{names}),0);
  const go=['Автомобиль','Агрегат горелки','Ремонт горелки','Гофра','Горелка','Головка блока'];
  assert.deepEqual(Array.from(run("searchNames(go,'го')",{go})),['Головка блока','Горелка','Гофра','Агрегат горелки','Ремонт горелки']);
});

test('long queries can match related group names while short queries match the displayed name',()=>{
  const entries=[{name:'Замена узла',group:'Датчики'},{name:'Датчик Б',group:''},{name:'Датчик А',group:''}];
  assert.deepEqual(Array.from(run("searchNames(entries,'дат',x=>x.name,x=>[x.group]).map(x=>x.name)",{entries})),['Датчик А','Датчик Б','Замена узла']);
  assert.deepEqual(Array.from(run("searchNames(entries,'да',x=>x.name,x=>[x.group]).map(x=>x.name)",{entries})),['Датчик А','Датчик Б']);
});
const nodes=root=>[root,...root.children.flatMap(nodes)];

test('daily dates, leap days and missing day diagnostics use one-day steps',()=>{
  assert.equal(run("periodEnd('2024-02-28','day')"),'2024-02-28');
  assert.equal(run("shiftPeriod('day','2024-02-28',1)"),'2024-02-29');
  assert.equal(run("shiftPeriod('day','2024-03-01',-1)"),'2024-02-29');
  assert.equal(run("displayPeriod({type:'day',start:'2026-11-16',end:'2026-11-16'})"),'16.11.2026');
  assert.equal(run("plotPeriodLabel({type:'day',start:'2026-11-16',end:'2026-11-16'})"),'16.11');
  assert.deepEqual(Array.from(run("rangePeriods('day','2024-02-28','2024-03-01').map(p=>p.start)")),['2024-02-28','2024-02-29','2024-03-01']);
  const result=run(`(()=>{
    const organizations=['СМУ16'],report=start=>({id:start,type:'day',start,end:start,organizations,rows:[{organization:'СМУ16',work:'Работа',count:0.5,amount:10}],totals:[],warnings:[]});
    const reports=[report('2026-11-01'),report('2026-11-03')],index=buildReportIndex(reports,[],{type:'day',organizations}),range=aggregateReportRange(index,{from:'2026-11-01',to:'2026-11-03',organizations});
    const gaps=collectQualityIssues(reports,organizations).filter(issue=>issue.type==='missing'&&issue.period?.type==='day');
    const duplicate=buildReportIndex([...reports,{...reports[0],id:'copy'}],[],{type:'day',organizations});
    return {range,gaps,duplicate};
  })()`);
  assert.equal(result.range.complete,false);assert.equal(result.range.total.amount,null);
  assert.ok(result.gaps.some(issue=>issue.period.start==='2026-11-02'));
  assert.ok(result.duplicate.issues.some(issue=>issue.type==='duplicate'));
});

test('daily baseline and changes ignore overlapping week and month totals',()=>{
  const result=run(`(()=>{
    const organizations=['СМУ16'],report=(start,i,type='day')=>({id:start+type,type,start,end:periodEnd(start,type),organizations,rows:[{organization:'СМУ16',work:'Работа',count:i,amount:i*i*10}],totals:[],warnings:[]});
    const days=Array.from({length:6},(_,i)=>report(addDays('2026-11-01',i),i+1));
    const reports=[...days,report('2026-11-01',1000,'week'),report('2026-11-01',2000,'month')];
    const index=buildReportIndex(reports,[],{type:'day',organizations}),total=aggregateReportRange(index,{from:'2026-11-01',to:'2026-11-06',organizations});
    const usual=analyzeUsual(index,{period:'2026-11-06',window:4,organizations});
    const regularity=analyzeRegularity(index,{from:'2026-11-01',to:'2026-11-06',organizations,longGap:4});
    const changes=analyzeWorkChanges(reports,[],{type:'day',method:'previous',from:'2026-11-06',to:'2026-11-06',organizations,percent:0,minCount:0,minAmount:0});
    const pairs=analyzeWorkChanges(reports,[],{type:'day',method:'range',from:'2026-11-01',to:'2026-11-03',organizations,percent:0,minCount:0,minAmount:0});
    return {total,usual,regularity,changes,pairs};
  })()`);
  assert.equal(result.total.total.count,21);assert.equal(result.total.total.amount,910);
  assert.equal(result.usual.used,4);assert.equal(result.usual.rows[0].count.base,3.5);assert.equal(result.usual.rows[0].amount.base,135);
  assert.equal(result.regularity.used,6);assert.equal(result.regularity.expected,6);
  assert.equal(result.changes.rows.length,1);assert.equal(result.changes.rows[0].basePeriod.start,'2026-11-05');assert.equal(result.changes.rows[0].amount.delta,110);
  assert.equal(result.pairs.rows.length,2);
});
const organizations=['СМУ16','АВТО'];
const metrics=['amount','count','average'];
const periods=[
  {type:'week',start:'2026-10-19',end:'2026-10-25'},
  {type:'week',start:'2026-10-26',end:'2026-11-01'}
];
const data=[
  new Map([['СМУ16',{amount:100,count:2,average:50}],['АВТО',{amount:200,count:4,average:50}]]),
  new Map([['СМУ16',{amount:60,count:3,average:20}],['АВТО',{amount:120,count:2,average:60}]])
];

test('two departments and three metrics produce six distinct series',()=>{
  const series=run('departmentMultiSeries(organizations,metrics,data)',{organizations,metrics,data});
  assert.equal(series.length,6);
  assert.equal(new Set(series.map(s=>s.label)).size,6);
  assert.deepEqual(Array.from(series.filter(s=>s.metric==='average'),s=>s.values[1]),[20,60]);
  const maxima=run('departmentScaleMax(series)',{series});
  assert.equal(maxima.get('amount'),200);
  assert.equal(maxima.get('count'),4);
  assert.equal(maxima.get('average'),60);
});

test('line and bar modes render six series with correct labels and source-value tooltips',()=>{
  for(const [mode,shape,expected] of [['line','polyline',6],['bar','rect',12]]){
    const root=new MockNode('div');
    run('state.chart=mode;drawDepartmentMultiChart(periods,organizations,metrics,data,root)',{
      root,mode,periods,organizations,metrics,data
    });
    const all=nodes(root);
    const plot=all.find(node=>node.tag==='svg'&&node.attributes.role==='img');
    assert.ok(plot,'График SVG существует');
    assert.equal(nodes(plot).filter(node=>node.tag===shape).length,expected);
    assert.equal(all.filter(node=>node.attributes.role==='listitem').length,6);
    const marks=all.filter(node=>node.dataset.chartTooltip);
    assert.equal(marks.length,12);
    assert.ok(marks.some(node=>node.dataset.chartTooltip.includes('СМУ16 · Сумма')));
    assert.ok(marks.some(node=>node.dataset.chartTooltip.includes('АВТО · Средняя')));
  }
});

test('missing values are gaps, while real zeros are plotted',()=>{
  const missing=[
    data[0],
    new Map([['СМУ16',{amount:0,count:0,average:null}],['АВТО',{amount:null,count:null,average:null}]])
  ];
  const series=run('departmentMultiSeries(organizations,metrics,missing)',{organizations,metrics,missing});
  assert.equal(series.find(s=>s.organization==='АВТО'&&s.metric==='count').values[1],null);
  assert.equal(series.find(s=>s.organization==='СМУ16'&&s.metric==='count').values[1],0);
  const root=new MockNode('div');
  run('state.chart="line";drawDepartmentMultiChart(periods,organizations,metrics,missing,root)',{
    root,periods,organizations,metrics,missing
  });
  assert.equal(nodes(root).filter(node=>node.tag==='circle').length,8);
  assert.equal(nodes(root).filter(node=>node.tag==='polyline').length,2);
});

test('negative values use absolute maxima for normalized scale',()=>{
  const negative=run("departmentScaleMax(departmentMultiSeries(['A'],['amount'],[new Map([['A',{amount:-350}]])]))");
  assert.equal(negative.get('amount'),350);
});

test('average is calculated per period; incomplete department remains null',()=>{
  const row=(organization,count,amount)=>({organization,work:'Работа',count,amount});
  const reports=[
    {id:'w1',type:'week',start:'2026-10-19',end:'2026-10-25',
      rows:[row('СМУ16',2,100),row('АВТО',4,200)],
      organizations:['СМУ16','АВТО'],totals:[],warnings:[]},
    {id:'w2',type:'week',start:'2026-10-26',end:'2026-11-01',
      rows:[row('СМУ16',0,0)],
      organizations:['СМУ16'],totals:[],warnings:[]}
  ];
  const index=run("buildReportIndex(reports,[],{type:'week',organizations})",{reports,organizations});
  const trend=run("departmentTrendData(index,'2026-10-19','2026-10-26',organizations)",{index,organizations});
  assert.equal(trend.periods.length,2);
  assert.equal(trend.data[0].get('СМУ16').average,50);
  assert.equal(trend.data[0].get('АВТО').average,50);
  assert.equal(trend.data[1].get('СМУ16').count,0);
  assert.equal(trend.data[1].get('СМУ16').average,null);
  assert.equal(trend.data[1].get('АВТО').average,null);
});

test('solid/dashed/dotted line styles are tied to metrics, not selection order',()=>{
  assert.equal(run("departmentMetricStyle('amount').dash"),null);
  assert.equal(run("departmentMetricStyle('count').dash"),'8 4');
  assert.equal(run("departmentMetricStyle('average').dash"),'2 4');
  const root=new MockNode('div'),selected=['average','count'];
  run('state.chart="line";drawDepartmentMultiChart(periods,organizations,selected,data,root)',{
    root,periods,organizations,selected,data
  });
  const all=nodes(root);
  const legendItems=all.filter(node=>node.attributes.role==='listitem');
  assert.equal(legendItems.length,4);
  // Для каждого подразделения первая линия относится к средней, вторая — к количеству.
  for(let i=0;i<legendItems.length;i+=2){
    const dotted=legendItems[i].children[0].children[0];
    const dashed=legendItems[i+1].children[0].children[0];
    assert.equal(dotted.attributes['stroke-dasharray'],'2 4');
    assert.equal(dashed.attributes['stroke-dasharray'],'8 4');
    assert.equal(dotted.attributes.stroke,dashed.attributes.stroke,'Цвет одного подразделения одинаков');
  }
  const linePaths=all.filter(node=>node.tag==='polyline');
  assert.ok(linePaths.some(node=>node.attributes['stroke-dasharray']==='2 4'));
  assert.ok(linePaths.some(node=>node.attributes['stroke-dasharray']==='8 4'));
});

test('bar legend swatches match fill intensity of their plotted metrics',()=>{
  const root=new MockNode('div');
  run('state.chart="bar";drawDepartmentMultiChart(periods,organizations,metrics,data,root)',{
    root,periods,organizations,metrics,data
  });
  const all=nodes(root),items=all.filter(node=>node.attributes.role==='listitem');
  assert.equal(items.length,6);
  assert.equal(items[0].children[0].children[0].attributes['fill-opacity'],'1');
  assert.equal(items[1].children[0].children[0].attributes['fill-opacity'],'0.72');
  assert.equal(items[2].children[0].children[0].attributes['fill-opacity'],'0.48');
  const plot=all.find(node=>node.tag==='svg'&&node.attributes.role==='img');
  assert.ok(plot);
  assert.equal(nodes(plot).filter(node=>node.tag==='rect').length,12);
  assert.equal(items[0].children[0].children[0].attributes.fill,items[1].children[0].children[0].attributes.fill);
});

test('raw work names and internal work keys keep the same chart color',()=>{
  assert.equal(run("workSeriesColor('Кран')"),run("workSeriesColor('work:Кран')"));
  assert.equal(run("workSeriesColor('Ремонт')"),run("workSeriesColor('work:Ремонт')"));
  run("state.settings.groups=[{id:'g-1',name:'Ремонтная группа',works:['Кран']}]");
  assert.equal(run("workSeriesColor('Ремонтная группа')"),run("workSeriesColor('group:g-1')"));
  // Переименование не меняет цвет группы, если её идентификатор остался прежним.
  run("state.settings.groups[0].name='Новое название'");
  assert.equal(run("workSeriesColor('Новое название')"),run("workSeriesColor('group:g-1')"));
  run("state.settings.groups=[]");
});

test('chart keeps lines across consecutive weeks and breaks across an absent week',()=>{
  const first=periods[0],second=periods[1];
  const late={type:'week',start:'2026-11-09',end:'2026-11-15'};
  const later={type:'week',start:'2026-11-16',end:'2026-11-22'};
  const seriesData=[100,150,200,250].map(amount=>new Map([['Кран',{amount}]]));
  const draw=(chosen,chosenData)=>{
    const root=new MockNode('div');
    run("state.chart='line';drawChart(chosen,new Set(['СМУ16']),'amount',root,{categories:['Кран'],data:chosenData,chart:'line'})",{
      chosen,chosenData,root
    });
    return {root,items:nodes(root)};
  };
  const adjacent=draw([first,second],seriesData.slice(0,2));
  assert.equal(adjacent.items.filter(node=>node.tag==='polyline').length,1);
  assert.equal(adjacent.items.filter(node=>node.tag==='circle').length,2);
  const separated=draw([first,late],seriesData.slice(0,2));
  assert.equal(separated.items.filter(node=>node.tag==='polyline').length,0);
  assert.equal(separated.items.filter(node=>node.tag==='circle').length,2);
  assert.ok(separated.items.some(node=>node.textContent?.includes('Пропуск')));
  assert.ok(separated.items.some(node=>node.textContent?.includes('Линии через пропуски не соединяются')));
  const doubleSegment=draw([first,second,late,later],seriesData);
  assert.equal(doubleSegment.items.filter(node=>node.tag==='polyline').length,2);
  assert.equal(doubleSegment.items.filter(node=>node.tag==='circle').length,4);
});

test('missing monthly period is identified without replacing it with zero',()=>{
  const periodsWithGap=[
    {type:'month',start:'2026-12-01',end:'2026-12-31'},
    {type:'month',start:'2027-02-01',end:'2027-02-28'}
  ];
  const gaps=run('chartTimelineGaps(periodsWithGap)',{periodsWithGap});
  assert.equal(gaps.length,1);
  assert.equal(gaps[0].first,'2027-01-01');
  assert.equal(gaps[0].last,'2027-01-01');
});

test('numeric movement colors distinguish increase, decrease and no change without assigning quality',()=>{
  assert.equal(run('numericChangeClass(20)'),'numeric-rise');
  assert.equal(run('numericChangeClass(-5)'),'numeric-fall');
  assert.equal(run('numericChangeClass(0)'),'');
  assert.equal(run('numericChangeClass(null)'),'');
  assert.equal(run('numericChangeClass(NaN)'),'');
  assert.equal(run('overviewTrendClass(120,100)'),'numeric-rise');
  assert.equal(run('overviewTrendClass(80,100)'),'numeric-fall');
  assert.equal(run('overviewTrendClass(100,100)'),'');
  assert.equal(run('overviewTrendClass(null,100)'),'');
});

test('unusual high and low levels are distinct, neither marked as positive or negative quality',()=>{
  assert.equal(run("usualStatusClass('high')"),'numeric-rise');
  assert.equal(run("usualStatusClass('low')"),'numeric-fall');
  assert.equal(run("usualStatusClass('normal')"),'');
  assert.equal(run("usualStatusClass('insufficient')"),'muted');
});

test('both themes provide directional colors and good/bad colors remain semantically separate',()=>{
  assert.match(html,/\.numeric-rise\{color:var\(--change-rise\)/);
  assert.match(html,/\.numeric-fall\{color:var\(--change-fall\)/);
  assert.match(html,/:root\[data-theme="dark"\]\{--change-rise:[^}]+--change-fall:/);
  assert.match(html,/\.quality-good\{color:var\(--positive\)!important/);
  assert.match(html,/\.quality-bad\{color:var\(--negative\)!important/);
  assert.match(html,/qualitySummary\.replaceChildren\(/);
  assert.match(html,/\.calendar-complete small\{color:var\(--positive\)/);
  assert.match(html,/\.calendar-overlap small,\.calendar-partial small\{color:var\(--negative\)/);
});

test('comparison KPI cards display blue and purple numeric directions',()=>{
  const root=new MockNode('div');
  context.document.getElementById=id=>id==='kpis'?root:null;
  const before={complete:true,periods:[periods[0]],total:{count:2,amount:100}};
  const after={complete:true,periods:[periods[1]],total:{count:5,amount:70}};
  run('renderKpis(after,before,new Set(organizations),["count","amount"])',{before,after,organizations});
  assert.equal(root.children.length,4);
  assert.equal(root.children[1].children[1].className,'numeric-rise');
  assert.equal(root.children[3].children[1].className,'numeric-fall');
  context.document.getElementById=undefined;
});

