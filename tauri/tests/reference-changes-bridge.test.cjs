'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
test('reference changes bridge preserves both intervals and tracks complete CSV separately',async()=>{
 const calls=[],events=[],request={revision:7,reportType:'day',from:'2024-01-10',to:'2024-01-11',baseFrom:'2023-12-01',baseTo:'2023-12-31',organizations:['A'],metric:'amount',percent:30,minCount:1,minAmount:10,kind:'all',search:'Work',sort:'delta',page:2};
 const window={__TAURI__:{core:{invoke:async(command,args)=>{calls.push({command,args});return {ok:true};}},dialog:{open:async()=>null}},dispatchEvent:event=>events.push(event)};
 class CustomEvent{constructor(type,value){this.type=type;this.detail=value.detail;}}
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window,CustomEvent});
 await window.desktopAPI.referenceChanges(request);await window.desktopAPI.referenceChangesCsv(request,'C:/export/full.csv');await window.desktopAPI.referenceSummaryCsv(request,'C:/export/summary.csv');
 assert.deepEqual(JSON.parse(JSON.stringify(calls)),[{command:'reference_changes',args:{request}},{command:'reference_changes_csv',args:{request,destination:'C:/export/full.csv'}},{command:'reference_summary_csv',args:{request,destination:'C:/export/summary.csv'}}]);
 assert.ok(events.some(e=>e.detail.reads===1));assert.ok(events.some(e=>e.detail.saves===1));assert.equal(window.tauriOperations.reads,0);assert.equal(window.tauriOperations.saves,0);
});
