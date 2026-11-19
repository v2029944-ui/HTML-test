const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
function setup(){
 const nodes=new Map(),calls=[];
 const node=()=>({children:[],value:'',selectedOptions:[{textContent:'Тест'}],events:{},append(x){this.children.push(x);},replaceChildren(){this.children=[];},addEventListener(k,f){this.events[k]=f;}});
 const $=id=>{if(!nodes.has(id))nodes.set(id,node());return nodes.get(id);};
 const api={nativeChangesPage(request,after){calls.push({request,after});return new Promise(resolve=>calls.at(-1).resolve=resolve);}};
 const w={desktopAPI:api,analizUserError:String};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-advanced-analysis.js'),'utf8'),{window:w,document:{getElementById:$,createElement:node}});
 for(const [k,v] of Object.entries({Mode:'changes',Metric:'amount',Method:'range',Percent:'30',Count:'1',Amount:'0'}))$('advanced'+k).value=v;
 const scope={displayType:'month',source:'daily',from:'2024-01-01',to:'2024-02-29',organizations:['А'],selectedNames:['<script>']};
 w.nativeAdvancedAnalysis.setContext(3,scope);
 return {w,$,calls,scope};
}
const result={summary:{rows:51,missing_pairs:2,import_warning_pairs:0,invalid_reports:0,duplicate_issues:0,overlap_issues:0},rows:[],next:{key:'work:Z',org:'А',current_start:'2024-02-01'}};
test('advanced pages capture grouped source, filter, revision, keyset and invalidate late responses',async()=>{
 const {w,$,calls}=setup();
 const first=$('advancedRun').events.click();
 assert.equal(calls[0].request.displayType,'month');assert.equal(calls[0].request.source,'daily');
 assert.equal(calls[0].request.revision,3);assert.equal(calls[0].request.selectedNames[0],'<script>');
 calls[0].resolve(result);await first;
 assert.match($('advancedStatus').textContent,/период: 2/);assert.match($('advancedContext').textContent,/дневные отчёты/);
 $('advancedNext').events.click();assert.equal(calls[1].after,result.next);
 w.nativeAdvancedAnalysis.clear();calls[1].resolve(result);await Promise.resolve();await Promise.resolve();
 assert.equal($('advancedRows').children.length,0);assert.equal($('advancedContext').textContent,'');assert.equal($('advancedRun').disabled,true);
});
