'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const {visiblePoints,visibleSeries}=require('../native-period-visibility.js');
const point=(start,count,amount,missing=0)=>({start,periods:1,missing,total:{count,amount}});
test('hides only confirmed double-zero periods, preserves missing, negative and malformed data',()=>{
 const points=[point('2024-01-01',0,0),point('2024-01-02',null,null,1),point('2024-01-03',1,0),point('2024-01-04',0,-2),{total:{count:0,amount:0}}];
 assert.deepEqual(visiblePoints(points,false),points);
 assert.deepEqual(visiblePoints(points,true),points.slice(1));
 assert.equal(points.length,5,'visibility must not change source data or KPI inputs');
});
test('organization timelines hide a period only when every displayed division has a confirmed zero',()=>{
 const a=[point('2024-01-01',0,0),point('2024-01-02',0,0),point('2024-01-03',null,null,1)];
 const b=[point('2024-01-01',0,0),point('2024-01-02',2,0),point('2024-01-03',0,0)];
 const series=[{organization:'A',points:a},{organization:'B',points:b}];
 const result=visibleSeries(series,true);
 assert.deepEqual(result.map(s=>s.points.map(p=>p.start)),[['2024-01-02','2024-01-03'],['2024-01-02','2024-01-03']]);
 assert.equal(a.length,3);
});
