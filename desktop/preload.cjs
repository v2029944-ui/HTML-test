'use strict';
const {contextBridge,ipcRenderer}=require('electron');
const call=(channel,...args)=>ipcRenderer.invoke(channel,...args);
contextBridge.exposeInMainWorld('desktopAPI',Object.freeze({
  load:()=>call('desktop:load'),
  save:data=>call('desktop:save',data),
  info:()=>call('desktop:info'),
  chooseFolder:()=>call('desktop:chooseFolder'),
  list:()=>call('desktop:list'),
  setRule:(name,rule)=>call('desktop:rule',name,rule),
  readExcel:name=>call('desktop:readExcel',name),
  checkExcel:(name,signature)=>call('desktop:checkExcel',name,signature),
  openDataFolder:()=>call('desktop:openDataFolder'),
  onFilesChanged:handler=>{
    const listener=()=>handler();
    ipcRenderer.on('desktop:filesChanged',listener);
    return ()=>ipcRenderer.removeListener('desktop:filesChanged',listener);
  }
}));
