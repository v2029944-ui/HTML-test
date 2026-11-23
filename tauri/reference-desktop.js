/* Navigation between the report screens and desktop SQL tools. */
(function () {
  'use strict';
  const byId = id => document.getElementById(id);
  const sections = {
    chartPanel: 'Динамика', structurePanel: 'Состав работ', comparison: 'Сравнение',
    usualPanel: 'Отклонения', regularityPanel: 'Частота',
    overviewPanel: 'Итоги и рейтинги', changes: 'Соседние периоды',
    upload: 'Отчёты', groups: 'Группы работ', databases: 'Базы данных', storage: 'Настройки'
  };
  const management = ['upload', 'groups', 'databases', 'storage'];
  // Keep the legacy route keys for transitions, but do not duplicate the
  // main summary and adjacent-history tools among the five detail tabs.
  const analyticsTabs = ['chartPanel','structurePanel','comparison','usualPanel','regularityPanel'];
  let active = 's1', panel = '', panelOpener = null, dialogOpener = null;
  let panelChange = 0;
  let returnPanel = null;
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
    sheet.querySelector('form .filter-apply')?.remove();
    sheet.setAttribute('aria-labelledby', heading.id);
    if (!sheet.open) {
      dialogOpener = anchor || document.activeElement;
      sheet.showModal();
    }
    return body;
  }

  function setDialogApply(action) {
    action.classList.add('filter-apply');
    byId('sheet').querySelector('form').append(action);
  }

  function disclosures() {
    byId('refAnalytics').setAttribute('aria-expanded', String(Boolean(panel) && !management.includes(panel)));
    byId('refSettings').setAttribute('aria-expanded', String(panel === 'storage'));
  }

  function closePanel(restoreFocus = true) {
    if (!panel) return;
    if (restoreFocus && returnPanel) {
      const previous = returnPanel;
      returnPanel = null;
      openPanel(previous.key);
      if (previous.opener.isConnected) previous.opener.focus({preventScroll:true});
      return;
    }
    returnPanel = null;
    window.referenceDetail.hide();
    panel = '';
    byId('toolsPage').hidden = true;
    byId('toolsPage').style.removeProperty('min-block-size');
    delete byId('refNative').dataset.panel;
    disclosures();
    if (restoreFocus && panelOpener?.isConnected) panelOpener.focus({preventScroll: true});
  }

  function openPanel(key, opener) {
    if (!Object.hasOwn(sections, key)) return;
    const tools = byId('toolsPage'), nav = tools.querySelector('.tool-tabs');
    const change = ++panelChange;
    if (byId('sheet').open) byId('sheet').close();
    if (opener) {
      returnPanel = panel && tools.contains(opener) ? {key: panel, opener} : null;
      if (!returnPanel) panelOpener = opener;
      if (!window.referenceFilters.atSide()) window.referenceFilters.closeFilters();
    }
    panel = key;
    tools.hidden = false;
    byId('refNative').dataset.panel = key;
    const manage = management.includes(key);
    const allowed = manage ? management : analyticsTabs;
    if (nav.firstElementChild?.dataset.section !== allowed[0]) {
      nav.replaceChildren(...allowed.map(item => {
        const tab = button(sections[item], () => openPanel(item));
        tab.dataset.section = item;
        return tab;
      }));
    }
    for (const tab of nav.children) tab.setAttribute('aria-pressed', String(tab.dataset.section === key));
    nav.hidden = key === 'changes' || key === 'overviewPanel';
    byId('toolsTitle').textContent = manage ? 'Управление' : key === 'changes' ? 'История соседних периодов' : key === 'overviewPanel' ? 'Сводка' : 'Подробная аналитика';
    window.unifiedUI.activate(manage || key === 'changes' ? key : 'dashboard');
    window.referenceDetail.show(key);
    if (!manage) window.referenceFilters.syncDetails();
    disclosures();
    const focus = opener ? byId('toolsTitle') : (nav.querySelector('[aria-pressed=true]') || byId('toolsTitle'));
    focus.focus({preventScroll:true});
    if (opener) {
      // Collapsing filters changes the sticky offset in ResizeObserver. Scroll
      // after that layout rather than retaining the expanded filter height.
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (panel === key && change === panelChange) tools.scrollIntoView({block:'start'});
      }));
    }
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

  function init() {
    const root = byId('refRoot'), tools = byId('toolsPage');
    // A short tab, or an empty calendar while its SQL request is pending,
    // otherwise shortens the document and clamps the user's scroll position.
    tools.addEventListener('click', event => {
      if (!event.target.closest('.tool-tabs button, #unifiedUploadNav button, #calendarPrev, #calendarNext, #referenceReportYear, #reportCalendar button, #calendarDetails button')) return;
      const rect = tools.getBoundingClientRect();
      const scale = rect.height / tools.offsetHeight;
      if (scale > 0) tools.style.minBlockSize = Math.ceil(Math.max(0, innerHeight - rect.top) / scale) + 'px';
    }, true);
    const native = byId('refNative');
    let plotWidth = 0;
    new ResizeObserver(([entry]) => {
      const width = entry.contentRect.width;
      if (width > 0 && width !== plotWidth) window.unifiedOriginal.redrawCharts(native);
      plotWidth = width;
    }).observe(native);
    const sheet = byId('sheet'), header = byId('sharedHeader');
    // Visibility, native modality and focus are owned by HTMLDialogElement.
    sheet.addEventListener('close', () => {
      if (!sheet.open && dialogOpener?.isConnected && dialogOpener.getClientRects().length) dialogOpener.focus({preventScroll:true});
    });
    header.querySelectorAll('.main-nav button').forEach((tab, i) =>
      tab.addEventListener('click', () => navigate(i ? 's2' : 's1')));
    byId('refImport').addEventListener('click', event => openPanel('upload', event.currentTarget));
    byId('refSettings').addEventListener('click', event => panel === 'storage' ? closePanel() : openPanel('storage', event.currentTarget));
    byId('referenceAdjacentHistory')?.addEventListener('click', event => openPanel('changes', event.currentTarget));
    byId('refAnalytics').addEventListener('click', event =>
      panel && !management.includes(panel) ? closePanel() : openPanel('chartPanel', event.currentTarget));
    byId('backBtn').addEventListener('click', () => closePanel());
    root.addEventListener('keydown', event => {
      // Let the browser dismiss only the active select picker.
      if (event.key === 'Escape' && CSS.supports('selector(select:open)') && event.target.closest('select:open')) event.stopPropagation();
    }, true);
    document.addEventListener('keydown', event => {
      if (event.key !== 'Escape' || event.defaultPrevented || document.querySelector('dialog:modal')) return;
      const disclosure = event.target.closest?.('details[open]');
      if (disclosure && root.contains(disclosure) && !disclosure.matches('#advancedAnalytics,#structurePanel,#usualPanel,#regularityPanel')) {
        if (disclosure.matches('.filter-fold')) window.referenceFilters.closeFilters();
        else disclosure.open = false;
        disclosure.querySelector(':scope > summary')?.focus({preventScroll:true});
        event.preventDefault();
      } else if (panel && tools.contains(event.target)) {
        closePanel();
        event.preventDefault();
      }
    });

    const notices=document.createElement('div');
    notices.className='desktop-notices';
    notices.append(byId('flash'),byId('appActivity'));
    header.after(notices);
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

  window.referenceDesktop = Object.freeze({init, dialog, setDialogApply, openPanel, navigate});
})();
