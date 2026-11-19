// Execute the actual standalone application's old analytics functions, not a port.
const fs=require('node:fs'),vm=require('node:vm');
const html=fs.readFileSync(require('node:path').join(__dirname,'../index.html'),'utf8');
const scripts=[...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)];
const context=vm.createContext({console,window:{matchMedia:()=>({matches:false})},document:{}});
vm.runInContext(scripts[1][1].replace(/\n(?:window\.appReady=)?main\(\);\s*$/,'\n'),context);
context.input=JSON.parse(fs.readFileSync(0,'utf8'));
const output=vm.runInContext(`(()=>{
const {reports,settings,scope,ranges,changes}=input;
const index=buildReportIndex(reports,settings.groups,{type:scope.type,organizations:scope.organizations});
const normalize=r=>({complete:r.complete,total:r.total,missing:[...r.orgs].flatMap(([org,x])=>x.missing.map(start=>({org,start}))).sort((a,b)=>a.org<b.org?-1:a.org>b.org?1:a.start<b.start?-1:1),values:[...r.by.values()].map(x=>({key:x.key,name:x.name,kind:x.kind,count:r.complete?x.count:null,amount:r.complete?x.amount:null,average:r.complete&&x.count>0?x.amount/x.count:null,active:x.active===true})).sort((a,b)=>a.key<b.key?-1:1)});
const result={ranges:ranges.map(r=>normalize(aggregateReportRange(index,{...r,organizations:scope.organizations,names:scope.names?new Set(scope.names):null}))),issues:index.issues.map(x=>x.type).sort(),changes:analyzeWorkChanges(reports,settings.groups,{...changes,type:scope.type,organizations:scope.organizations}).rows.filter(x=>!scope.names||scope.names.includes(x.name)).map(x=>({key:x.key,name:x.name,kind:x.kind,org:x.org,baseStart:x.basePeriod.start,currentStart:x.currentPeriod.start,status:x.status,count:x.count,amount:x.amount,average:x.average})).sort((a,b)=>JSON.stringify([a.currentStart,a.org,a.key])<JSON.stringify([b.currentStart,b.org,b.key])?-1:1)};
const rangeHeader=['key','name','kind','count','amount','average','active'];
result.rangeCsv=result.ranges.map(r=>analyticsCsv([rangeHeader,...r.values.map(x=>[x.key,x.name,x.kind,x.count,x.amount,x.average,x.active])]));
const changeHeader=['key','name','kind','org','baseStart','currentStart','status','count.base','count.current','count.delta','count.percent','amount.base','amount.current','amount.delta','amount.percent','average.base','average.current','average.delta','average.percent'];
result.changesCsv=analyticsCsv([changeHeader,...result.changes.map(x=>[x.key,x.name,x.kind,x.org,x.baseStart,x.currentStart,x.status,...['count','amount','average'].flatMap(m=>['base','current','delta','percent'].map(k=>x[m][k]))])]);
if(ranges.length>=2){
 const b=aggregateReportRange(index,{...ranges[0],organizations:scope.organizations,names:scope.names?new Set(scope.names):null});
 const a=aggregateReportRange(index,{...ranges[1],organizations:scope.organizations,names:scope.names?new Set(scope.names):null});
 if(input.advanced){result.structure=analyzeStructure(b,a,"amount",12);result.contributions=analyzeContributions(b,a);}
 result.comparison=(scope.names?comparisonEntries([b,a],new Set(scope.names)).filter(Boolean):[...new Set([...b.by.keys(),...a.by.keys()])].map(key=>b.by.get(key)||a.by.get(key))).sort((a,b)=>a.key<b.key?-1:1).map(desc=>{
 const key=desc.key,bc=rangeMetric(b,null,key,'count'),ba=rangeMetric(b,null,key,'amount'),ac=rangeMetric(a,null,key,'count'),aa=rangeMetric(a,null,key,'amount'),bv=rangeMetric(b,null,key,'average'),av=rangeMetric(a,null,key,'average');
 return {key,name:desc.name,kind:desc.kind,status:!b.complete||!a.complete?null:bc===0&&ba===0&&(ac!==0||aa!==0)?'new':(bc!==0||ba!==0)&&ac===0&&aa===0?'gone':'existing',count:bc===null||ac===null?null:changeMeasure(bc,ac,changes.minCount,changes.percent),amount:ba===null||aa===null?null:changeMeasure(ba,aa,changes.minAmount,changes.percent),average:bv===null||av===null?{base:bv,current:av,delta:null,percent:null}:changeMeasure(bv,av,0,changes.percent)};
 });
}
if(input.advanced){
 const opt={organizations:scope.organizations,names:scope.names?new Set(scope.names):null,groups:settings.groups};
 result.usual=analyzeUsual(index,{...opt,period:changes.to,window:4,percent:changes.percent});
 result.regularity=analyzeRegularity(index,{...opt,from:changes.from,to:changes.to,longGap:4});
}
return result;
})()`,context);
process.stdout.write(JSON.stringify(output));
