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
  if(!validDate(rule.start)||(rule.type==='month'&&!rule.start.endsWith('-01')))
    throw Error('Укажите корректную дату дня, начала недели или первого дня месяца.');
  const sheet=typeof rule.sheet==='string'?rule.sheet.trim():'';
  if(sheet.length>120)throw Error('Название листа Excel слишком длинное.');
  return {type:rule.type,start:rule.start,sheet};
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

