(async function onlineUpdates(){
  'use strict';
  await window.appReady;
  const tauri=window.__TAURI__,settings=document.getElementById('storage'),main=document.querySelector('main');
  if(!tauri?.updater||!settings||!main)return;
  const deferredKey='work-reports-update-deferred-v1',operations=window.tauriOperations;
  let busy=false,update=null;
  const panel=elem('section',null,'panel');panel.id='onlineUpdates';
  panel.append(elem('h2','Обновление программы'),elem('p','Новая версия скачивается и устанавливается только после подтверждения. Отчёты и настройки сохраняются. Программа закроется на время установки.','muted small'));
  const status=elem('p','Проверка доступности обновлений…','muted small');status.id='appUpdateStatus';
  status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  const actions=elem('div',null,'actions');
  const button=(id,text,style='secondary')=>{const b=elem('button',text,'button '+style);b.id=id;b.type='button';return b;};
  const check=button('checkAppUpdate','Проверить обновления'),install=button('installAppUpdate','Обновить','primary'),notes=button('appUpdateWhatsNew','Что нового');
  install.hidden=notes.hidden=true;actions.append(check,notes,install);panel.append(status,actions);settings.append(panel);
  const banner=elem('section',null,'panel update-notice');banner.id='appUpdateNotice';banner.hidden=true;banner.setAttribute('aria-label','Доступно обновление программы');
  const title=elem('h2');title.id='appUpdateNoticeTitle';title.setAttribute('role','status');title.setAttribute('aria-live','polite');
  const bannerActions=elem('div',null,'actions'),bannerNotes=button('noticeWhatsNew','Что нового'),later=button('deferAppUpdate','Позже'),bannerInstall=button('noticeInstallUpdate','Обновить','primary');
  const bannerStatus=elem('p',null,'muted small');bannerStatus.id='appUpdateNoticeStatus';bannerStatus.hidden=true;
  bannerStatus.setAttribute('role','status');bannerStatus.setAttribute('aria-live','polite');
  bannerActions.append(bannerNotes,later,bannerInstall);banner.append(title,bannerStatus,bannerActions);main.prepend(banner);
  const management=document.getElementById('managementToggle'),badge=elem('span','●','update-badge');badge.id='appUpdateBadge';badge.hidden=true;
  const originalLabel=management?.getAttribute('aria-label')||'Управление';management?.append(badge);
  const dialog=elem('dialog',null,'update-notes');dialog.id='appUpdateNotes';dialog.setAttribute('aria-labelledby','appUpdateNotesHeading');
  const notesHeading=elem('h2');notesHeading.id='appUpdateNotesHeading';const notesBody=elem('div',null,'update-notes-body'),closeNotes=button('closeUpdateNotes','Закрыть');
  dialog.append(notesHeading,notesBody,closeNotes);main.append(dialog);closeNotes.addEventListener('click',()=>dialog.close());
  const message=(text,error=false,showInNotice=false)=>{
    status.textContent=text;status.className=(error?'warn':'muted')+' small';
    bannerStatus.textContent=text;bannerStatus.className=(error?'warn':'muted')+' small';
    bannerStatus.hidden=!update||(!error&&!showInNotice);
    // The settings panel may be hidden while the user initiates an update from Analytics.
    if(update&&error)banner.hidden=false;
  };
  function showNotes(){if(!update)return;notesHeading.textContent='Что нового в версии '+update.version;notesBody.textContent=(typeof update.body==='string'&&update.body.trim())||'Описание изменений для этой версии не опубликовано.';if(!dialog.open)dialog.showModal();}
  notes.addEventListener('click',showNotes);bannerNotes.addEventListener('click',showNotes);
  function showAvailable(manual){
    install.hidden=notes.hidden=badge.hidden=!update;
    if(management)management.setAttribute('aria-label',update?originalLabel+', доступно обновление '+update.version:originalLabel);
    badge.title=update?'Доступна версия '+update.version:'';badge.setAttribute('aria-label',badge.title);
    let deferred='';try{deferred=localStorage.getItem(deferredKey)||'';}catch{/* Without preferences, show the notification on each launch. */}
    banner.hidden=!update||(!manual&&deferred===update.version);
    if(update)title.textContent='Доступна новая версия '+update.version;
  }
  later.addEventListener('click',()=>{
    if(!update||busy)return;
    banner.hidden=true;
    try{localStorage.setItem(deferredKey,update.version);}
    catch{message('Уведомление скрыто до перезапуска. Сохранить отсрочку не удалось: локальные предпочтения недоступны.',true);}
  });
  function blocked(){
    if(state.desktopSyncing||state.excelImports||state.folderImport||operations?.reads)return 'Завершите импорт или ручную проверку Excel и повторите обновление.';
    if(operations?.saves)return 'Дождитесь завершения сохранения данных и повторите обновление.';
    return '';
  }
  function lock(value){busy=value;check.disabled=install.disabled=bannerInstall.disabled=later.disabled=value;}
  const backupEvent='html-test-backup-update-progress';
  async function checkBackupUpdate(){
    const info=await tauri.core.invoke('check_backup_update');
    if(!info)return null;
    if(typeof info.version!=='string'||!info.version||typeof info.currentVersion!=='string')
      throw Error('Некорректный ответ резервного сервера обновлений.');
    return {...info,fromBackup:true,close:async()=>{},
      downloadAndInstall:async onEvent=>{
        const unlisten=await tauri.event?.listen?.(backupEvent,({payload})=>onEvent(payload));
        try{await tauri.core.invoke('install_backup_update',{expectedVersion:info.version});}
        finally{if(unlisten)unlisten();}
      }
    };
  }
  function networkDownloadError(err){
    const text=String(err?.message||err||'');
    return !/signature|подпис|checksum|digest|permission|диск|места на диске/i.test(text)
      && /network|connection|connect|download|request|timeout|timed out|dns|proxy|socket|tls|http|status|offline|сеть|соединени|загрузк|тайм-аут/i.test(text);
  }
  async function checkUpdate(manual=false){
    if(busy)return;lock(true);
    try{
      if(!await tauri.core.invoke('updater_configured')){if(update){await update.close();update=null;}showAvailable(false);message('В этой сборке онлайн-обновление не настроено: требуется выпуск с ключом проверки подписи.');return;}
      if(update){await update.close();update=null;}
      showAvailable(false);message('Проверяем новую версию…');
      try{update=await tauri.updater.check({timeout:15000});}
      catch(primaryError){
        try{
          update=await checkBackupUpdate();
        }catch(backupError){
          throw Error('GitHub: '+(primaryError?.message||primaryError)+
            '; Яндекс: '+(backupError?.message||backupError));
        }
        if(!update)throw Error('GitHub недоступен, а Яндекс не предлагает новую версию; актуальность обновлений не подтверждена.');
      }
      message(update?'Доступна версия '+update.version+' (установлена '+update.currentVersion+
        (update.fromBackup?'; источник — Яндекс':'')+').':'Установлена актуальная версия.');
      showAvailable(manual);
    }catch(err){showAvailable(false);message('Не удалось проверить обновления: '+(err?.message||String(err))+'. Можно повторить проверку позже.',true);}
    finally{lock(false);}
  }
  async function installUpdate(){
    if(busy||!update)return;
    const reason=blocked();if(reason){message(reason,true);return;}
    lock(true);state.appUpdating=true;if(operations)operations.updating=true;
    const wasInert=main.inert;
    try{
      if(!await confirm('Скачать и установить версию '+update.version+'? Программа закроется. Несохранённые изменения в формах будут потеряны. Сохранённые отчёты и настройки останутся.'))return;
      const changed=blocked();if(changed){message(changed,true);return;}
      main.inert=true;let received=0,total=0;message('Скачиваем обновление…',false,true);
      const progress=event=>{
        if(event.event==='Started'||event.data?.contentLength)total=event.data?.contentLength||total;
        if(event.event==='Progress'){received+=event.data.chunkLength;message(total?'Загружено '+Math.min(100,Math.round(received/total*100))+'%':'Загружено '+Math.round(received/1024)+' КБ',false,true);}
        if(event.event==='Finished')message('Проверка подписи и установка обновления…',false,true);
      };
      try{
        await update.downloadAndInstall(progress,{timeout:120000,restartAfterInstall:true});
      }catch(primaryError){
        if(update.fromBackup||!networkDownloadError(primaryError))throw primaryError;
        let reserve;
        try{reserve=await checkBackupUpdate();}
        catch(error){throw Error('GitHub: '+(primaryError?.message||primaryError)+'; Яндекс: '+(error?.message||error));}
        if(!reserve||reserve.version!==update.version)
          throw Error('GitHub: '+(primaryError?.message||primaryError)+'; на Яндексе нет подтверждённой версии '+update.version+'.');
        await update.close();
        update=reserve;received=0;total=0;
        message('Скачиваем ту же подписанную версию с Яндекса…',false,true);
        await update.downloadAndInstall(progress,{timeout:120000,restartAfterInstall:true});
      }
      message('Обновление передано установщику. Если программа закрылась, дождитесь завершения установки.',false,true);
    }catch(err){message('Обновление не установлено: '+(err?.message||String(err))+'. Можно повторить попытку.',true);}
    finally{main.inert=wasInert;state.appUpdating=false;if(operations)operations.updating=false;lock(false);}
  }
  install.addEventListener('click',installUpdate);bannerInstall.addEventListener('click',installUpdate);
  check.addEventListener('click',()=>checkUpdate(true));
  await checkUpdate();
})();
