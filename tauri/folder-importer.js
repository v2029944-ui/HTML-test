// Выполняется в Tauri через совместимый API; самостоятельный index.html работает без этого модуля.
(async function tauriFolderWorkspace(){
  'use strict';
  if(!window.desktopAPI)return;
  await window.appReady;
  const api=window.desktopAPI;
  const upload=document.getElementById('upload');
  const root=document.createElement('section');
  root.className='panel';
  root.setAttribute('aria-label','Автоматическое обновление отчётов с диска');
  const title=document.createElement('h2');
  title.textContent='Папка Excel · автоматическое обновление';
  const summary=document.createElement('p');
  summary.className='muted small';
  summary.textContent='Выберите формат файла. Для квартальных и годовых таблиц даты берутся из заголовков; при необходимости укажите год. При замене файла с тем же именем данные обновятся, пока программа открыта.';
  const buttons=document.createElement('div');buttons.className='actions';
  const choose=document.createElement('button');
  choose.className='button primary';choose.textContent='Выбрать папку с Excel';
  const refresh=document.createElement('button');
  refresh.className='button secondary';refresh.textContent='Проверить файлы сейчас';
  const openData=document.createElement('button');
  openData.className='button secondary';openData.textContent='Папка базы данных';
  const closeManual=document.createElement('button');
  closeManual.className='button secondary';closeManual.textContent='Закрыть ручную проверку';closeManual.hidden=true;
  closeManual.addEventListener('click',async()=>{
    resetWorkbookSelection();renderQuality();closeManual.hidden=true;await syncFiles();
  });
  buttons.append(choose,refresh,openData,closeManual);
  const location=document.createElement('p');location.className='muted small';
  const status=document.createElement('p');status.setAttribute('role','status');
  status.setAttribute('aria-live','polite');status.className='muted small';
  const list=document.createElement('div');
  const log=document.createElement('div');log.className='muted small';
  root.append(title,summary,buttons,location,status,list,log);
  upload.insertBefore(root,upload.firstChild);
  const privacy=document.querySelector('.privacy');
  if(privacy)privacy.textContent='Отчёты сохраняются на диске компьютера';
  const footer=document.querySelector('footer');
  if(footer)footer.textContent='Настольная версия · Excel и база хранятся локально · Интернет не требуется для работы';
  const note=document.getElementById('storageModeInfo');
  let running=false,rerun=false,scheduleTimer=null;
  let latest=null;

  function message(text,warning=false){
    status.textContent=text;
    status.className=(warning?'warn':'muted')+' small';
  }
  function node(tag,text,className){
    const el=document.createElement(tag);
    if(text!==undefined)el.textContent=text;
    if(className)el.className=className;
    return el;
  }
  function suggestedRule(name){
    const day=name.match(/(?:day|день|дневн(?:ой|ая|ые|ое))[\s_-]*(\d{4}-\d{2}-\d{2})/i);
    const week=name.match(/(?:week|неделя)[\s_-]*(\d{4}-\d{2}-\d{2})/i);
    const month=name.match(/(?:month|месяц)[\s_-]*(\d{4}-\d{2})/i);
    if(day)return {type:'day',start:day[1],sheet:''};
    if(week)return {type:'week',start:week[1],sheet:''};
    if(month)return {type:'month',start:month[1]+'-01',sheet:''};
    if(name.toLocaleLowerCase('ru-RU')==='анализ.xlsx')
      return {type:'week',start:'2026-10-19',sheet:''};
    return {type:'week',start:'',sheet:''};
  }
  function dateInput(type,start){
    const input=node('input');input.type=type==='month'?'month':'date';
    input.value=type==='month'?(start||'').slice(0,7):start||'';
    input.setAttribute('aria-label','Первый период файла');
    return input;
  }
  async function showFiles(){
    latest=await api.list();
    const info=await api.info();
    location.textContent=latest.folder?
      'Источник: '+latest.folder+' · База: '+info.dataFile:
      'Папка пока не выбрана. База хранится отдельно в '+info.dataFile+'.';
    if(note)note.textContent='Настольное хранилище: '+info.dataFile+'. Резервная копия: reports.json.bak. Данные не находятся в браузере.';
    list.replaceChildren();
    if(!latest.folder){list.append(node('p','Выберите папку с XLSX-файлами.','muted small'));return latest;}
    if(!latest.files.length){list.append(node('p','В папке нет файлов .xlsx. Добавьте отчёты и нажмите «Проверить файлы сейчас».','muted small'));return latest;}
    list.append(node('h3','Найденные файлы ('+latest.files.length+')'));
    for(const file of latest.files){
      const rule=latest.rules[file.name],initial=rule||suggestedRule(file.name);
      const line=node('div',undefined,'desktop-file-row');
      const filename=node('strong',file.name+' ('+Math.ceil(file.size/1024)+' КБ)','desktop-file-name');
      const fields=node('div',undefined,'desktop-file-fields');
      function field(label,control){
        const box=node('label',undefined,'desktop-file-field');
        box.append(node('span',label,'small'),control);fields.append(box);return box;
      }
      const format=node('select');
      for(const [value,label] of [['auto','Автоматически'],['legacy','Обычные периоды'],['daily-wide','Дневные данные за квартал / год']]){
        const option=node('option',label);option.value=value;format.append(option);
      }
      format.value=initial.format||(rule?'legacy':'auto');
      field('Формат Excel',format);
      const type=node('select');
      for(const [key,label] of [['week','Недели'],['month','Месяцы'],['day','Дни']]){
        const opt=node('option',label);opt.value=key;type.append(opt);
      }
      type.value=initial.type;
      const typeField=field('Тип периода',type);
      let start=dateInput(initial.type,initial.start);
      const startField=field('Дата первого периода',start);
      type.addEventListener('change',()=>{
        const fresh=dateInput(type.value,'');
        start.replaceWith(fresh);start=fresh;
      });
      const sheet=node('input');sheet.type='text';sheet.value=initial.sheet||'';
      sheet.placeholder='Автоматически';
      const sheetField=field('Лист обычного отчёта',sheet);
      const year=node('input');year.type='number';year.min='1900';year.max='9999';year.step='1';
      year.value=initial.year||'';year.placeholder='Из заголовков Excel';
      const yearField=field('Год (если отсутствует в Excel)',year);
      const sheetBox=node('fieldset',undefined,'desktop-daily-sheets');
      sheetBox.append(node('legend','Листы дневных данных'));
      const sheetHint=node('p','Все подходящие листы. Нажмите «Найти листы», чтобы выбрать отдельные.','muted small');
      const sheetChoices=node('div',undefined,'desktop-sheet-choices');
      const findSheets=node('button','Найти листы','button secondary');findSheets.type='button';findSheets.disabled=file.tooLarge;
      sheetBox.append(sheetHint,sheetChoices,findSheets);
      let workbook=null,chosenSheets=Array.isArray(initial.sheets)?initial.sheets.slice():[];
      function visibility(){
        const daily=format.value==='daily-wide';
        typeField.hidden=daily;startField.hidden=daily;sheetField.hidden=daily;
        yearField.hidden=format.value==='legacy';sheetBox.hidden=format.value==='legacy';
      }
      format.addEventListener('change',visibility);visibility();
      async function readWorkbook(){
        const source=await api.readExcel(file.name);
        if(source.signature!==file.signature||!await api.checkExcel(file.name,source.signature))
          throw Error('Файл изменился. Обновите список файлов и повторите проверку.');
        workbook=window.XLSX.read(new Uint8Array(source.bytes),{type:'array',cellDates:true});
        return workbook;
      }
      findSheets.addEventListener('click',async()=>{
        findSheets.disabled=true;
        try{
          const book=await readWorkbook(),names=findDailySheets(book);
          sheetChoices.replaceChildren();
          sheetHint.textContent=names.length?'Отметьте листы для импорта. Все сняты — все подходящие листы.':'Подходящие листы не определены. Используйте ручную проверку и настройку колонок.';
          for(const name of names){
            const label=node('label'),check=node('input');check.type='checkbox';check.value=name;
            check.checked=!chosenSheets.length||chosenSheets.includes(name);
            check.addEventListener('change',()=>{chosenSheets=Array.from(sheetChoices.querySelectorAll('input:checked'),x=>x.value);});
            label.append(check,document.createTextNode(name));sheetChoices.append(label);
          }
        }catch(err){message(file.name+': '+err.message,true);}
        finally{findSheets.disabled=file.tooLarge;}
      });
      function dailyOptions(){
        const value=year.value.trim();
        if(value&&(!/^\d{4}$/.test(value)||Number(value)<1900||Number(value)>9999))throw Error('Год должен быть целым числом от 1900 до 9999.');
        return {year:value?Number(value):undefined,sheets:chosenSheets.slice(),mappings:initial.mappings||{}};
      }
      const save=node('button',rule?'Сохранить правило':'Настроить автоимпорт','button secondary');
      save.type='button';save.disabled=file.tooLarge;
      save.addEventListener('click',async()=>{
        save.disabled=true;
        try{
          let daily=format.value==='daily-wide';
          if(format.value==='auto')daily=findDailySheets(await readWorkbook()).length>0;
          let nextRule;
          if(daily){
            const options=dailyOptions();
            // Validate the complete source before enabling automatic updates.
            parseDailyWorkbook(workbook||await readWorkbook(),{...options,fileName:file.name});
            nextRule={format:'daily-wide',type:'day',start:'',...options};
          }else{
            const raw=start.value,day=type.value==='month'?raw+'-01':raw;
            nextRule={type:type.value,start:day,sheet:sheet.value,format:format.value};
          }
          await api.setRule(file.name,nextRule);
          message('Правило для '+file.name+' сохранено.');
          await showFiles();
          await syncFiles();
        }catch(err){message(file.name+': '+err.message,true);}
        finally{save.disabled=file.tooLarge;}
      });
      const manual=node('button','Проверить и сохранить вручную','button secondary');
      manual.type='button';manual.disabled=file.tooLarge;
      manual.addEventListener('click',async()=>{
        if(running){message('Дождитесь завершения синхронизации.',true);return;}
        running=true;state.desktopSyncing=true;manual.disabled=true;
        try{
          const raw=start.value,first=type.value==='month'?raw+'-01':raw;
          const folder=latest.folder;
          const source=await api.readExcel(file.name);
          const listing=await api.list();
          if(listing.folder!==folder||source.signature!==file.signature||!await api.checkExcel(file.name,source.signature))
            throw Error('Файл изменился. Обновите список файлов и повторите проверку.');
          await selectWorkbook(new Uint8Array(source.bytes),file.name);
          if(format.value==='daily-wide')$('importFormat').value='daily-wide';
          if(format.value==='legacy')$('importFormat').value='legacy';
          syncImportFormat();
          let selected='';
          if($('importFormat').value==='daily-wide'){
            configureDailyImport(dailyOptions());
          }else{
            if(!validReportDate(first))throw Error('Укажите дату первого периода файла.');
            $('importType').value=type.value;$('importType').dispatchEvent(new Event('change'));
            $(type.value==='month'?'importMonth':'importWeek').value=type.value==='month'?first.slice(0,7):first;
            selected=sheet.value.trim();
            if(!selected)selected=state.workbook.SheetNames.find(name=>{
              try{return parseSheet(state.workbook.Sheets[name]).some(x=>x.rows.length);}catch{return false;}
            });
            if(!selected||!state.workbook.Sheets[selected])throw Error('Отчётный лист не найден. Выберите его в обычном импорте.');
            $('sheetSelect').value=selected;
            $('importPeriodDates').replaceChildren();renderImportDateFields();
          }
          state.folderImport={name:file.name,folder:listing.folder,signature:source.signature,
            sha256:source.sha256,rule:listing.rules[file.name],sheet:selected};
          $('excelFileStatus').textContent='Из папки: '+file.name;
          $('excelFileStatus').title=file.name;
          closeManual.hidden=false;
          await inspectImport();
          message('Предпросмотр открыт ниже. Проверьте замечания, затем нажмите «Сохранить отчёты». Автоимпорт приостановлен до сохранения или закрытия ручной проверки.');
          $('importPreview').scrollIntoView({behavior:'smooth',block:'start'});
        }catch(err){message(file.name+': '+err.message,true);}
        finally{running=false;state.desktopSyncing=false;manual.disabled=file.tooLarge;if(rerun){rerun=false;scheduleSync();}}
      });
      const actions=node('div',undefined,'desktop-file-actions');actions.append(save,manual);
      line.append(filename,fields,sheetBox,actions);
      const imported=state.reports.filter(r=>r.desktopSource===file.name).map(r=>r.importedAt).filter(x=>x&&Number.isFinite(Date.parse(x))).sort().at(-1);
      if(imported)line.append(node('p','Последнее успешное обновление: '+new Date(imported).toLocaleString('ru-RU'),'muted small'));
      if(!rule)line.append(node('small','Настройте правило для автоматического обновления.','warn small'));
      if(file.tooLarge)line.append(node('small','Слишком большой файл (максимум 30 МБ).','warn small'));
      list.append(line);
    }
  }
  function parseExcel(file,rule,bytes,signature,sha256){
    if(!window.XLSX)throw Error('Встроенный разбор Excel не загрузился.');
    const workbook=window.XLSX.read(bytes,{type:'array',cellDates:true});
    if(rule.format==='daily-wide'||(rule.format==='auto'&&findDailySheets(workbook).length>0)){
      const result=parseDailyWorkbook(workbook,{year:rule.year,sheets:rule.sheets,mappings:rule.mappings,fileName:file.name});
      const reports=result.reports;
      if(!Array.isArray(reports)||!reports.length)throw Error('Не найдены дневные отчёты. Предыдущие данные сохранены.');
      if((result.issues||[]).length||(result.warnings||[]).length||result.hasMismatch||reports.some(r=>(!r.rows.length&&!r.explicitZero)||(r.issues||[]).length||(r.warnings||[]).length||r.hasMismatch)){
        const affected=reports.filter(r=>(!r.rows.length&&!r.explicitZero)||(r.issues||[]).length||(r.warnings||[]).length||r.hasMismatch);
        const sheets=[...new Set(affected.map(r=>r.sheetName).filter(Boolean))];
        throw Error('Есть замечания к дневным данным Excel'+(sheets.length?' (листы: '+sheets.join(', ')+')':'')+'. Откройте ручную проверку. Предыдущие данные сохранены.');
      }
      const dates=new Set(),when=new Date().toISOString(),key=importRuleKey(rule);
      return reports.map(report=>{
        if(report.type!=='day'||!validReportDate(report.start)||report.end!==report.start||dates.has(report.start))
          throw Error('Некорректная или повторяющаяся дата дневного отчёта. Предыдущие данные сохранены.');
        dates.add(report.start);
        return {...report,id:uuid(),importedAt:when,desktopSource:file.name,
          desktopSignature:signature,desktopRule:key,desktopHash:sha256};
      });
    }
    const candidates=rule.sheet?[rule.sheet]:workbook.SheetNames;
    let selected=null;
    for(const sheetName of candidates){
      if(!workbook.Sheets[sheetName])continue;
      try{
        const parsed=parseSheet(workbook.Sheets[sheetName]);
        if(parsed.length&&parsed.some(x=>x.rows.length)){
          selected={sheetName,parsed};break;
        }
      }catch(err){
        if(rule.sheet)throw err;
      }
    }
    if(!selected)throw Error('Не найден подходящий лист с колонками «Работа / Количество / Сумма». Настройте обычный импорт вручную.');
    if(selected.parsed.some(x=>!x.rows.length||x.warnings.length||x.issues.length||x.hasMismatch)){
      const issues=[...new Set(selected.parsed.flatMap(x=>x.warnings))].slice(0,3).join('; ');
      throw Error('Есть замечания к Excel. Предыдущие данные сохранены. '+(issues||'Проверьте пустые периоды и итоги.'));
    }
    const when=new Date().toISOString(),key=importRuleKey(rule);
    const reports=selected.parsed.map((item,i)=>{
      const start=rule.type==='month'?addMonths(rule.start,i):shiftPeriod(rule.type,rule.start,i);
      return {id:uuid(),type:rule.type,start,end:periodEnd(start,rule.type),
        fileName:file.name,sheetName:selected.sheetName,sourceBlock:item.index,
        sourceCol:item.col,sourceCountCol:item.countCol,sourceAmountCol:item.amountCol,
        rows:item.rows,warnings:item.warnings,issues:item.issues,totals:item.totals,
        count:item.count,amount:item.amount,hasMismatch:item.hasMismatch,
        organizations:item.organizations,importedAt:when,
        desktopSource:file.name,desktopSignature:signature,
        desktopRule:key,desktopHash:sha256};
    });
    return reports;
  }
  async function syncFiles(force=false){
    if(state.folderImport||state.appUpdating)return;
    if(state.desktopBlocked){message('Сохранение на диск заблокировано из-за ошибки чтения базы.',true);return;}
    if(running){rerun=true;return;}
    running=true;state.desktopSyncing=true;choose.disabled=true;refresh.disabled=true;
    try{
      const listing=await api.list();
      if(!listing.folder){message('Сначала выберите папку.');return;}
      const previous=state.reports;
      let next=previous.slice(),updated=0,unchanged=0,skipped=0;
      const errors=[];
      for(const file of listing.files){
        const rule=listing.rules[file.name];
        if(!rule){skipped++;continue;}
        if(file.tooLarge){errors.push(file.name+': превышен лимит 30 МБ');continue;}
        const sameSource=r=>r.desktopSource===file.name&&(!r.desktopFolder||r.desktopFolder===listing.folder);
        const known=next.filter(sameSource);
        const ruleKey=importRuleKey(rule);
        if(!force&&known.length&&known.every(r=>
          r.desktopSignature===file.signature&&r.desktopRule===ruleKey)){
          unchanged++;continue;
        }
        try{
          const source=await api.readExcel(file.name);
          if(source.signature!==file.signature)throw Error('Файл меняется, повторим проверку.');
          const parsed=parseExcel(file,rule,new Uint8Array(source.bytes),source.signature,source.sha256);
          for(const report of parsed)report.desktopFolder=listing.folder;
          const existing=next.filter(r=>!sameSource(r));
          const accepted=existing.slice();
          for(const item of parsed){
            const overlap=accepted.find(r=>r.type===item.type&&r.start<=item.end&&r.end>=item.start);
            if(overlap)throw Error('Период '+displayPeriod(item)+' уже принадлежит '+(overlap.fileName||'другому отчёту')+'. Импорт отменён.');
            accepted.push(item);
          }
          if(!await api.checkExcel(file.name,source.signature))
            throw Error('Файл изменён во время обработки; попробуйте после копирования.');
          next=existing.concat(parsed);
          updated++;
        }catch(err){errors.push(file.name+': '+err.message);}
      }
      if(updated){
        await storeReports(next);
        // Automatic sync must not discard the user's selected work filters.
        renderAll();
      }
      message('Синхронизация: обновлено файлов '+updated+
        ', без изменений '+unchanged+', ожидают настройки '+skipped+
        (errors.length?', ошибок '+errors.length+'.':'.'),errors.length>0);
      log.replaceChildren();
      errors.forEach(error=>log.append(node('p',error,'warn small')));
      const stored=new Set(next.map(r=>r.desktopSource).filter(Boolean));
      const present=new Set(listing.files.map(f=>f.name));
      for(const name of stored)if(!present.has(name))
        log.append(node('p','Файл '+name+' удалён из папки. Его последние отчёты сохранены на диске для защиты от случайной потери.','warn small'));
      // Не перезаписываем чужие отчёты; удаление источника не стирает данные.
    }catch(err){message('Ошибка синхронизации: '+err.message,true);}
    finally{
      running=false;state.desktopSyncing=false;choose.disabled=false;refresh.disabled=false;
      if(rerun){rerun=false;scheduleSync();}
    }
  }
  function scheduleSync(){
    clearTimeout(scheduleTimer);
    scheduleTimer=setTimeout(async()=>{await showFiles();await syncFiles();},1800);
  }
  choose.addEventListener('click',async()=>{
    try{
      const chosen=await api.chooseFolder();
      if(chosen){await showFiles();await syncFiles();}
    }catch(err){message('Не удалось выбрать папку: '+err.message,true);}
  });
  refresh.addEventListener('click',async()=>{try{await showFiles();await syncFiles(true);}catch(err){message(err.message,true);}});
  openData.addEventListener('click',async()=>{try{
    const error=await api.openDataFolder();
    if(error)throw Error(error);
  }catch(err){message(err.message,true);}});
  api.onFilesChanged(scheduleSync);
  try{
    await showFiles();
    await syncFiles();
  }catch(err){message('Не удалось прочитать папку Excel: '+err.message,true);}
})();


