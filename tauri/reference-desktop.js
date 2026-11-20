/* The detail panel and the full workspace share one set of native SQL controls. */
(function () {
  'use strict';
  const byId = id => document.getElementById(id);
  const sections = {
    chartPanel: 'Динамика', structurePanel: 'Состав работ', comparison: 'Сравнение',
    usualPanel: 'Отклонения', regularityPanel: 'Частота',
    upload: 'Отчёты', groups: 'Группы работ', storage: 'Настройки'
  };
  const management = ['upload', 'groups', 'storage'];
  let active = 's1', panel = '', panelOpener = null, dialogOpener = null, workspaceOpener = null;
  function button(text, action) {
    const result = document.createElement('button');
    result.type = 'button';
    result.textContent = text;
    result.addEventListener('click', () => action(result));
    return result;
  }

  function dialog(title, anchor) {
    const sheet = byId('sheet'), body = byId('dialogBody');
    const heading = document.createElement('h2');
    heading.id = 'refDialogTitle';
    heading.textContent = title;
    body.replaceChildren(heading);
    sheet.setAttribute('aria-labelledby', heading.id);
    if (!sheet.open) {
      dialogOpener = anchor || document.activeElement;
      sheet.showModal();
    }
    return body;
  }

  function disclosures() {
    byId('refAnalytics').setAttribute('aria-expanded', String(Boolean(panel) && !management.includes(panel)));
    byId('refSettings').setAttribute('aria-expanded', String(panel === 'storage'));
  }

  function closePanel(restoreFocus = true) {
    if (!panel) return;
    window.referenceDetail.hide();
    panel = '';
    byId('toolsPage').hidden = true;
    delete byId('refNative').dataset.panel;
    disclosures();
    if (restoreFocus && panelOpener?.isConnected) panelOpener.focus({preventScroll: true});
  }

  function openPanel(key, opener) {
    if (!Object.hasOwn(sections, key)) return;
    const tools = byId('toolsPage'), nav = tools.querySelector('.tool-tabs');
    if (byId('sheet').open) byId('sheet').close();
    if (opener) {
      panelOpener = opener;
      byId('reportShell').querySelector('.filter-fold').open = false;
    }
    panel = key;
    tools.hidden = false;
    byId('refNative').dataset.panel = key;
    const manage = management.includes(key);
    const allowed = Object.keys(sections).filter(item => management.includes(item) === manage);
    nav.replaceChildren(...allowed.map(item => {
      const tab = button(sections[item], () => openPanel(item));
      tab.setAttribute('aria-pressed', String(item === key));
      return tab;
    }));
    byId('toolsTitle').textContent = manage ? 'Управление' : 'Подробная аналитика';
    window.unifiedUI.activate(manage ? key : 'dashboard');
    window.referenceDetail.show(key);
    if (!manage) window.referenceFilters.syncDetails();
    disclosures();
    const focus = opener ? byId('toolsTitle') : nav.querySelector('[aria-pressed=true]');
    focus.focus({preventScroll:true});
    if (opener) tools.scrollIntoView({block:'start'});
  }

  function navigate(id) {
    if (id !== 's1' && id !== 's2') return;
    closePanel(false);
    active = id;
    byId('s1').hidden = id !== 's1';
    byId('s2').hidden = id !== 's2';
    const nav = byId('sharedHeader').querySelectorAll('.main-nav button');
    for (const [index, tab] of nav.entries()) {
      if (id === ['s1','s2'][index]) tab.setAttribute('aria-current', 'page');
      else tab.removeAttribute('aria-current');
    }
    window.referenceResults.setScreen(active);
    window.scrollTo(0, 0);
  }

  function openWorkspace(anchor) {
    workspaceOpener = anchor;
    const native = byId('refNative'), workspace = byId('refWorkspaceDialog');
    closePanel(false);
    delete native.dataset.panel;
    workspace.append(native);
    native.querySelector('main.container').prepend(byId('flash'));
    window.unifiedUI.activate('dashboard');
    workspace.showModal();
    byId('refWorkspaceClose').focus();
  }

  function menu(anchor) {
    const body = dialog('Управление', anchor);
    const list = document.createElement('div');
    list.className = 'navigation';
    for (const key of management) list.append(button(sections[key], () => openPanel(key, anchor)));
    list.append(button('Исходный макет', openWorkspace));
    body.append(list);
    list.firstElementChild.focus();
  }

  function init() {
    const root = byId('refRoot'), tools = byId('toolsPage'), workspace = byId('refWorkspaceDialog');
    const sheet = byId('sheet'), header = byId('sharedHeader');
    // Visibility, native modality and focus are owned by HTMLDialogElement.
    sheet.addEventListener('close', () => {
      if (!sheet.open && !panel && dialogOpener?.isConnected) dialogOpener.focus({preventScroll:true});
    });
    byId('refWorkspaceClose').addEventListener('click', () => workspace.close());
    workspace.addEventListener('close', () => {
      tools.append(byId('refNative'));
      header.after(byId('flash'));
      workspaceOpener?.focus({preventScroll:true});
    });
    header.querySelectorAll('.main-nav button').forEach((tab, i) =>
      tab.addEventListener('click', () => navigate(i ? 's2' : 's1')));
    byId('refData').addEventListener('click', event => menu(event.currentTarget));
    byId('refImport').addEventListener('click', event => openPanel('upload', event.currentTarget));
    byId('refSettings').addEventListener('click', event => panel === 'storage' ? closePanel() : openPanel('storage', event.currentTarget));
    byId('refAnalytics').addEventListener('click', event =>
      panel && !management.includes(panel) ? closePanel() : openPanel('chartPanel', event.currentTarget));
    byId('backBtn').addEventListener('click', () => closePanel());
    document.addEventListener('keydown', event => {
      if (event.key !== 'Escape' || event.defaultPrevented || document.querySelector('dialog:modal')) return;
      const disclosure = event.target.closest?.('details[open]');
      if (disclosure && root.contains(disclosure) && !disclosure.matches('#advancedAnalytics,#structurePanel,#usualPanel,#regularityPanel')) {
        disclosure.open = false;
        disclosure.querySelector(':scope > summary')?.focus();
        event.preventDefault();
      } else if (panel && tools.contains(event.target)) {
        closePanel();
        event.preventDefault();
      }
    });

    header.after(byId('flash'));
    const currentStatus = byId('unifiedIndexStatus'), status = header.querySelector('.source-state');
    function statusChanged() {
      status.textContent = currentStatus.firstElementChild.textContent;
      const failed = !window.unifiedUI.snapshot && !currentStatus.lastElementChild.hidden;
      statusAction.hidden = currentStatus.children[1].hidden && !failed;
      statusAction.textContent = failed ? 'Повторить' : 'Остановить';
      statusAction.onclick = () => currentStatus.children[failed ? 2 : 1].click();
    }
    const statusAction = document.createElement('button');
    statusAction.type = 'button';
    statusAction.className = 'btn';
    status.after(statusAction);
    new MutationObserver(statusChanged).observe(currentStatus, {childList:true,subtree:true,attributes:true});
    statusChanged();

    const syncAppearance = () => {
      for (const name of ['theme','palette','design','uiSize']) root.dataset[name] = document.documentElement.dataset[name];
    };
    new MutationObserver(syncAppearance).observe(document.documentElement, {attributes:true,attributeFilter:['data-theme','data-palette','data-design','data-ui-size']});
    syncAppearance();
    window.referenceDetail.init();
    window.referenceResults.init();
    window.referenceFilters.init();
    navigate('s1');
  }

  window.referenceDesktop = Object.freeze({init, dialog, openPanel, navigate});
})();
