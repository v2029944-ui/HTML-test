// Run original management calculations in the actual standalone application.
const fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const html=fs.readFileSync(path.join(__dirname,'../index.html'),'utf8');
const scripts=[...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)];
const context=vm.createContext({console,window:{matchMedia:()=>({matches:false})},document:{}});
vm.runInContext(scripts[1][1].replace(/\n(?:window\.appReady=)?main\(\);\s*$/,'\n'),context);
context.input=JSON.parse(fs.readFileSync(0,'utf8'));
process.stdout.write(JSON.stringify(vm.runInContext(`(()=>{state.reports=input.reports;state.settings=input.settings;return {coverage:groupingCoverage(),distinct:distinctWorks().sort((a,b)=>a<b?-1:a>b?1:0),groups:state.settings.groups.map(g=>({id:g.id,name:g.name,members:g.works.length,unique:new Set(g.works).size})),reports:state.reports.map(r=>({id:r.id,rows:r.rows.length,savedAmount:r.amount,warnings:r.warnings?.length||0,organizations:r.organizations}))};})()`,context)));
