/* One installed application; initialize no original full-history handlers. */
(function(){
'use strict';
if(window.appReady)throw Error('Приложение уже запущено.');
window.appReady=Promise.resolve().then(async()=>{
 const UI=window.unifiedUI;
 if(!UI)throw Error('Не загружен интерфейс приложения.');
 for(const name of ['unifiedManagement','unifiedReports','unifiedExtended','unifiedAnalytics']){
  if(typeof window[name]?.init!=='function')throw Error('Не загружен модуль '+name+'.');
 }
 for(const name of ['unifiedManagement','unifiedReports','unifiedExtended','unifiedAnalytics'])window[name].init();
 UI.init();
 window.referenceDesktop?.init();
 await UI.refresh();
 return true;
}).catch(error=>{
 window.unifiedUI?.notice('Не удалось открыть приложение: '+(error?.message||String(error)),true);
 // Keep the error visible and leave retry to the common SQLite controller.
 return false;
});
})();
