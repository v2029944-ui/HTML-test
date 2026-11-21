/* The active workspace is selected by Rust. Every data request captures its identity. */
(function () {
  'use strict';
  const raw = (name, args = {}) => window.__TAURI__.core.invoke(name, args);
  let state = null, bootstrap = null, ready = false, timer = null, busy = false;
  const statusMessage = error => window.analizUserError ? window.analizUserError(error) : error?.message || String(error);
  const personal = 'Личная база на этом компьютере.';

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
    } else label = state ? personal : 'Не удалось определить активную базу.';
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
  }

  function schedule() {
    clearTimeout(timer);
    if (ready && (!state || state.mode === 'shared')) timer = setTimeout(() => check(false), 5000);
  }

  async function apply(result, force = false) {
    const previous = state;
    state = accepted(result);
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
    if (busy || window.tauriOperations?.updating) return;
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

  async function init() {
    if (ready) return;
    ready = true;
    node('sharedWorkspaceConnect').addEventListener('click', connect);
    node('sharedWorkspaceDiscard').addEventListener('click', discard);
    node('sharedWorkspaceRetry').addEventListener('click', () => check(true));
    window.addEventListener('pagehide', () => { ready = false; clearTimeout(timer); }, {once:true});
    try { await initial(); render(); }
    catch (failure) { render(statusMessage(failure)); }
    schedule();
  }
  window.sharedWorkspace = Object.freeze({invoke, init, get state() { return state; }, get busy() { return busy; }});
})();
