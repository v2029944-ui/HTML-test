/* The native storage layer verifies SQLite files and activates a staged restore on restart. */
(function () {
  'use strict';
  let initialized = false;
  function init() {
    if (initialized) return;
    initialized = true;
    const ui = window.unifiedUI, api = ui.api, byId = ui.$;
    let review = null, saved = null, pending = null, busy = false, sequence = 0, launchMessage = '';
    const workspace = () => window.sharedWorkspace?.state;
    const personal = () => workspace()?.mode === 'personal';
    const scheduleToggle = byId('scheduledBackupEnabled');
    const hasSchedule = typeof api.nativeScheduledBackup === 'function';
    let scheduleToken, scheduleState = null, scheduleBusy = false, scheduleError = '';
    const current = () => ({
      token: workspace()?.workspace_token, revision: ui.revision, generation: ui.generation
    });
    function same(context) {
      return context && context.token === workspace()?.workspace_token &&
        context.revision === ui.revision && context.generation === ui.generation &&
        Number.isSafeInteger(context.revision) && workspace()?.available && !workspace()?.pending;
    }
    function requireCurrent(context) {
      if (!same(context)) throw Error('Выбранная база или её данные изменились. Повторите проверку.');
    }
    function status(id, message) {
      byId(id).textContent = message;
      if (id === 'databaseBackupStatus') byId(id).hidden = !message;
    }
    function renderSchedule() {
      if (!hasSchedule) return;
      const out = byId('scheduledBackupStatus');
      byId('scheduledBackupControl').hidden = false;
      out.hidden = false;
      if (!scheduleBusy || !scheduleState) scheduleToggle.checked = scheduleState?.enabled === true;
      scheduleToggle.disabled = scheduleBusy || !scheduleState || !same(current());
      const stamp = scheduleState?.lastSuccess;
      const last = Number.isSafeInteger(stamp)
        ? 'Последняя успешная копия: ' + new Date(stamp).toLocaleString('ru-RU') + '. Файл: ' + scheduleState.lastPath
        : 'Ежедневная копия ещё не создавалась.';
      const details = 'Приложение хранит семь последних ежедневных копий этой базы. Копирование выполняется, пока приложение открыто и база выбрана.';
      const error = scheduleError || scheduleState?.error;
      const message = [last, details, error ? 'Не удалось выполнить резервное копирование. ' + error : ''].filter(Boolean).join(' ');
      if (out.textContent !== message) out.textContent = message;
    }
    async function updateSchedule(choice = null) {
      if (typeof api.nativeScheduledBackup !== 'function') return;
      const context = current(), token = context.token;
      if (scheduleToken !== token) {
        scheduleToken = token;
        scheduleState = null;
        scheduleError = '';
      }
      if (scheduleBusy || !same(context)) { renderSchedule(); return; }
      scheduleBusy = true;
      renderSchedule();
      try {
        const result = await api.nativeScheduledBackup(choice);
        if (token !== workspace()?.workspace_token) return;
        scheduleState = result;
        scheduleError = '';
      } catch (error) {
        if (token === workspace()?.workspace_token) scheduleError = error?.message || String(error);
      } finally {
        scheduleBusy = false;
        if (token !== workspace()?.workspace_token) void updateSchedule();
        else renderSchedule();
      }
    }
    function render() {
      const ready = same(current()), local = personal();
      byId('exportBackup').textContent = workspace()?.mode === 'shared' ? 'Сохранить копию общей базы' : 'Сохранить копию личной базы';
      byId('exportBackup').disabled = busy || !ready;
      byId('openBackupFolder').hidden = !saved;
      byId('openBackupFolder').disabled = busy || !ready;
      byId('importBackup').disabled = busy || !ready || !local || pending !== false;
      byId('scheduleDatabaseRestore').hidden = !local || !same(review) || pending !== false;
      byId('scheduleDatabaseRestore').disabled = busy || !ready;
      byId('cancelDatabaseRestore').hidden = !local || pending !== true;
      byId('cancelDatabaseRestore').disabled = busy || !ready;
    }
    function bind(id, action) {
      byId(id).addEventListener('click', async () => {
        if (busy || byId(id).hidden || byId(id).disabled) return;
        const context = current();
        if (!same(context)) return;
        busy = true;
        render();
        try { await action(context); }
        catch (error) {
          const message = window.analizUserError ? window.analizUserError(error) : error?.message || String(error);
          ui.notice(message, true);
          if (same(context) && id === 'importBackup' && !review) status('databaseRestoreStatus', message);
        } finally { busy = false; render(); }
      });
    }
    bind('exportBackup', async context => {
      const isLocal = personal();
      const destination = await api.chooseSqliteBackupPath(isLocal ? 'analiz-personal.sqlite3' : 'analiz-shared.sqlite3');
      if (!destination) return;
      requireCurrent(context);
      await api.nativeBackupSqlite(destination);
      requireCurrent(context);
      saved = destination;
      status('databaseBackupStatus', (isLocal ? 'Проверенная копия личной базы' : 'Проверенная копия общей базы') + ' сохранена в ' + destination);
    });
    bind('openBackupFolder', async () => { if (saved) await api.nativeOpenBackupFolder(saved); });
    bind('importBackup', async context => {
      if (!personal() || pending !== false) return;
      const source = await api.chooseRestoreBackupPath();
      if (!source) return;
      requireCurrent(context);
      review = null;
      status('databaseRestoreStatus', 'Проверка файла…');
      const result = await api.nativeRestorePreview(source);
      requireCurrent(context);
      if (!/^[a-f0-9]{64}$/i.test(result?.verification) || !Number.isSafeInteger(result.reportCount) ||
          result.reportCount < 0 || !Number.isSafeInteger(ui.snapshot?.reports)) throw Error('Не удалось проверить копию.');
      review = {...context, source, verification: result.verification, before: ui.snapshot.reports, after: result.reportCount};
      status('databaseRestoreStatus', 'Копия проверена. Сейчас отчётов в личной базе: ' + review.before +
        '. После восстановления: ' + review.after + '. Файл: ' + source);
    });
    bind('scheduleDatabaseRestore', async context => {
      const selected = review;
      if (!personal() || pending !== false || !same(selected)) return;
      if (!await window.confirm('Заменить личную базу при следующем запуске? Сейчас отчётов: ' +
        selected.before + '. В проверенной копии: ' + selected.after + '. Группы, справочник и настройки также будут взяты из копии.')) return;
      requireCurrent(context);
      const result = await api.nativeRestoreSchedule(selected.source, selected.verification, context.revision);
      requireCurrent(context);
      if (result?.restartRequired !== true) throw Error('Приложению не удалось подтвердить подготовку восстановления.');
      review = null;
      pending = true;
      status('databaseRestoreStatus', 'Восстановление запланировано. Закройте приложение и запустите снова. Изменения, внесённые до перезапуска, останутся в страховочной копии.');
    });
    bind('cancelDatabaseRestore', async context => {
      if (!personal() || pending !== true) return;
      const cancelled = await api.nativeRestoreCancel();
      requireCurrent(context);
      pending = false;
      status('databaseRestoreStatus', cancelled ? 'Восстановление отменено. Личная база сохранена.' : 'Запланированного восстановления нет.');
    });
    let lastToken;
    async function refresh() {
      const id = ++sequence, context = current();
      if (scheduleToken !== context.token || !scheduleState) void updateSchedule();
      else renderSchedule();
      if (context.token !== lastToken) {
        lastToken = context.token;
        saved = null;
        status('databaseBackupStatus', '');
      }
      review = null;
      pending = null;
      status('databaseRestoreStatus', personal() ? launchMessage : 'Для восстановления перейдите в личную базу.');
      render();
      if (!same(context) || !personal()) return;
      try {
        const [scheduled, launch] = await Promise.all([api.nativeRestorePending(), api.nativeRestoreLaunchInfo()]);
        if (id !== sequence || !same(context)) return;
        pending = scheduled === true;
        launchMessage = launch?.message || '';
        const reminder = 'Восстановление ожидает перезапуска приложения. Изменения, внесённые до перезапуска, останутся в страховочной копии.';
        status('databaseRestoreStatus', pending ? (launch?.restored === false ? launchMessage + ' ' : '') + reminder : launchMessage);
      } catch (error) {
        if (id === sequence && same(context)) status('databaseRestoreStatus', 'Не удалось проверить состояние восстановления. ' + (error?.message || String(error)));
      }
      render();
    }
    ui.onSnapshot(refresh);
    if (scheduleToggle) scheduleToggle.addEventListener('change', () => updateSchedule(scheduleToggle.checked));
    if (hasSchedule) window.setInterval?.(() => {
      if (scheduleState?.enabled || !scheduleState) void updateSchedule();
    }, 60_000);
    render();
    renderSchedule();
  }
  window.databaseBackups = Object.freeze({init});
})();
