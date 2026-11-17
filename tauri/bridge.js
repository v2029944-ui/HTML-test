(function(){
  'use strict';
  const tauri=window.__TAURI__;
  if(!tauri?.core?.invoke || !tauri?.dialog?.open) {
    throw Error('Tauri API не инициализирован. Откройте приложение через Tauri, а не как файл HTML.');
  }
  const invoke=(command,args)=>tauri.core.invoke(command,args||{});
  const operations=window.tauriOperations={saves:0,reads:0,updating:false};
  async function tracked(command,args,kind){
    if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
    operations[kind]++;
    try{return await invoke(command,args);}finally{operations[kind]--;}
  }
  window.desktopAPI=Object.freeze({
    load:()=>invoke('load_data'),
    save:data=>tracked('save_data',{data},'saves'),
    info:()=>invoke('get_info'),
    chooseFolder:async()=>{
      const folder=await tauri.dialog.open({directory:true,multiple:false,title:'Папка с Excel-отчётами'});
      return folder?tracked('set_folder',{folder},'saves'):null;
    },
    list:()=>invoke('list_excel'),
    setRule:(name,rule)=>tracked('set_rule',{name,rule},'saves'),
    readExcel:name=>tracked('read_excel',{name},'reads'),
    checkExcel:(name,signature)=>invoke('check_excel',{name,signature}),
    openDataFolder:()=>invoke('open_data_folder'),
    // The webview checks the folder while the program is open. It rescans at startup.
    onFilesChanged:handler=>{
      let previous=null,hadError=false,active=true,busy=false;
      const poll=async()=>{
        if(!active||busy)return;
        busy=true;
        try{
          const result=await invoke('list_excel');
          const fingerprint=JSON.stringify([
            result.folder,
            result.files.map(file=>[file.name,file.signature])
          ]);
          const changed=previous!==null && fingerprint!==previous;
          const recovered=hadError;
          previous=fingerprint;
          hadError=false;
          if(changed||recovered)handler();
        }catch(err){
          hadError=true;
          // The renderer reports errors during its regular scan.
        }finally{busy=false;}
      };
      poll();
      const timer=setInterval(poll,3000);
      return ()=>{active=false;clearInterval(timer);};
    }
  });
})();

