'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const {prepareHtml}=require('../prepare.cjs');

test('generated frontend preserves original HTML and inserts bridge before inline startup',()=>{
  const html='<!doctype html><html><head></head><body><script>window.appReady=main();</script></body></html>';
  const out=prepareHtml(html);
  assert.equal(html.includes('tauri-bridge'),false);
  assert.ok(out.indexOf('tauri-bridge.js')<out.indexOf('window.appReady=main();'));
  assert.ok(out.indexOf('tauri-folder-importer.js')>out.indexOf('window.appReady=main();'));
  assert.throws(()=>prepareHtml(out),/уже присутствуют/);
});

test('bridge passes load, save and configured Excel directory operations to native commands',async()=>{
  const calls=[],window={__TAURI__:{
    core:{invoke:async(name,args)=>{calls.push([name,args]);return name==='list_excel'?{folder:'C:/Reports',files:[]}:name==='get_info'?{dataFile:'C:/Db/reports.sqlite3'}:{ok:true};}},
    dialog:{open:async()=> 'C:/Reports'}
  }};
  const context={window,setInterval:()=>11,clearInterval:()=>{}};
  const text=fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8');
  vm.runInNewContext(text,context);
  assert.equal(typeof window.desktopAPI.load,'function');
  await window.desktopAPI.load();
  await window.desktopAPI.save({reports:[],settings:{}});
  await window.desktopAPI.chooseFolder();
  await window.desktopAPI.setRule('Анализ.xlsx',{type:'week',start:'2026-10-19'});
  const methods=calls.map(x=>x[0]);
  assert.deepEqual(methods,['load_data','save_data','set_folder','set_rule']);
});

test('native command bridge is absent from the original browser-only HTML',()=>{
  const root=path.resolve(__dirname,'../..');
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');
  assert.equal(html.includes('tauri-bridge.js'),false);
  assert.ok(html.includes('indexedDB.open(DB_NAME,DB_VERSION)'));
});

test('Tauri build copies the current HTML-test interface and current Excel importer without divergence',()=>{
  const os=require('node:os');
  const root=path.resolve(__dirname,'../..');
  const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'tauri-parity-'));
  try{
    const {prepare}=require('../prepare.cjs');
    const prepared=prepare({base:root,output:tmp});
    const source=fs.readFileSync(path.join(root,'index.html'),'utf8');
    const folderImporter=fs.readFileSync(path.join(root,'tauri/folder-importer.js'),'utf8');
    const built=fs.readFileSync(path.join(tmp,'index.html'),'utf8');
    const adapted=fs.readFileSync(path.join(tmp,'tauri-folder-importer.js'),'utf8');
    assert.equal(built,prepareHtml(source),'Tauri must use the complete latest HTML');
    assert.equal(prepared.sourceBytes,Buffer.byteLength(source,'utf8'));
    assert.equal(adapted,folderImporter.replace(
      'Резервная копия: reports.json.bak. Данные не находятся в браузере.',
      'Используется SQLite. Исходный reports.json сохраняется после переноса; делайте отдельные резервные копии.'
    ),'Tauri must use the complete latest Excel folder importer');
    assert.ok(fs.existsSync(path.join(tmp,'tauri-bridge.js')));
  }finally{fs.rmSync(tmp,{recursive:true,force:true});}
});

test('all HTML-test reporting period types are supported by native Tauri SQLite',()=>{
  const root=path.resolve(__dirname,'../..');
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');
  const rust=fs.readFileSync(path.join(root,'tauri/src-tauri/src/storage.rs'),'utf8');
  const htmlList=html.match(/const REPORT_TYPES\s*=\s*\[([^\]]+)\]/);
  const rustList=rust.match(/const REPORT_TYPES:\s*\[&str;\s*\d+\]\s*=\s*\[([^\]]+)\]/);
  assert.ok(htmlList,'Cannot locate the HTML reporting types');
  assert.ok(rustList,'Cannot locate the native reporting types');
  const values=m=>[...m[1].matchAll(/['"]([^'"]+)['"]/g)].map(x=>x[1]).sort();
  assert.deepEqual(values(rustList),values(htmlList),'SQLite is missing a type supported by HTML');
  assert.ok(values(htmlList).includes('day'));
  assert.match(rust,/REPORT_TYPES\.contains\(&rule\.kind\.as_str\(\)\)/);
  assert.match(rust,/REPORT_TYPES\.contains\(&x\)/);
});

