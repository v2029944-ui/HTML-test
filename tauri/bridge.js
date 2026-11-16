(function(){
  'use strict';
  const tauri=window.__TAURI__;
  if(!tauri?.core?.invoke || !tauri?.dialog?.open) {
    throw Error('Tauri API не инициализирован. Откройте приложение через Tauri, а не как файл HTML.');
  }
  const invoke=(command,args)=>tauri.core.invoke(command,args||{});
  window.desktopAPI=Object.freeze({
    load:()=>invoke('load_data'),
    save:data=>invoke('save_data',{data}),
    info:()=>invoke('get_info'),
    chooseFolder:async()=>{
      const folder=await tauri.dialog.open({directory:true,multiple:false,title:'Папка с Excel-отчётами'});
      return folder?invoke('set_folder',{folder}):null;
    },
    list:()=>invoke('list_excel'),
    setRule:(name,rule)=>invoke('set_rule',{name,rule}),
    readExcel:name=>invoke('read_excel',{name}),
    checkExcel:(name,signature)=>invoke('check_excel',{name,signature}),
    openDataFolder:()=>invoke('open_data_folder'),
    // The webview checks the folder while the program is open. It rescans at startup.
    onFilesChanged:handler=>{
      let previous=null,active=true,busy=false;
      const poll=async()=>{
        if(!active||busy)return;
        busy=true;
        try{
          const result=await invoke('list_excel');
          const fingerprint=JSON.stringify([
            result.folder,
            result.files.map(file=>[file.name,file.signature])
          ]);
          if(previous!==null && fingerprint!==previous)handler();
          previous=fingerprint;
        }catch(err){
          // The renderer reports errors during its regular scan.
        }finally{busy=false;}
      };
      poll();
      const timer=setInterval(poll,3000);
      return ()=>{active=false;clearInterval(timer);};
    }
  });
})();
