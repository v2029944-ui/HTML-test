'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');

test('current Tauri version is synchronized and has concrete versioned release notes',()=>{
 const config=JSON.parse(fs.readFileSync(path.join(__dirname,'../src-tauri/tauri.conf.json'),'utf8'));
 const pkg=JSON.parse(fs.readFileSync(path.join(__dirname,'../package.json'),'utf8'));
 const cargo=fs.readFileSync(path.join(__dirname,'../src-tauri/Cargo.toml'),'utf8');
 const v=cargo.match(/^version\s*=\s*"([^"]+)"/m);
 assert.ok(v,'Cargo package version must be defined');
 assert.equal(config.version,pkg.version);
 assert.equal(config.version,v[1]);
 const filename=path.join(__dirname,'../release-notes',config.version+'.md');
 assert.ok(fs.existsSync(filename),'Every candidate needs version-specific changelog');
 const notes=fs.readFileSync(filename,'utf8');
 assert.match(notes,/^#\s+.+/m);
 assert.match(notes,/^[-*]\s+.+/m);
 assert.match(notes,/## Что добавлено/);
 assert.match(notes,/## Обновление и ограничения/);
 assert.ok(notes.length>400,'No generic placeholder is allowed for a release');
});
