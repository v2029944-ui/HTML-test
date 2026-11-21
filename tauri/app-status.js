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
  function dismissButton(label,action){
    const button=document.createElement('button');
    button.type='button';button.className='status-dismiss';button.title=label;
    button.setAttribute('aria-label',label);button.onclick=action;
    return button;
  }
  function returnFocus(opener){
    const target=opener?.isConnected && opener!==document.body && !opener.disabled && opener.getClientRects().length
      ?opener:document.querySelector('#toolsPage button[aria-pressed="true"],#sharedHeader [aria-current="page"]');
    target?.focus({preventScroll:true});
  }
  const flash=$('flash');
  if(flash){
    const message=document.createElement('span');
    let opener=null;
    const dismiss=dismissButton('Закрыть уведомление',()=>{
      const restoreFocus=document.activeElement===dismiss;
      flash.hidden=true;
      message.textContent='';
      if(restoreFocus)returnFocus(opener);
      opener=null;
    });
    flash.append(message,dismiss);
    window.showDesktopNotice=(text,error)=>{
      if(document.activeElement!==dismiss)opener=document.activeElement;
      message.textContent=text;
      flash.classList.toggle('error',Boolean(error));
      flash.hidden=false;
    };
  }
  const version=mount('appVersion','span','app-version');
  const activity=mount('appActivity','div','app-activity');
  let activityText=$('appActivityText');
  let activityDismissed=false,activityOpener=null,wasBusy=false;
  if(activity){
    activity.setAttribute('role','status');activity.setAttribute('aria-live','polite');
    activity.setAttribute('aria-atomic','true');
    if(!activityText&&typeof document.createElement==='function'){
      const spinner=document.createElement('span');spinner.className='app-activity-spinner';
      spinner.setAttribute('aria-hidden','true');
      activityText=document.createElement('span');activityText.id='appActivityText';
      activity.append(spinner,activityText);
    }
    const dismiss=dismissButton('Закрыть уведомление о ходе операции',()=>{
      activityDismissed=true;
      const restoreFocus=document.activeElement===dismiss;
      activity.hidden=true;
      if(restoreFocus)returnFocus(activityOpener);
    });
    activity.append(dismiss);
  }
  const main=$('sqlMain')||document.querySelector?.('main');
  let showTimer=null,readVisible=false;
  function renderActivity(detail){
    const reads=Number(detail?.reads)||0,saves=Number(detail?.saves)||0;
    const busy=reads>0||saves>0;
    if(busy && !wasBusy)activityOpener=document.activeElement;
    if(!busy){activityDismissed=false;activityOpener=null;}
    wasBusy=busy;
    if(!busy){if(showTimer!==null)clearTimeout(showTimer);showTimer=null;readVisible=false;}
    if(activity&&busy&&!saves&&!readVisible&&!activityDismissed&&showTimer===null){
      showTimer=setTimeout(()=>{showTimer=null;readVisible=true;if(wasBusy&&!activityDismissed)activity.hidden=false;},200);
    }
    if(activity)activity.hidden=!busy||activityDismissed||(!saves&&!readVisible);
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
