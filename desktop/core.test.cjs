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
