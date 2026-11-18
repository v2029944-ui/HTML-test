// Tauri SQL workspace: metadata-only view of the configured Excel directory.
// Never read XLSX bytes or import reports on a background folder event.
(function nativeFolderStatus(){
  'use strict';
  const api=window.desktopAPI;
  if(!api?.list||!api?.chooseFolder)return;
  const $=id=>document.getElementById(id);
  const choose=$('sourceFolderChoose'),refresh=$('sourceFolderRefresh');
  const location=$('sourceFolderLocation'),status=$('sourceFolderStatus');
  const rows=$('sourceFolderRows'),count=$('sourceFolderCount');
  if(!choose||!refresh||!location||!status||!rows||!count)return;

  const maxVisible=100;
  const make=(tag,value)=>{
    const node=document.createElement(tag);
    if(value!==undefined)node.textContent=String(value);
    return node;
  };
  let folder=null,busy=false,pending=false,changing=false,closed=false;
  const report=(message,error=false)=>{
    status.textContent=message;
    status.className='status '+(error?'error':'muted');
  };
  function lock(){
    choose.disabled=busy||changing;
    refresh.disabled=busy||changing;
  }
  async function load(){
    if(closed)return;
    if(busy||changing){pending=true;return;}
    busy=true;lock();
    try{
      const result=await api.list();
      if(!result||(!Array.isArray(result.files))||
          (result.folder!==null&&typeof result.folder!=='string'))
        throw Error('Некорректный список файлов из SQLite.');
      const files=result.files;
      folder=result.folder;
      location.textContent=folder?'Выбрана папка: '+folder:'Папка Excel ещё не выбрана.';
      count.textContent='Найдено XLSX: '+files.length+
        (files.length>maxVisible?' · показаны первые '+maxVisible:'');
      rows.replaceChildren();
      const rules=result.rules&&typeof result.rules==='object'?result.rules:{};
      for(const file of files.slice(0,maxVisible)){
        if(!file||typeof file.name!=='string'||!Number.isFinite(file.size)||
           file.size<0||typeof file.tooLarge!=='boolean')
          throw Error('Некорректные метаданные файла Excel.');
        const rule=Object.prototype.hasOwnProperty.call(rules,file.name);
        const tr=make('tr');
        tr.append(
          make('td',file.name),
          make('td',Math.ceil(file.size/1024)+' КБ'),
          make('td',file.tooLarge?'Превышен предел 30 МБ':rule?'Есть правило':'Правило не задано')
        );
        rows.append(tr);
      }
      report(folder?'Папка проверена. Изменения файлов не импортируются автоматически в SQL-режиме.':
        'Выберите папку, чтобы видеть доступные XLSX. Добавление отчётов выполняется только после проверки.');
    }catch(error){
      rows.replaceChildren();
      count.textContent='Список XLSX недоступен.';
      report('Не удалось проверить папку Excel: '+String(error?.message||error),true);
    }finally{
      busy=false;lock();
      if(pending&&!closed){pending=false;void load();}
    }
  }
  choose.addEventListener('click',async()=>{
    if(busy||changing)return;
    if(folder&&!window.confirm(
      'При выборе другой папки правила файлов прежней папки будут сброшены. Сохранённые отчёты в SQLite останутся. Продолжить?'
    ))return;
    changing=true;lock();
    try{
      const selected=await api.chooseFolder();
      if(selected)report('Папка выбрана. Список источников обновляется.');
      else report('Выбор папки отменён.');
    }catch(error){
      report('Не удалось выбрать папку: '+String(error?.message||error),true);
    }finally{
      changing=false;lock();
      await load();
    }
  });
  refresh.addEventListener('click',()=>{void load();});
  if(typeof api.onFilesChanged==='function'){
    const stop=api.onFilesChanged(()=>{
      if(!closed){report('Состав папки изменился. Проверьте файлы перед импортом.');void load();}
    });
    if(typeof stop==='function'&&typeof window.addEventListener==='function'){
      window.addEventListener('pagehide',()=>{closed=true;stop();},{once:true});
    }
  }
  window.nativeFolderReady=load();
})();
