(async function onlineUpdates(){
  'use strict';
  await window.appReady;
  const tauri=window.__TAURI__,settings=document.getElementById('storage');
  if(!tauri?.updater||!settings)return;
  const panel=elem('section',null,'panel');panel.id='onlineUpdates';
  panel.append(elem('h2','Обновление программы'));
  panel.append(elem('p','Новая версия скачивается и устанавливается после подтверждения. Отчёты и настройки сохраняются. Программа закроется на время установки.','muted small'));
  const status=elem('p','Проверка доступности обновлений…','muted small');
  status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  const actions=elem('div',null,'actions'),check=elem('button','Проверить обновления','button secondary');
  const install=elem('button','Скачать и установить','button primary');install.hidden=true;
  check.type=install.type='button';check.id='checkAppUpdate';install.id='installAppUpdate';
  status.id='appUpdateStatus';actions.append(check,install);panel.append(status,actions);settings.append(panel);
  let busy=false,update=null;
  const message=(text,error=false)=>{status.textContent=text;status.className=(error?'warn':'muted')+' small';};
  async function checkUpdate(){
    if(busy)return;busy=true;check.disabled=true;install.hidden=true;
    try{
      if(!await tauri.core.invoke('updater_configured')){
        message('В этой сборке онлайн-обновление не настроено: требуется выпуск с ключом проверки подписи.');return;
      }
      if(update){await update.close();update=null;}
      message('Проверяем новую версию…');
      update=await tauri.updater.check({timeout:15000});
      message(update?'Доступна версия '+update.version+' (установлена '+update.currentVersion+').':'Установлена актуальная версия.');
      install.hidden=!update;
    }catch(err){message('Не удалось проверить обновления: '+(err?.message||String(err))+'. Можно повторить проверку позже.',true);}
    finally{busy=false;check.disabled=false;}
  }
  install.addEventListener('click',async()=>{
    if(busy||!update)return;
    if(state.desktopSyncing){message('Дождитесь завершения импорта Excel и повторите обновление.',true);return;}
    busy=true;state.appUpdating=true;check.disabled=install.disabled=true;
    const main=document.querySelector('main'),wasInert=main?.inert;
    try{
      if(!await confirm('Скачать и установить версию '+update.version+'? Программа закроется. Несохранённые изменения в формах будут потеряны. Сохранённые отчёты и настройки останутся.'))return;
      if(main)main.inert=true;
      let received=0,total=0;
      message('Скачиваем обновление…');
      await update.downloadAndInstall(event=>{
        if(event.event==='Started')total=event.data.contentLength||0;
        if(event.event==='Progress'){
          received+=event.data.chunkLength;
          message(total?'Загружено '+Math.min(100,Math.round(received/total*100))+'%':'Загружено '+Math.round(received/1024)+' КБ');
        }
        if(event.event==='Finished')message('Проверка подписи и установка обновления…');
      },{timeout:120000,restartAfterInstall:true});
      message('Обновление передано установщику. Если программа закрылась, дождитесь завершения установки.');
    }catch(err){message('Обновление не установлено: '+(err?.message||String(err))+'. Можно повторить попытку.',true);}
    finally{
      if(main)main.inert=wasInert;
      state.appUpdating=false;busy=false;check.disabled=install.disabled=false;
    }
  });
  check.addEventListener('click',checkUpdate);
  await checkUpdate();
})();
