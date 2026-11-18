// Lightweight Tauri-only entrypoint. Never load the legacy report history here.
(function routeStartup(){
  'use strict';
  const preference='analiz-sql-startup-v1';
  let target='index.html';
  try {
    if(window.localStorage.getItem(preference)==='sql')target='native-workspace.html';
  }catch{/* Storage may be disabled; retain the familiar interface. */}
  try {
    window.location.replace(target);
  }catch{
    // Keep the static navigation links available instead of trapping the user.
    const message=document.getElementById('startupStatus');
    if(message)message.textContent='Не удалось выполнить переход автоматически. Выберите режим по ссылке ниже.';
  }
})();
