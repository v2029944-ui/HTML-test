// SQL-only preference. No report JSON, bridge calls or changes to the browser HTML.
(function sqlStartupPreference(){
  'use strict';
  const checkbox=document.getElementById('sqlStartupPreference');
  const status=document.getElementById('sqlStartupPreferenceStatus');
  if(!checkbox||!status)return;
  const key='analiz-sql-startup-v1';
  try {
    checkbox.checked=window.localStorage.getItem(key)==='sql';
  }catch{
    checkbox.disabled=true;
    status.textContent='Настройка запуска недоступна: локальное хранилище запрещено.';
    return;
  }
  checkbox.addEventListener('change',()=>{
    const requested=checkbox.checked;
    try {
      if(requested)window.localStorage.setItem(key,'sql');
      else window.localStorage.removeItem(key);
      if(window.localStorage.getItem(key)!==(requested?'sql':null))
        throw Error('Настройка не сохранена');
      status.textContent=requested
        ? 'При следующем запуске откроется SQL-режим. Импорт Excel доступен в разделе «Excel и импорт».'
        : 'При следующем запуске откроется классический интерфейс.';
    }catch{
      checkbox.checked=!requested;
      status.textContent='Не удалось сохранить настройку запуска. Предыдущий режим сохранён.';
    }
  });
})();
