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

test('print validates batch offsets, acknowledges delivery and waits for both the channel and native completion',async()=>{
 let command,args,finish,fail;const replies=[];
 class Channel{}
 const window={__TAURI__:{core:{Channel,invoke:(name,params)=>{
   command=name;args=params;return new Promise((resolve,reject)=>{finish=resolve;fail=reject;});
 }},event:{emit:async(name,value)=>replies.push([name,value])},dialog:{open:async()=>null}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),{window});
 const send=event=>args.batches.onmessage(event),ready=()=>send({kind:'ready',control:'print-7'});
 const request={revision:1,kind:'all',page:19,search:'Work'},collected=[];
 let done=false;const read=window.desktopAPI.referenceChangesPrint(request,batch=>collected.push(...batch)).then(result=>{done=true;return result;});
 assert.equal(command,'reference_changes_print');assert.equal(args.request,request);assert.equal(window.tauriOperations.reads,1);
 ready();send({kind:'rows',offset:0,rows:['one']});send({kind:'rows',offset:1,rows:['two']});
 assert.deepEqual(replies,[['print-7',1],['print-7',2]]);
 finish(['A']);await new Promise(resolve=>setImmediate(resolve));assert.equal(done,false);
 send({kind:'done'});assert.deepEqual(await read,['A']);assert.deepEqual(collected,['one','two']);
 const interrupted=window.desktopAPI.referenceChangesPrint(request,()=>{});
 fail(Error('read failed'));await assert.rejects(interrupted,/read failed/);
 const invalid=window.desktopAPI.referenceChangesPrint(request,()=>assert.fail('bad offsets must not render'));
 ready();send({kind:'rows',offset:17,rows:['duplicate']});finish(['A']);
 await assert.rejects(invalid,/порядок строк/);assert.deepEqual(replies.at(-1),['print-7',null]);
 for(const abortBeforeReady of [true,false]){
  const cancel=new AbortController(),stale=window.desktopAPI.referenceChangesPrint(request,()=>assert.fail('cancelled rows must not render'),cancel.signal);
  if(abortBeforeReady)cancel.abort();ready();if(!abortBeforeReady)cancel.abort();
  send({kind:'rows',offset:0,rows:['obsolete']});assert.deepEqual(replies.at(-1),['print-7',null]);
  fail(Error('Печать отменена.'));await assert.rejects(stale,/отменена/);
 }
 assert.equal(window.tauriOperations.reads,0);
});
