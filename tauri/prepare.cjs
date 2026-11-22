'use strict';
const fs=require('node:fs'),path=require('node:path');
const {buildUnified}=require('./unified-build.cjs');
const scripts=['shared-workspace','bridge','native-errors','native-period-selection','app-status','analysis-summary','unified-base','sql-row-scroll','unified-coverage','unified-management','unified-reports','native-metric-kinds','unified-horizontal-bars','unified-extended','unified-analytics','reference-calendar','reference-detail','reference-results','reference-filters','reference-desktop','unified-app','updates'];
const styles=['sql-row-scroll','unified','app-status','reference-shell','reference-detail'];
function prepareHtml(source){return buildUnified(source).html;}
function prepare(options={}){
 const base=options.base||path.resolve(__dirname,'..'),output=options.output||path.join(__dirname,'web');
 const source=fs.readFileSync(path.join(base,'index.html'),'utf8'),unified=buildUnified(source);
 // This is a generated distribution directory. Remove retired application routes
 // and controllers so a previous build cannot leave a second interface reachable.
 fs.mkdirSync(output,{recursive:true});
 const assets=new Map([['index.html',unified.html],['tauri-unified-original.js',unified.script]]);
 for(const name of scripts)assets.set('tauri-'+name+'.js',fs.readFileSync(path.join(__dirname,name+'.js')));
 for(const name of styles)assets.set('tauri-'+name+'.css',fs.readFileSync(path.join(__dirname,name+'.css')));
 // These are loaded after the native folder controls have been created.
 for(const name of ['native-folder-auto-import','native-folder-status'])assets.set(name+'.js',fs.readFileSync(path.join(__dirname,name+'.js')));
 const retired=/^(?:startup\.html|native-workspace\.html|tauri-(?:native-[\w-]+|startup(?:-preference)?|folder-importer|appearance)\.js|tauri-(?:classic|native-skin)\.css)$/;
 for(const entry of fs.readdirSync(output,{withFileTypes:true}))if(entry.isFile()&&retired.test(entry.name)&&!assets.has(entry.name))fs.unlinkSync(path.join(output,entry.name));
 for(const [name,contents]of assets)fs.writeFileSync(path.join(output,name),contents);
 return {sourceBytes:Buffer.byteLength(source,'utf8'),target:output};
}
if(require.main===module){const result=prepare();process.stdout.write('Prepared canonical SQLite interface ('+result.sourceBytes+' bytes)\n');}
module.exports={prepareHtml,prepare};