test('watched-folder importer suggests correct dates for daily, weekly and monthly Excel',()=>{
  const root=path.resolve(__dirname,'../..');
  const renderer=fs.readFileSync(path.join(root,'tauri/folder-importer.js'),'utf8');
  for(const source of [renderer.replace(/\r\n/g,'\n'),renderer.replace(/\r?\n/g,'\r\n')]){
    const fn=source.match(/function suggestedRule\(name\)\{([\s\S]*?)\r?\n  \}\r?\n  function dateInput/);
    assert.ok(fn,'Cannot find the shared importer date guess');
    const suggest=new Function('name',fn[1]);
    assert.equal(suggest('День-2026-11-16.xlsx').type,'day');
    assert.equal(suggest('Дневной-2026-11-16.xlsx').start,'2026-11-16');
    assert.equal(suggest('week_2026-11-09.xlsx').type,'week');
    assert.equal(suggest('Месяц-2026-11.xlsx').start,'2026-11-01');
  }
  assert.match(renderer,/shiftPeriod\(rule\.type,rule\.start,i\)/);
});


test('Tauri dialog permission and every confirmation guard writes until answered',async()=>{
  const root=path.resolve(__dirname,'../..');
  const html=fs.readFileSync(path.join(root,'index.html'),'utf8');
  const cap=JSON.parse(fs.readFileSync(path.join(root,'tauri/src-tauri/capabilities/default.json'),'utf8'));
  assert.ok(cap.permissions.includes('dialog:allow-message'));
  const confirmationCount=(html.match(/\bconfirm\(/g)||[]).length;
  assert.ok(confirmationCount>=7,'Existing destructive confirmations must remain');
  assert.equal((html.match(/!await confirm\(/g)||[]).length,confirmationCount,'No confirmation may bypass await');

  const compileFunction=(name,context)=>{
    const start=html.indexOf('async function '+name+'(');
    assert.ok(start>=0,'Missing function '+name);
    const end=html.indexOf('\n}',start);
    assert.ok(end>start,'Missing function end '+name);
    return vm.runInNewContext('('+html.slice(start,end+2)+')',context);
  };
  const handlers=[...html.matchAll(/del\.addEventListener\('click',async\(\)=>\{([\s\S]*?)\}\);/g)];
  assert.equal(handlers.length,2,'Expected report and group delete handlers');
  const compileHandler=(body,context)=>vm.runInNewContext('(async()=>{'+body+'})',context);
  let writes=0;
  const confirm=async()=>false,noop=()=>{};

  const report={id:'old',type:'day',start:'2026-11-16',end:'2026-11-16'};
  const reportContext={confirm,displayPeriod:()=>report.start,r:report,state:{reports:[report]},
    storeReports:async()=>{writes++;},renderAll:noop,notice:noop};
  await compileHandler(handlers[0][1],reportContext)();
  assert.equal(writes,0,'Cancelling report deletion must not write');

  const group={id:'old-group',name:'Старая группа',works:['Работа']};
  await compileHandler(handlers[1][1],{
    confirm,g:group,state:{settings:{groups:[group]}},
    saveSettings:async()=>{writes++;},remapCategorySelection:noop,
    resetGroup:noop,renderAll:noop,notice:noop
  })();
  assert.equal(writes,0,'Cancelling group deletion must not write');

  await compileFunction('importGroupFile',{
    confirm,readJsonFile:async()=>({format:'work-groups',version:1,groups:[{id:'new',name:'Новая',works:['Работа']}]}),
    checkedGroups:x=>x,state:{settings:{groups:[]}},
    saveSettings:async()=>{writes++;},resetGroup:noop,renderAll:noop,notice:noop
  })({name:'groups.json'});
  assert.equal(writes,0,'Cancelling group replacement must not write');

  await compileFunction('importBackupFile',{
    confirm,readJsonFile:async()=>({format:'work-reports-backup',version:1,reports:[],
      settings:{orgNames:['A','B','C','D'],groups:[],catalog:[]}}),
    checkedGroups:x=>x,REPORT_TYPES:['day','week','month'],state:{},
    storeAllData:async()=>{writes++;},resetGroup:noop,renderAll:noop,notice:noop
  })({name:'backup.json'});
  assert.equal(writes,0,'Cancelling backup restoration must not write');

  const clear=html.match(/\$\('clearReports'\)\.addEventListener\('click',async\(\)=>\{([\s\S]*?)\}\);/);
  assert.ok(clear,'Missing all-reports delete handler');
  const clearContext={confirm,window:{desktopAPI:{}},state:{},storeReports:async()=>{writes++;},
    renderAll:noop,notice:noop};
  await compileHandler(clear[1],clearContext)();
  assert.equal(writes,0,'Cancelling delete-all must not write');

  const pending={id:'next',type:'day',start:'2026-11-16',end:'2026-11-16',rows:[{work:'Работа',organization:'СМУ16',count:1,amount:10}],warnings:['incomplete']};
  const importContext={
    withExcelImport:action=>action(),confirm,inspectImport:async()=>{},state:{pending:[pending],reports:[],settings:{catalog:[]},
      catalogPreview:[]},$:()=>({checked:false}),
    displayPeriod:x=>x.start,storeReports:async()=>{writes++;},
    storeAllData:async()=>{writes++;},catalogWithEntries:()=>[],renderAll:noop,notice:noop,
    window:{desktopAPI:false}
  };
  const importReport=compileFunction('saveImport',importContext);
  await importReport();
  assert.equal(writes,0,'Cancelling import with warnings must not write');
  pending.warnings=[];
  importContext.state.reports=[{type:'month',start:'2026-11-01',end:'2026-11-30'}];
  await importReport();
  assert.equal(writes,0,'Cancelling overlapping import must not write');

  clearContext.confirm=async()=>true;
  await compileHandler(clear[1],clearContext)();
  assert.equal(writes,1,'Explicit approval still permits deletion');
});

test('folder watcher syncs on recovery even without a file change',async()=>{
  let available=false,files=[{name:'День.xlsx',signature:'1'}],poll,changes=0;
  const window={__TAURI__:{
    core:{invoke:async name=>{
      if(name==='list_excel'){
        if(!available)throw Error('Папка недоступна');
        return {folder:'C:/Reports',files};
      }
      return {ok:true};
    }},
    dialog:{open:async()=>null}
  }};
  const context={window,setInterval:fn=>{poll=fn;return 1;},clearInterval:()=>{}};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../bridge.js'),'utf8'),context);
  const unsubscribe=window.desktopAPI.onFilesChanged(()=>changes++);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(changes,0);
  available=true;
  await poll();
  assert.equal(changes,1,'Recovery after failed initial scan must sync');
  await poll();
  assert.equal(changes,1,'Unchanged scans must not re-sync');
  files=[{name:'День.xlsx',signature:'2'}];
  await poll();
  assert.equal(changes,2,'File changes must still trigger sync');
  available=false;
  await poll();
  available=true;
  await poll();
  assert.equal(changes,3,'Recovery after valid scan must sync');
  unsubscribe();
});


test('WebView2 DevTools is enabled only in the non-distributable GUI test configuration',()=>{
  const root=path.resolve(__dirname,'../..');
  const normal=JSON.parse(fs.readFileSync(path.join(root,'tauri/src-tauri/tauri.conf.json'),'utf8'));
  const gui=JSON.parse(fs.readFileSync(path.join(root,'tauri/tests/ci-webview2.json'),'utf8'));
  assert.ok(!JSON.stringify(normal).includes('remote-debugging-port'),'signed installer must not expose DevTools');
  assert.equal(gui.app.windows.length,1);
  assert.match(gui.app.windows[0].additionalBrowserArgs,/--remote-debugging-port=9227/);
  for(const key of ['title','width','height','minWidth','minHeight'])
    assert.equal(gui.app.windows[0][key],normal.app.windows[0][key]);
  const workflow=fs.readFileSync(path.join(root,'.github/workflows/tauri-windows.yml'),'utf8');
  assert.match(workflow,/Preserve signed preview before building test-only WebView2 bundle/);
  assert.match(workflow,/tauri build --config tests\/ci-webview2\.json/);
  assert.match(workflow,/if: github\.ref != 'refs\/heads\/work\/tauri-dropdown-updates-20261117'/);
  assert.match(workflow,/cargo test --locked --manifest-path tauri\/import-core\/Cargo\.toml/);
  const unsigned=workflow.indexOf('Preserve unsigned CI installer before GUI test build');
  const gui=workflow.indexOf('Build non-distributable GUI test installer with WebView2 CDP');
  assert.ok(unsigned>=0&&unsigned<gui,'Original installer must be preserved before GUI build');
  assert.match(workflow,/ANALIZ_SMOKE_UNSIGNED_CI/);
  assert.match(workflow,/path: tauri\/test-artifacts\/setup\.exe/);
});
