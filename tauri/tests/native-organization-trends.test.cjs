'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-organization-trends.js'),'utf8');
class Element{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.value='';this.className='';this.style={};this.attributes={};}
 append(...items){this.children.push(...items);}
 replaceChildren(...items){this.children=[...items];}
 setAttribute(key,value){this.attributes[key]=value;}
 addEventListener(type,handler){(this.listeners||(this.listeners={}))[type]=handler;}
}
function mount(){
 const ids=['hideEmptyPeriods','organizationTrendsMetric','organizationTrendsStatus','organizationTrendsChart',
            'organizationTrendsLegend','organizationTrendsRows'];
 const e=Object.fromEntries(ids.map(id=>[id,new Element(id)]));
 e.organizationTrendsMetric.value='amount';
 const window={nativePeriodVisibility:require('../native-period-visibility.js')},document={getElementById:id=>e[id],createElement:tag=>new Element(tag),
   createElementNS:(ns,tag)=>{assert.equal(ns,'http://www.w3.org/2000/svg');return new Element(tag);}};
 vm.runInNewContext(script,{window,document});
 return {window,e};
}
function find(root,tag){
 return root.children.flatMap(child=>[
   ...(child.tag===tag?[child]:[]),...find(child,tag)
 ]);
}
const point=(start,amount,count,missing=0)=>({
 start,periods:1,missing,
 total:missing?{amount:null,count:null}:{amount,count}
});
const series=(organization,points)=>({organization,points});
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('separate missing division breaks only its line; zeros and negatives remain values',()=>{
 const {window,e}=mount();
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:2,series:[
   series('A',[point('2024-02-28',10,2),point('2024-02-29',20,0,1),point('2024-03-01',-20,-3)]),
   series('B',[point('2024-02-28',0,0),point('2024-02-29',5,1),point('2024-03-01',10,2)])
 ]});
 assert.equal(e.organizationTrendsLegend.children.length,2);
 assert.equal(e.organizationTrendsRows.children.length,6);
 assert.equal(e.organizationTrendsRows.children[1].children[5].textContent,'—');
 assert.equal(e.organizationTrendsRows.children[3].children[5].textContent,'0');
 assert.equal(e.organizationTrendsRows.children[2].children[5].textContent,'-20');
 const svg=e.organizationTrendsChart.children[0];
 assert.equal(svg.tag,'svg');
 assert.equal(find(svg,'path').length,1,'only B has uninterrupted line');
 assert.equal(find(svg,'circle').length,5);
 assert.match(e.organizationTrendsStatus.textContent,/подтверждено 5 из 6/);
 e.organizationTrendsMetric.value='count';e.organizationTrendsMetric.listeners.change();
 assert.match(e.organizationTrendsStatus.textContent,/Количество \/ объём/);
 window.nativeOrganizationTrends.clear('Фильтры изменены');
 assert.equal(e.organizationTrendsRows.children.length,0);
 assert.equal(e.organizationTrendsChart.children.length,0);
});
test('subset is explicit, untrusted names are text, inconsistent series are rejected',()=>{
 const {window,e}=mount();
 const initial=series('<img src=x>',[point('2024-02-28',0,0)]);
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:9,series:[initial]});
 assert.equal(e.organizationTrendsLegend.children[0].children[1].textContent,'<img src=x>');
 assert.match(e.organizationTrendsStatus.textContent,/первые 1 из 9/);
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:2,series:[
   initial,series('B',[point('2024-03-01',2,1)])
 ]});
 assert.equal(e.organizationTrendsChart.children.length,0);
 assert.equal(e.organizationTrendsRows.children.length,0);
 assert.match(e.organizationTrendsStatus.textContent,/Расхождение периодов/);
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:9,
   series:Array.from({length:9},(_,i)=>series('Org'+i,[point('2024-02-28',i,i)]))});
 assert.equal(e.organizationTrendsChart.children.length,0);
 assert.match(e.organizationTrendsStatus.textContent,/Некорректный ответ/);
});

test('hidden zero periods still validate every original organization series',()=>{
 const {window,e}=mount();e.hideEmptyPeriods.checked=true;
 const a=point('2024-01-01',0,0),b={...point('2024-01-01',0,0),periods:2};
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:2,series:[series('A',[a]),series('B',[b])]});
 assert.match(e.organizationTrendsStatus.textContent,/Расхождение периодов/);
});
test('all confirmed zero periods hidden never imply missing data',()=>{
 const {window,e}=mount();e.hideEmptyPeriods.checked=true;
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:1,series:[series('A',[point('2024-01-01',0,0)])]});
 assert.match(e.organizationTrendsStatus.textContent,/нулевые и скрыты/);
 assert.ok(!e.organizationTrendsStatus.className.includes('error'));
});
test('division keeps its classic settings color when other divisions are filtered out',()=>{
 const {window,e}=mount();const points=[point('2024-02-28',10,1),point('2024-02-29',20,2)];
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:2,series:[series('A',points),series('B',points)]},['A','B']);
 const before=e.organizationTrendsLegend.children[1].children[0].style.backgroundColor;
 window.nativeOrganizationTrends.show({granularity:'period',selected_organizations:1,series:[series('B',points)]},['A','B']);
 assert.equal(e.organizationTrendsLegend.children[0].children[0].style.backgroundColor,before);
 e.organizationTrendsMetric.value='count';e.organizationTrendsMetric.listeners.change();
 assert.equal(e.organizationTrendsLegend.children[0].children[0].style.backgroundColor,before);
});
