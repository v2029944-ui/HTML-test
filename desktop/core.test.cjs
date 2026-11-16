'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const {DiskStore,safeExcelName,normalizeRule,validateData}=require('./core.cjs');
const create=()=>fs.mkdtempSync(path.join(os.tmpdir(),'analiz-desktop-'));
const testData=()=>({
  reports:[{
    id:'r-1',type:'week',start:'2026-10-19',end:'2026-10-25',
    rows:[{organization:'СМУ16',work:'Работа',count:1.5,amount:200}]
  }],
  settings:{orgNames:['СМУ16','АВТО','ТРАК','ЦЕНТР'],groups:[],catalog:[]}
});
test('disk storage persists reports and settings outside browser',()=>{
  const root=create();
  try{
    const store=new DiskStore(root);
    assert.equal(store.loadData().reports.length,0);
    const data=testData();
    store.saveData(data);
    const saved=new DiskStore(root).loadData();
    assert.equal(saved.reports[0].rows[0].count,1.5);
    assert.equal(saved.settings.orgNames.length,4);
    assert.ok(fs.existsSync(path.join(root,'reports.json')));
    assert.equal(saved.format,'work-reports-desktop');
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('an update creates a backup of the last committed data',()=>{
  const root=create();
  try{
    const store=new DiskStore(root),data=testData();
    store.saveData(data);
    const next=testData();next.reports[0].rows[0].amount=420;
    store.saveData(next);
    assert.equal(store.loadData().reports[0].rows[0].amount,420);
    const old=JSON.parse(fs.readFileSync(path.join(root,'reports.json.bak'),'utf8'));
    assert.equal(old.reports[0].rows[0].amount,200);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('corrupt reports are rejected before overwriting the valid file',()=>{
  const root=create();
  try{
    const store=new DiskStore(root),data=testData();
    store.saveData(data);
    const corrupt=testData();corrupt.reports[0].rows[0].amount=NaN;
    assert.throws(()=>store.saveData(corrupt),/некорректная строка/);
    assert.equal(store.loadData().reports[0].rows[0].amount,200);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('Excel inputs cannot escape the selected folder or read temporary XLSX files',()=>{
  assert.ok(safeExcelName('2026-10-19.xlsx'));
  assert.ok(safeExcelName('Анализ.xlsx'));
  for(const unsafe of ['../secret.xlsx','..\\secret.xlsx','~$Работа.xlsx',
    'report.xls','image.png','../a.xlsx','report:stream.xlsx','x\u0000.xlsx']){
    assert.equal(safeExcelName(unsafe),false,unsafe);
  }
});
test('folder discovery, rules and signed file reads work',()=>{
  const root=create(),folder=path.join(root,'Отчёты');
  try{
    fs.mkdirSync(folder);fs.writeFileSync(path.join(folder,'Неделя.xlsx'),'demo');
    fs.writeFileSync(path.join(folder,'~$Новая.xlsx'),'tmp');
    fs.writeFileSync(path.join(folder,'notes.txt'),'ignored');
    const store=new DiskStore(path.join(root,'Данные'));
    store.setFolder(folder);
    store.setRule('Неделя.xlsx',{type:'week',start:'2026-10-19'});
    const scan=store.listExcel();
    assert.equal(scan.files.length,1);
    assert.equal(scan.rules['Неделя.xlsx'].start,'2026-10-19');
    const source=store.readExcel('Неделя.xlsx');
    assert.equal(source.bytes.toString(),'demo');
    assert.equal(source.signature,scan.files[0].signature);
    assert.equal(store.checkFile('Неделя.xlsx',source.signature),true);
    fs.writeFileSync(path.join(folder,'Неделя.xlsx'),'changed');
    assert.equal(store.checkFile('Неделя.xlsx',source.signature),false);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('invalid dates and corrupted saved JSON never silently reset the data',()=>{
  assert.throws(()=>normalizeRule({type:'month',start:'2026-02-30'}));
  assert.throws(()=>normalizeRule({type:'month',start:'2026-02-15'}));
  assert.throws(()=>normalizeRule({type:'week',start:'2026-02-30'}));
  const root=create();
  try{
    const store=new DiskStore(root);
    fs.writeFileSync(store.dataFile,'{invalid json');
    assert.throws(()=>store.loadData());
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});


test('daily reports and rules persist alongside weekly and monthly data',()=>{
  const root=create();
  try{
    const store=new DiskStore(root),data=testData();
    data.reports.push({...data.reports[0],id:'daily',type:'day',start:'2026-11-10',end:'2026-11-10'});
    data.reports.push({...data.reports[0],id:'monthly',type:'month',start:'2026-11-01',end:'2026-11-30'});
    assert.deepEqual(normalizeRule({type:'day',start:'2026-11-10'}),{type:'day',start:'2026-11-10',sheet:''});
    assert.throws(()=>normalizeRule({type:'day',start:'2026-02-30'}));
    store.saveData(data);
    const saved=new DiskStore(root).loadData();
    assert.deepEqual(saved.reports.map(r=>r.type),['week','day','month']);
    assert.equal(saved.reports[1].rows[0].count,1.5);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});

const wideRule=()=>({type:'day',start:'',sheet:'',format:'daily-wide',year:2026,
  sheets:['Январь','Февраль'],mappings:{'Январь':{workCol:0,headerRow:3,firstDayCol:1,totalCountCol:null,totalAmountCol:64}}});
test('daily-wide rules round-trip without adding properties to legacy rules',()=>{
  const root=create(),folder=path.join(root,'Excel');
  try{
    fs.mkdirSync(folder);
    const store=new DiskStore(root);store.setFolder(folder);
    const legacy={type:'week',start:'2026-11-16',sheet:''};
    store.setRule('legacy.xlsx',legacy);
    store.setRule('annual.xlsx',wideRule());
    const saved=new DiskStore(root).getInfo().rules;
    assert.deepEqual(saved['legacy.xlsx'],legacy);
    assert.deepEqual(saved['annual.xlsx'],wideRule());
    assert.deepEqual(normalizeRule({type:'day',start:'',sheet:'',format:'daily-wide'}),
      {type:'day',start:'',sheet:'',format:'daily-wide'});
    assert.deepEqual(normalizeRule({...wideRule(),year:9999,mappings:{'Январь':{workCol:2047,headerRow:59999,firstDayCol:2047}}}).year,9999);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('daily-wide options reject malformed types, years, sheets and mappings',()=>{
  const mapping={workCol:0,headerRow:0,firstDayCol:1};
  const invalid=[{format:'annual'},{type:'month'},{start:'2026-02-30'},
    {year:1899},{year:10000},{year:2026.5},{year:'2026'},{year:null},
    {sheets:'Январь'},{sheets:Array(13).fill('Лист')},{sheets:['']},{sheets:[2]},
    {sheets:['a'.repeat(121)]},{sheets:null},{mappings:[]},{mappings:null},
    {mappings:{'':mapping}},{mappings:{Sheet:{...mapping,workCol:-1}}},
    {mappings:{Sheet:{...mapping,firstDayCol:2048}}},
    {mappings:{Sheet:{...mapping,headerRow:60000}}},
    {mappings:{Sheet:{...mapping,totalAmountCol:2.5}}},
    {mappings:{Sheet:{...mapping,totalCountCol:'2'}}},
    {mappings:{Sheet:{headerRow:0,firstDayCol:1}}}];
  for(const extra of invalid)assert.throws(()=>normalizeRule({...wideRule(),...extra}),JSON.stringify(extra));
  assert.throws(()=>normalizeRule({type:'day',start:'2026-11-16',year:2026}));
});
test('invalid rules and failed atomic writes preserve the last saved configuration',()=>{
  const root=create(),folder=path.join(root,'Excel');
  try{
    fs.mkdirSync(folder);const store=new DiskStore(root);store.setFolder(folder);
    store.setRule('annual.xlsx',wideRule());
    const before=fs.readFileSync(store.configFile,'utf8');
    assert.throws(()=>store.setRule('annual.xlsx',{...wideRule(),year:10000}));
    assert.equal(fs.readFileSync(store.configFile,'utf8'),before);
    // A directory at the backup path forces an actual filesystem write failure.
    fs.rmSync(store.configFile+'.bak',{force:true});fs.mkdirSync(store.configFile+'.bak');
    assert.throws(()=>store.setRule('annual.xlsx',{...wideRule(),year:2027}));
    assert.equal(fs.readFileSync(store.configFile,'utf8'),before);
    assert.equal(fs.readdirSync(root).some(name=>name.endsWith('.tmp')),false);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
