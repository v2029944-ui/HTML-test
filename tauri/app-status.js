// Installed app identity and real IPC activity, shared by both Tauri screens.
// This indicator never invents progress percentages or changes report data.
(function appStatus(){
  'use strict';
  const $=id=>document.getElementById(id);
  const host=document.querySelector?.('header .header-actions')||document.querySelector?.('header');
  function mount(id,tag,className){
    let element=$(id);
    if(!element&&host&&typeof document.createElement==='function'){
      element=document.createElement(tag);element.id=id;element.className=className;
      host.append(element);
    }
    return element;
  }
  const version=mount('appVersion','span','app-version');
  const activity=mount('appActivity','div','app-activity');
  let activityText=$('appActivityText');
  if(activity){
    activity.setAttribute('role','status');activity.setAttribute('aria-live','polite');
    activity.setAttribute('aria-atomic','true');
    if(!activityText&&typeof document.createElement==='function'){
      const spinner=document.createElement('span');spinner.className='app-activity-spinner';
      spinner.setAttribute('aria-hidden','true');
      activityText=document.createElement('span');activityText.id='appActivityText';
      activity.append(spinner,activityText);
    }
  }
  const main=$('sqlMain')||document.querySelector?.('main');
  function renderActivity(detail){
    const reads=Number(detail?.reads)||0,saves=Number(detail?.saves)||0;
    const busy=reads>0||saves>0;
    if(activity)activity.hidden=!busy;
    if(main)main.setAttribute('aria-busy',String(busy));
    if(activityText)activityText.textContent=saves>0
      ?'Сохранение данных…':reads>0?'Загрузка отчётов и расчёт показателей…':'';
  }
  renderActivity(window.tauriOperations);
  window.addEventListener?.('tauri-operation-change',event=>renderActivity(event.detail));
  if(version){
    version.textContent='Определяем версию…';
    const unavailable=()=>{version.textContent='Версия недоступна';};
    try{
      const app=window.__TAURI__?.app;
      if(typeof app?.getVersion!=='function'){unavailable();return;}
      Promise.resolve(app.getVersion()).then(value=>{
        if(typeof value==='string'&&value.trim())version.textContent='Версия '+value.trim();
        else unavailable();
      },unavailable);
    }catch{unavailable();}
  }
})();
