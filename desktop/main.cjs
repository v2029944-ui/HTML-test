'use strict';
const fs=require('node:fs');
const path=require('node:path');
const {app,BrowserWindow,dialog,ipcMain,shell}=require('electron');
const {DiskStore}=require('./core.cjs');

let window=null,store=null,lastFolderSnapshot=null;
if(!app.requestSingleInstanceLock())app.quit();
else app.on('second-instance',()=>{if(window){if(window.isMinimized())window.restore();window.focus();}});

function registerHandlers(){
  ipcMain.handle('desktop:load',()=>store.loadData());
  ipcMain.handle('desktop:save',(_event,data)=>store.saveData(data));
  ipcMain.handle('desktop:info',()=>store.getInfo());
  ipcMain.handle('desktop:chooseFolder',async()=>{
    const result=await dialog.showOpenDialog(window,{title:'Папка с Excel-отчётами',properties:['openDirectory']});
    if(result.canceled||!result.filePaths.length)return null;
    const info=store.setFolder(result.filePaths[0]);
    lastFolderSnapshot=null;
    return info;
  });
  ipcMain.handle('desktop:list',()=>store.listExcel());
  ipcMain.handle('desktop:rule',(_event,name,rule)=>store.setRule(name,rule));
  ipcMain.handle('desktop:readExcel',(_event,name)=>{
    const file=store.readExcel(name);
    return {...file,bytes:new Uint8Array(file.bytes)};
  });
  ipcMain.handle('desktop:checkExcel',(_event,name,signature)=>store.checkFile(name,signature));
  ipcMain.handle('desktop:openDataFolder',()=>{
    return shell.openPath(store.root);
  });
}
function snapshot(){
  try{
    const listing=store.listExcel();
    return JSON.stringify([listing.folder,listing.files.map(f=>[f.name,f.signature])]);
  }catch(err){return 'error:'+err.message;}
}
function startPolling(){
  lastFolderSnapshot=snapshot();
  const timer=setInterval(()=>{
    if(!window||window.isDestroyed())return;
    const next=snapshot();
    if(next!==lastFolderSnapshot){
      lastFolderSnapshot=next;
      window.webContents.send('desktop:filesChanged');
    }
  },3000);
  timer.unref?.();
}
async function createWindow(){
  window=new BrowserWindow({
    width:1320,height:900,minWidth:720,minHeight:530,
    title:'Анализ выполненных работ',
    backgroundColor:'#f7f8fa',
    webPreferences:{
      nodeIntegration:false,contextIsolation:true,sandbox:true,
      webSecurity:true,preload:path.join(__dirname,'preload.cjs')
    }
  });
  window.webContents.setWindowOpenHandler(()=>({action:'deny'}));
  window.webContents.on('will-navigate',(event,url)=>{
    if(url!==window.webContents.getURL())event.preventDefault();
  });
  window.webContents.on('did-finish-load',async()=>{
    try{
      const script=fs.readFileSync(path.join(__dirname,'renderer.js'),'utf8');
      await window.webContents.executeJavaScript(script);
    }catch(err){
      dialog.showErrorBox('Не удалось запустить настольные функции',err.message);
    }
  });
  await window.loadFile(path.join(__dirname,'..','index.html'));
  startPolling();
}
app.whenReady().then(async()=>{
  store=new DiskStore(path.join(app.getPath('documents'),'Анализ работ'));
  registerHandlers();
  await createWindow();
}).catch(err=>dialog.showErrorBox('Ошибка запуска',err.message));
app.on('window-all-closed',()=>{if(process.platform!=='darwin')app.quit();});
app.on('activate',()=>{if(BrowserWindow.getAllWindows().length===0)createWindow();});
