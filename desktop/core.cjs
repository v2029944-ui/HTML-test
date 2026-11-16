'use strict';
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');

const MAX_EXCEL_SIZE=30*1024*1024;
const MAX_DATA_SIZE=100*1024*1024;
const INITIAL_SETTINGS={orgNames:['СМУ16','АВТО','ТРАК','ЦЕНТР'],groups:[],catalog:[]};
const DEFAULT_DATA=()=>({format:'work-reports-desktop',version:1,reports:[],settings:structuredClone(INITIAL_SETTINGS)});
const DEFAULT_CONFIG=()=>({version:1,folder:null,rules:{}});

function safeExcelName(name){
  return typeof name==='string'&&name.length<=240&&/\.xlsx$/i.test(name)&&
    !name.startsWith('~$')&&!/[\/\\:\u0000-\u001f]/.test(name)&&
    name!=='.'&&name!=='..'&&path.basename(name)===name;
}
function validDate(value){
  if(typeof value!=='string'||!/^\d{4}-\d{2}-\d{2}$/.test(value))return false;
  const date=new Date(value+'T12:00:00Z');
  return Number.isFinite(date.getTime())&&date.toISOString().slice(0,10)===value;
}
function normalizeRule(rule){
  if(!rule||!['week','month','day'].includes(rule.type))throw Error('Укажите тип отчётов: дни, недели или месяцы.');
  const has=key=>Object.prototype.hasOwnProperty.call(rule,key);
  const wide=rule.format==='daily-wide';
  if((has('format')&&!wide)||(wide&&rule.type!=='day'))throw Error('Некорректный формат импорта.');
  if(!(wide&&rule.start==='')&&(!validDate(rule.start)||(rule.type==='month'&&!rule.start.endsWith('-01'))))
    throw Error('Укажите корректную дату дня, начала недели или первого дня месяца.');
  const sheet=typeof rule.sheet==='string'?rule.sheet.trim():'';
  if(sheet.length>120)throw Error('Название листа Excel слишком длинное.');
  const result={type:rule.type,start:rule.start,sheet};
  if(wide)result.format='daily-wide';
  const invalid=()=>{throw Error('Некорректные параметры годового импорта.');};
  for(const key of ['year','sheets','mappings'])if(has(key)&&!wide)invalid();
  if(has('year')){
    if(!Number.isInteger(rule.year)||rule.year<1900||rule.year>9999)invalid();
    result.year=rule.year;
  }
  const sheetName=name=>typeof name==='string'&&name.trim().length>0&&name.length<=120;
  if(has('sheets')){
    if(!Array.isArray(rule.sheets)||rule.sheets.length>12||!rule.sheets.every(sheetName))invalid();
    result.sheets=[...rule.sheets];
  }
  if(has('mappings')){
    if(!rule.mappings||typeof rule.mappings!=='object'||Array.isArray(rule.mappings))invalid();
    result.mappings={};
    for(const [name,mapping] of Object.entries(rule.mappings)){
      if(!sheetName(name)||!mapping||typeof mapping!=='object'||Array.isArray(mapping))invalid();
      const fields=['workCol','headerRow','firstDayCol','totalCountCol','totalAmountCol'];
      if(Object.keys(mapping).some(key=>!fields.includes(key)))invalid();
      const copy={};
      for(const key of fields){
        const optional=key==='totalCountCol'||key==='totalAmountCol';
        if(optional&&!Object.prototype.hasOwnProperty.call(mapping,key))continue;
        const value=mapping[key];
        if(!(optional&&value===null)&&(!Number.isInteger(value)||value<0||value>(key==='headerRow'?59999:2047)))invalid();
        copy[key]=value;
      }
      Object.defineProperty(result.mappings,name,{value:copy,enumerable:true,writable:true,configurable:true});
    }
  }
  return result;
}
function validateData(data){
  if(!data||!Array.isArray(data.reports)||!data.settings||!Array.isArray(data.settings.orgNames)||
    data.settings.orgNames.length!==4||!Array.isArray(data.settings.groups)||!Array.isArray(data.settings.catalog))
    throw Error('Некорректные данные для сохранения.');
  for(const r of data.reports){
    if(!r||typeof r.id!=='string'||!['week','month','day'].includes(r.type)||
      !validDate(r.start)||!validDate(r.end)||!Array.isArray(r.rows)||
      !r.rows.every(x=>x&&typeof x.work==='string'&&typeof x.organization==='string'&&
        Number.isFinite(x.count)&&Number.isFinite(x.amount)))
      throw Error('Сохранение отменено: некорректная строка отчёта.');
  }
  const serialized=JSON.stringify({format:'work-reports-desktop',version:1,
    reports:data.reports,settings:data.settings});
  if(Buffer.byteLength(serialized,'utf8')>MAX_DATA_SIZE)throw Error('Файл данных превышает 100 МБ.');
  return serialized;
}
function atomicWrite(filename,content){
  fs.mkdirSync(path.dirname(filename),{recursive:true});
  const temp=filename+'.'+process.pid+'.'+crypto.randomBytes(5).toString('hex')+'.tmp';
  try{
    fs.writeFileSync(temp,content,{encoding:'utf8',flag:'wx',mode:0o600});
    // Backup a previously valid snapshot; the old file stays intact until rename succeeds.
    if(fs.existsSync(filename))fs.copyFileSync(filename,filename+'.bak');
    fs.renameSync(temp,filename);
  }finally{
    try{fs.unlinkSync(temp);}catch{}
  }
}
function fileSignature(stat){
  return [stat.size,Math.trunc(stat.mtimeMs),Math.trunc(stat.ctimeMs)].join(':');
}
class DiskStore {
  constructor(root){
    if(typeof root!=='string'||!root.trim())throw Error('Не задана папка хранения.');
    this.root=root;
    this.dataFile=path.join(root,'reports.json');
    this.configFile=path.join(root,'desktop-config.json');
    fs.mkdirSync(root,{recursive:true});
  }
  loadData(){
    if(!fs.existsSync(this.dataFile))return DEFAULT_DATA();
    const data=JSON.parse(fs.readFileSync(this.dataFile,'utf8'));
    validateData(data);
    return data;
  }
  saveData(data){
    const text=validateData(data);
    atomicWrite(this.dataFile,text);
    return {ok:true,dataFile:this.dataFile};
  }
  loadConfig(){
    if(!fs.existsSync(this.configFile))return DEFAULT_CONFIG();
    const data=JSON.parse(fs.readFileSync(this.configFile,'utf8'));
    if(data?.version!==1||typeof data.rules!=='object'||!data.rules||
      !(data.folder===null||typeof data.folder==='string'))
      throw Error('Некорректная конфигурация папки. Проверьте desktop-config.json.');
    return data;
  }
  getInfo(){
    const config=this.loadConfig();
    return {folder:config.folder,rules:config.rules,dataFile:this.dataFile,configFile:this.configFile};
  }
  setFolder(folder){
    if(typeof folder!=='string')throw Error('Неверная папка.');
    const resolved=fs.realpathSync(folder);
    if(!fs.statSync(resolved).isDirectory())throw Error('Выбранный путь не является папкой.');
    const config=this.loadConfig(),previous=config.folder;
    config.folder=resolved;
    if(previous!==resolved)config.rules={};
    atomicWrite(this.configFile,JSON.stringify(config,null,2));
    return this.getInfo();
  }
  setRule(name,rule){
    if(!safeExcelName(name))throw Error('Некорректное имя файла Excel.');
    const config=this.loadConfig();
    if(!config.folder)throw Error('Сначала выберите папку с Excel.');
    config.rules[name]=normalizeRule(rule);
    atomicWrite(this.configFile,JSON.stringify(config,null,2));
    return config.rules[name];
  }
  listExcel(){
    const config=this.loadConfig();
    if(!config.folder)return {folder:null,files:[],rules:config.rules};
    const dir=fs.realpathSync(config.folder);
    if(!fs.statSync(dir).isDirectory())throw Error('Папка отчётов недоступна.');
    const files=fs.readdirSync(dir,{withFileTypes:true}).filter(entry=>
      entry.isFile()&&safeExcelName(entry.name)).map(entry=>{
      const stat=fs.statSync(path.join(dir,entry.name));
      return {name:entry.name,size:stat.size,signature:fileSignature(stat),
        tooLarge:stat.size>MAX_EXCEL_SIZE};
    }).sort((a,b)=>a.name.localeCompare(b.name,'ru'));
    return {folder:dir,files,rules:config.rules};
  }
  readExcel(name){
    if(!safeExcelName(name))throw Error('Разрешены только XLSX-файлы из выбранной папки.');
    const config=this.loadConfig();
    if(!config.folder)throw Error('Папка не выбрана.');
    const filename=path.join(config.folder,name);
    const statBefore=fs.lstatSync(filename);
    if(!statBefore.isFile())throw Error('Это не обычный файл Excel.');
    if(statBefore.size>MAX_EXCEL_SIZE)throw Error('Excel превышает лимит 30 МБ.');
    const bytes=fs.readFileSync(filename);
    const statAfter=fs.lstatSync(filename);
    if(fileSignature(statBefore)!==fileSignature(statAfter)||bytes.length!==statAfter.size)
      throw Error('Файл изменяется во время чтения. Повторите проверку после окончания копирования.');
    return {bytes,signature:fileSignature(statAfter),
      sha256:crypto.createHash('sha256').update(bytes).digest('hex')};
  }
  checkFile(name,signature){
    if(!safeExcelName(name))return false;
    try{
      const file=path.join(this.loadConfig().folder,name),stat=fs.lstatSync(file);
      return stat.isFile()&&fileSignature(stat)===signature;
    }catch{return false;}
  }
}
module.exports={DiskStore,safeExcelName,normalizeRule,validateData,validDate,fileSignature};


