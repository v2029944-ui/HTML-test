/* The active workspace is selected by Rust. Every data request captures its identity. */
(function () {
  'use strict';
  const raw = (name, args = {}) => window.__TAURI__.core.invoke(name, args);
  let state = null, bootstrap = null, ready = false, timer = null, busy = false;
  let transfer = null;
  const selectedTransferIds = new Set();
  const statusMessage = error => window.analizUserError ? window.analizUserError(error) : error?.message || String(error);

  function accepted(result) {
    if (!result || !['personal','shared'].includes(result.mode) || typeof result.workspace_token !== 'string' ||
        !result.workspace_token || typeof result.available !== 'boolean' || typeof result.pending !== 'boolean' ||
        (result.mode === 'shared' && typeof result.folder !== 'string')) {
      throw Error('Приложение не смогло определить активную базу.');
    }
    return Object.freeze({...result});
  }

  function initial() {
    if (state) return Promise.resolve(state);
    if (!bootstrap) bootstrap = raw('shared_workspace_status')
      .then(result => (state = accepted(result))).finally(() => { bootstrap = null; });
    return bootstrap;
  }

  async function invoke(command, args) {
    const selected = state || await initial();
    if (!selected.available || selected.pending) throw Error(selected.error ? statusMessage(selected.error) : 'Общая база временно недоступна.');
    const response = await raw(command, {...args, workspaceToken: selected.workspace_token});
    if (selected.workspace_token !== state?.workspace_token) throw Error('Активная база изменилась. Повторите действие.');
    return response;
  }

  const node = id => document.getElementById(id);
  function render(error) {
    const status = node('sharedWorkspaceStatus'), message = node('sharedWorkspaceMessage');
    let label;
    if (state?.mode === 'shared') {
      const connection = state.pending ? 'Ожидается подтверждение сохранения' :
        !state.available ? 'Связь с общей базой отсутствует' : 'Общая база подключена';
      label = connection + ' · ' + state.folder +
        (Number.isSafeInteger(state.generation) ? ' · версия ' + state.generation : '');
    } else label = state ? 'Общая папка не подключена.' : 'Не удалось определить активную базу.';
    const mode = node('sharedWorkspaceMode');
    const summary = state?.mode === 'personal' ? 'Личная база' :
      state?.pending ? 'Общая база · сохранение ожидает отправки' :
      state?.mode === 'shared' ? (state.available ? 'Общая база подключена' : 'Общая база недоступна') : 'База не определена';
    mode.textContent = summary;
    const shared = state?.mode === 'shared', local = state?.mode === 'personal';
    node('personalDatabaseCard').dataset.selected = String(local);
    node('sharedDatabaseCard').dataset.selected = String(shared);
    node('personalDatabaseSelection').textContent = local ? '✓ Используется сейчас' : '';
    node('sharedDatabaseSelection').textContent = shared ? '✓ Используется сейчас' : '';
    node('sharedWorkspaceDisconnect').hidden = !shared;
    node('sharedWorkspaceDisconnect').disabled = busy || Boolean(state?.pending);
    node('sharedWorkspaceConnect').textContent = shared ? 'Выбрать другую общую папку' : 'Подключить общую папку';
    if (status.textContent !== label) status.textContent = label;
    const detail = error || (state?.error ? statusMessage(state.error) : '');
    if (message.textContent !== detail) message.textContent = detail;
    message.hidden = !message.textContent;
    node('sharedWorkspaceConnect').disabled = busy || Boolean(state?.pending);
    node('sharedWorkspaceRetry').hidden = !error && Boolean(state?.available) && !state?.pending;
    node('sharedWorkspaceRetry').textContent = state?.pending ? 'Повторить отправку' : 'Повторить подключение';
    node('sharedWorkspaceRetry').disabled = busy;
    node('sharedWorkspaceDiscard').hidden = !state?.pending;
    node('sharedWorkspaceDiscard').disabled = busy;
    renderTransfer();
  }

  function schedule() {
    clearTimeout(timer);
    if (ready && (!state || state.mode === 'shared')) timer = setTimeout(() => check(false), 5000);
  }

  async function apply(result, force = false) {
    const previous = state;
    state = accepted(result);
    if (previous?.workspace_token !== state.workspace_token) { transfer = null; selectedTransferIds.clear(); }
    if (previous && (previous.workspace_token !== state.workspace_token || (previous.available && !state.available))) {
      window.nativeFolderAutoImport?.forgetWorkspaceSession();
    }
    render();
    if (force || !previous || state.workspace_token !== previous.workspace_token ||
        state.available !== previous.available || state.pending !== previous.pending) {
      await window.unifiedUI.refreshWorkspace();
    }
  }

  async function check(manual) {
    if (busy) return;
    if (window.tauriOperations?.updating) { schedule(); return; }
    busy = true;
    let error = '';
    try {
      let result;
      try { result = await raw('shared_workspace_sync'); }
      catch (failure) {
        if (state?.mode !== 'shared') throw failure;
        // Preserve the configured folder on transport failure and block stale reads/writes.
        result = {...state, available: false, error: statusMessage(failure)};
      }
      await apply(result, manual && !window.unifiedUI.snapshot);
    } catch (failure) {
      error = statusMessage(failure);
    } finally {
      busy = false;
      render(error);
      schedule();
    }
  }

  async function connect() {
    if (busy || !state || state.pending || window.tauriOperations?.updating || window.tauriOperations?.saves) return;
    busy = true;
    render();
    let error = '';
    try {
      const path = await window.desktopAPI.chooseSharedFolder();
      if (!path) return;
      if (state?.mode === 'shared' && !state.available) window.nativeFolderAutoImport?.forgetWorkspaceSession();
      else await window.nativeFolderAutoImport?.stopForWorkspaceChange();
      const result = await raw('shared_workspace_connect', {folder: path});
      await apply(result, true);
    } catch (failure) { error = statusMessage(failure); }
    finally { busy = false; render(error); schedule(); }
  }

  async function disconnect() {
    if (busy || state?.mode !== 'shared' || state.pending || window.tauriOperations?.updating || window.tauriOperations?.saves) return;
    busy = true;
    render();
    const token = state.workspace_token;
    let error = '';
    try {
      if (!await window.confirm('Перейти в личную базу на этом компьютере? Отчёты останутся в своих базах. Автоимпорт из папки в этом сеансе остановится.')) return;
      if (state.available) await window.nativeFolderAutoImport?.stopForWorkspaceChange();
      else window.nativeFolderAutoImport?.forgetWorkspaceSession();
      const result = await raw('shared_workspace_disconnect', {workspaceToken: token});
      await apply(result, true);
    } catch (failure) { error = statusMessage(failure); }
    finally { busy = false; render(error); schedule(); }
  }

  async function discard() {
    if (busy || !state?.pending || window.tauriOperations?.updating) return;
    busy = true;
    render();
    const token = state.workspace_token;
    let error = '';
    try {
      const confirmed = await window.confirm('Отменить неподтверждённые изменения и получить актуальную общую базу?');
      if (!confirmed) return;
      const result = await raw('shared_workspace_discard_pending', {workspaceToken: token});
      await apply(result, true);
    } catch (failure) { error = statusMessage(failure); }
    finally { busy = false; render(error); schedule(); }
  }

  function transferMessage(message) {
    const output=node('personalTransferResult');
    if(output)output.textContent=message;
  }

  function renderTransfer() {
    const panel=node('personalTransferPanel');
    if(!panel)return;
    const shared=state?.mode==='shared';
    panel.hidden=!shared;
    if(!shared){
      node('personalTransferPreview').disabled=true;
      return;
    }
    const available=Boolean(state.available&&!state.pending&&!busy&&!window.tauriOperations?.updating);
    node('personalTransferPreview').disabled=!available;
    const next=node('personalTransferNext'),previous=node('personalTransferPrev');
    const applyButton=node('personalTransferApply'),allButton=node('personalTransferSelectAll');
    const pager=node('personalTransferPager'),summary=node('personalTransferSummary');
    const page=transfer?.preview;
    const hasPage=Boolean(page);
    allButton.hidden=!hasPage;
    applyButton.hidden=!hasPage;
    pager.hidden=!hasPage||page.totalReports<=page.pageSize;
    if(!hasPage){
      applyButton.disabled=true;
      summary.textContent='';
      node('personalTransferList').replaceChildren?.();
      return;
    }
    previous.disabled=!available||page.pageIndex===0;
    next.disabled=!available||!page.hasNext;
    node('personalTransferPage').textContent='Страница '+(page.pageIndex+1);
    const availableRows=page.rows.filter(row=>row.status==='ready');
    summary.textContent='Личная база: '+page.totalReports+' отчётов · новых на этой странице: '+
      availableRows.length+' · выбрано: '+selectedTransferIds.size+' из 25 за один перенос.'+
      (page.organizationWarning?' '+page.organizationWarning:'');
    applyButton.disabled=!available||!selectedTransferIds.size;
    allButton.disabled=!available||!availableRows.length||selectedTransferIds.size>=25;
    const list=node('personalTransferList');
    if(!list?.replaceChildren||typeof document.createElement!=='function')return;
    list.replaceChildren();
    for(const entry of page.rows){
      const label=document.createElement('label');
      label.className='personal-transfer-row'+(entry.status!=='ready'?' personal-transfer-conflict':'');
      const cb=document.createElement('input');
      cb.type='checkbox';cb.disabled=!available||entry.status!=='ready'||(!selectedTransferIds.has(entry.id)&&selectedTransferIds.size>=25);
      cb.checked=selectedTransferIds.has(entry.id);
      cb.addEventListener('change',()=>{
        if(cb.checked){if(selectedTransferIds.size>=25){cb.checked=false;return;}selectedTransferIds.add(entry.id);}
        else selectedTransferIds.delete(entry.id);
        renderTransfer();
      });
      const text=document.createElement('span');
      text.textContent=[entry.reportType,entry.start===entry.end?entry.start:entry.start+' — '+entry.end,
        entry.sourceName||'Импортированный отчёт',
        entry.status==='conflict'?'Конфликт идентификатора или периода — перенос недоступен':
        entry.status==='organization_conflict'?'Подразделение отсутствует в общей базе — перенос недоступен':
        entry.status==='duplicate'?'Уже есть в общей базе — перенос не требуется':'Можно перенести'].join(' · ');
      label.title=entry.id;
      label.append(cb,text);
      list.append(label);
    }
  }

  async function previewTransfer(pageIndex=0,resetSelection=false){
    if(busy||state?.mode!=='shared'||!state.available||state.pending||
       window.tauriOperations?.updating||window.tauriOperations?.saves)return;
    const token=state.workspace_token;
    busy=true;render();transferMessage('');
    try{
      const preview=await raw('shared_workspace_transfer_preview',{workspaceToken:token,page:pageIndex});
      if(token!==state?.workspace_token)throw Error('Общая база изменилась. Повторите проверку.');
      if(!preview||!Array.isArray(preview.rows)||!Number.isSafeInteger(preview.sourceRevision)||
         !Number.isSafeInteger(preview.targetRevision))throw Error('Список отчётов недостоверен.');
      const prior=transfer?.preview;
      if(resetSelection||!prior||prior.sourceRevision!==preview.sourceRevision||prior.targetRevision!==preview.targetRevision)selectedTransferIds.clear();
      transfer={token,preview};
    }catch(error){transfer=null;selectedTransferIds.clear();transferMessage(statusMessage(error));}
    finally{busy=false;render();schedule();}
  }

  async function applyTransfer(){
    if(busy||!transfer||!selectedTransferIds.size||
       state?.mode!=='shared'||!state.available||state.pending)return;
    const {token,preview}=transfer;
    if(token!==state.workspace_token){transferMessage('Общая база изменилась. Повторите проверку.');return;}
    const ids=[...selectedTransferIds];
    if(ids.length>25){transferMessage('За один перенос можно выбрать не более 25 отчётов.');return;}
    // Entries selected on earlier pages were marked ready; Rust rechecks
    // every ID and both revisions before writing.
    if(!await window.confirm('Добавить '+ids.length+' отчётов в общую базу? Личная база не изменится. Перед переносом будет сохранена проверенная копия общей базы.'))return;
    if(token!==state?.workspace_token)return;
    busy=true;render();transferMessage('');
    try{
      const receipt=await raw('shared_workspace_transfer_apply',{
        workspaceToken:token,sourceRevision:preview.sourceRevision,
        targetRevision:preview.targetRevision,selectedIds:ids
      });
      transfer=null;selectedTransferIds.clear();
      await apply(await raw('shared_workspace_status'),true);
      transferMessage('Перенесено отчётов: '+receipt.imported+
        '. Резервная копия общей базы: '+receipt.backupPath+
        '. Личная база сохранена без изменений.');
    }catch(error){
      transfer=null;selectedTransferIds.clear();
      // Network failure may leave an already prepared generation pending.
      transferMessage(statusMessage(error)+' Не повторяйте перенос до проверки состояния публикации общей базы.');
      try{await apply(await raw('shared_workspace_status'),true);}catch{/* no connection: retain warning */}
    }finally{busy=false;render();schedule();}
  }

  async function init() {
    if (ready) return;
    ready = true;
    node('sharedWorkspaceShortcut').addEventListener('click', event =>
      window.referenceDesktop.openPanel('databases', event.currentTarget));
    node('sharedWorkspaceDisconnect').addEventListener('click', disconnect);
    node('sharedWorkspaceConnect').addEventListener('click', connect);
    node('sharedWorkspaceDiscard').addEventListener('click', discard);
    node('sharedWorkspaceRetry').addEventListener('click', () => check(true));
    node('personalTransferPreview')?.addEventListener('click',()=>previewTransfer(0,true));
    node('personalTransferPrev')?.addEventListener('click',()=>previewTransfer(Math.max(0,(transfer?.preview.pageIndex||0)-1)));
    node('personalTransferNext')?.addEventListener('click',()=>previewTransfer((transfer?.preview.pageIndex||0)+1));
    node('personalTransferSelectAll')?.addEventListener('click',()=>{
      for(const row of transfer?.preview.rows||[])if(row.status==='ready'&&selectedTransferIds.size<25)selectedTransferIds.add(row.id);
      renderTransfer();
    });
    node('personalTransferApply')?.addEventListener('click',applyTransfer);
    window.addEventListener('pagehide', () => { ready = false; clearTimeout(timer); }, {once:true});
    try { await initial(); render(); }
    catch (failure) { render(statusMessage(failure)); }
    schedule();
  }
  window.sharedWorkspace = Object.freeze({invoke, init, get state() { return state; }, get busy() { return busy; }});
})();
