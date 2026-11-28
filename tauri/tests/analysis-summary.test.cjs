const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('simultaneous totals share SQL only for the identical workspace and complete filter identity', async () => {
 const window = {sharedWorkspace:{state:{workspace_token:'first'}}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'), {window});
 const scope = {revision:3,source:'daily',displayType:'month',type:'day',from:'2024-02-01',to:'2024-02-29',
  baseFrom:'2024-01-01',baseTo:'2024-01-31',organizations:['A'],allNames:false,selectedNames:['Работа'],selectedKeys:['work:a']};
 let comparisons = 0, organizations = 0;
 const api = {nativeAnalysisCompare:async () => ++comparisons,nativeAnalysisOrganizations:async () => ++organizations};
 const read = (s) => window.analysisSummary.read(api,s);
 const first = read(scope);
 assert.equal(first,read({...scope,organizations:['A'],selectedKeys:['work:a']}));
 assert.deepEqual([comparisons,organizations],[1,1]);
 for (const patch of [{revision:4},{source:'imported'},{displayType:'week'},{type:'month'},{from:'2024-02-02'},
  {to:'2024-02-28'},{baseFrom:'2024-01-02'},{baseTo:'2024-01-30'},{organizations:['B']},
  {allNames:true},{selectedNames:['Другая']},{selectedKeys:['group:a']}]) {
  const normal = read(scope), changed = read({...scope,...patch});
  assert.notEqual(normal,changed);
 }
 const current = read(scope);
 window.sharedWorkspace.state.workspace_token = 'second';
 assert.notEqual(read(scope),current);
 await first;
 const failure = Error('ошибка чтения');
 api.nativeAnalysisCompare = async () => { throw failure; };
 await assert.rejects(read(scope),failure);
 api.nativeAnalysisCompare = async () => ++comparisons;
 await read(scope);
});

test('overview totals explicitly keep all works while the separate detail work selection changes',async()=>{
 const window={};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../analysis-summary.js'),'utf8'),{window});
 const calls=[],api={nativeAnalysisCompare:async(...args)=>{calls.push(['compare',args]);return {};},nativeAnalysisOrganizations:async(...args)=>{calls.push(['organizations',args]);return [];}};
 const scope={revision:1,type:'day',source:'daily',displayType:'day',from:'2024-01-10',to:'2024-01-11',baseFrom:'2024-01-01',baseTo:'2024-01-02',organizations:['A','B','C','D'],allNames:false,selectedNames:['Группа'],selectedKeys:['group:g']};
 await window.analysisSummary.read(api,scope);
 assert.deepEqual(calls[0][1].slice(7),[null,null,false]);assert.deepEqual(calls[1][1].slice(5),[null,null]);
 const shell=window.analysisSummary.read(api,scope,true),detail=window.analysisSummary.read(api,scope);
 assert.equal(shell,detail);await shell;
 assert.equal(calls[2][1].at(-1),true);
 assert.deepEqual(scope.selectedNames,['Группа']);
});

test('selection displays unique work counts and keeps only the highest selected rows', () => {
 const window={};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,"../analysis-summary.js"),"utf8"),{window});
 const {topLevelWorkRows,workSelectionText}=window.analysisSummary;
 const rows=[
  {key:'group:r',kind:'group'},
  {key:'group:c',kind:'group',ancestor_keys:['group:r']},
  {key:'work:w',kind:'work',ancestor_keys:['group:r','group:c']},
  {key:'work:u',kind:'work'}
 ];
 assert.deepEqual(Array.from(topLevelWorkRows(rows),row=>row.key),['group:r','work:u']);
 assert.deepEqual(Array.from(topLevelWorkRows(rows.slice(1)),row=>row.key),['group:c','work:u']);
 assert.equal(workSelectionText({selected:5,total:10}),'Выбрано 5 из 10 работ');
});
