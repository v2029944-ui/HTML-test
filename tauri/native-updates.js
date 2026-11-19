// SQL-first Tauri mode must offer the same signed-updater flow as the classic UI.
// Tauri Updater verifies the signature. This script never writes reports or SQLite.
(function nativeSqlUpdates(){
'use strict';
const tauri=window.__TAURI__,ops=window.tauriOperations;
const $=id=>document.getElementById(id);
const check=$('sqlUpdateCheck'),install=$('sqlUpdateInstall'),notes=$('sqlUpdateNotes');
const message=$('sqlUpdateStatus'),dialog=$('sqlUpdateDialog'),body=$('sqlUpdateNotesText'),close=$('sqlUpdateClose');
if(!check||!install||!notes||!message||!dialog||!body||!close)return;
let busy=false,available=null;
const status=(text,error=false)=>{message.textContent=text;message.className='status '+(error?'error':'muted');};
const lock=()=>{check.disabled=busy;install.disabled=busy||!available;notes.disabled=busy||!available;};
function blocked(){
 if(ops?.updating)return 'Установка другого обновления уже выполняется.';
 if(ops?.reads||ops?.saves)return 'Дождитесь завершения чтения или сохранения данных SQLite.';
 return '';
}
async function checkUpdates(){
 if(busy)return;
 const reason=blocked();if(reason){status(reason,true);return;}
 busy=true;lock();
 try{
  if(!tauri?.core?.invoke||!tauri?.updater?.check)throw Error('Tauri Updater недоступен.');
  if(!await tauri.core.invoke('updater_configured')){
    status('Онлайн-обновления недоступны в этой сборке: необходим установщик с ключом проверки подписи.');
    install.hidden=true;notes.hidden=true;
    return;
  }
  if(available&&typeof available.close==='function')await available.close();
  available=null;install.hidden=true;notes.hidden=true;
  status('Проверяем подписанные обновления…');
  available=await tauri.updater.check({timeout:15000});
  if(available){
    status('Доступна версия '+available.version+'. Ознакомьтесь с изменениями перед установкой.');
    install.hidden=false;notes.hidden=false;
  }else status('Установлена актуальная версия.');
 }catch(error){status('Не удалось проверить обновления: '+String(error?.message||error),true);}
 finally{busy=false;lock();}
}
async function installUpdate(){
 if(busy||!available)return;
 const reason=blocked();if(reason){status(reason,true);return;}
 if(!window.confirm('Скачать и установить подписанную версию '+available.version+'? Приложение закроется. Сохраните все изменения.'))return;
 const again=blocked();if(again){status(again,true);return;}
 busy=true;if(ops)ops.updating=true;lock();
 try{
  let received=0,total=0;
  status('Загружается подписанное обновление…');
  await available.downloadAndInstall(event=>{
    if(event.event==='Started')total=event.data?.contentLength||0;
    if(event.event==='Progress'){
      received+=event.data?.chunkLength||0;
      status(total?'Загружено '+Math.min(100,Math.round(received/total*100))+'%':'Загружено '+Math.round(received/1024)+' КБ');
    }
    if(event.event==='Finished')status('Проверка подписи и установка…');
  },{timeout:120000,restartAfterInstall:true});
  status('Установка передана установщику. Если приложение закрылось, завершите установку.');
 }catch(error){status('Обновление не установлено: '+String(error?.message||error),true);}
 finally{if(ops)ops.updating=false;busy=false;lock();}
}
check.addEventListener('click',()=>{void checkUpdates();});
install.addEventListener('click',()=>{void installUpdate();});
notes.addEventListener('click',()=>{
 if(!available)return;
 body.textContent=(typeof available.body==='string'&&available.body.trim())||'Описание изменений отсутствует.';
 if(!dialog.open)dialog.showModal();
});
close.addEventListener('click',()=>dialog.close());
lock();
window.nativeSqlUpdaterReady=checkUpdates();
})();
