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
vm.runInContext(scripts[1][1].replace(/\nmain\(\);\s*$/,'\n'),context);
const run=(expression,values={})=>{
  Object.assign(context,values);
  return vm.runInContext(expression,context);
};
const nodes=root=>[root,...root.children.flatMap(nodes)];
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
  assert.deepEqual(series.filter(s=>s.metric==='average').map(s=>s.values[1]),[20,60]);
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
    assert.equal(all.filter(node=>node.tag===shape).length,expected);
    assert.equal(all.filter(node=>node.tag==='span').length,6);
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
