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
  let active = 's1', panel = '', analyticsTab = 'chartPanel', panelOpener = null, dialogOpener = null;
  let panelChange = 0;
  let returnPanel = null, settingsReturn = null;
  function button(text, action) {
    const result = document.createElement('button');
    result.type = 'button';
    result.textContent = text;
    result.addEventListener('click', () => action(result));
    return result;
  }

  function appearanceChoices(id, title, captions) {
    const select = byId(id), field = document.createElement('fieldset');
    const legend = document.createElement('legend');
    legend.textContent = title;
    field.className = 'appearance-choices';
    field.append(legend);
    const inputs = [];
    for (const option of select.options) {
      const label = document.createElement('label'), input = document.createElement('input');
      input.type = 'radio';
      input.name = id + 'Choice';
      input.value = option.value;
      const caption = document.createElement('span');
      caption.textContent = captions[option.value] || option.textContent;
      input.addEventListener('change', () => {
        select.value = input.value;
        select.dispatchEvent(new Event('change', {bubbles:true}));
      });
      label.append(input, caption);
      field.append(label);
      inputs.push(input);
    }
    const original = select.closest('label');
    original.hidden = true;
    original.after(field);
    return () => { for (const input of inputs) input.checked = input.value === select.value; };
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
    byId('refSettings').setAttribute('aria-expanded', String(Boolean(settingsReturn)));
    const nav = byId('sharedHeader').querySelectorAll('.main-nav button');
    for (const [index, tab] of nav.entries()) {
      if (!settingsReturn && active === ['s1','s2'][index]) tab.setAttribute('aria-current', 'page');
      else tab.removeAttribute('aria-current');
    }
  }

  function closePanel(restoreFocus = true) {
    if (settingsReturn) {
      const previous = settingsReturn;
      settingsReturn = null;
      delete byId('refRoot').dataset.settingsPage;
      if (restoreFocus) {
        if (previous.panel) openPanel(previous.panel, null, false);
        else closePanel(false);
        returnPanel = previous.returnPanel;
        panelOpener = previous.panelOpener;
        if (previous.opener?.isConnected) previous.opener.focus({preventScroll:true});
        const change = panelChange;
        requestAnimationFrame(() => requestAnimationFrame(() => {
          if (change === panelChange) window.scrollTo(0, previous.scrollY);
        }));
        return;
      }
    }
    if (!panel || (restoreFocus && analyticsTabs.includes(panel))) return;
    if (restoreFocus && returnPanel) {
      const previous = returnPanel;
      returnPanel = null;
      openPanel(previous.key);
      if (previous.opener.isConnected) previous.opener.focus({preventScroll:true});
      return;
    }
    returnPanel = null;
    if (restoreFocus && active === 's1') {
      openPanel(analyticsTab, null, false);
      if (panelOpener?.isConnected) panelOpener.focus({preventScroll:true});
      return;
    }
    window.referenceDetail.hide();
    ++panelChange;
    panel = '';
    byId('toolsPage').hidden = true;
    byId('toolsPage').style.removeProperty('min-block-size');
    delete byId('refNative').dataset.panel;
    disclosures();
    if (restoreFocus && panelOpener?.isConnected) panelOpener.focus({preventScroll: true});
  }

  function openPanel(key, opener, moveFocus = true) {
    if (!Object.hasOwn(sections, key)) return;
    const tools = byId('toolsPage'), nav = tools.querySelector('.tool-tabs');
    const change = ++panelChange;
    if (byId('sheet').open) byId('sheet').close();
    const openingSettings = key === 'storage' && !settingsReturn;
    if (openingSettings) {
      settingsReturn = {panel, returnPanel, panelOpener, opener:opener || byId('refSettings'), scrollY:window.scrollY};
      byId('refRoot').dataset.settingsPage = 'true';
      returnPanel = null;
    } else if (opener && !settingsReturn) {
      returnPanel = panel && tools.contains(opener) ? {key: panel, opener} : null;
      if (!returnPanel) panelOpener = opener;
      if (!window.referenceFilters.atSide()) window.referenceFilters.closeFilters();
    }
    panel = key;
    if (analyticsTabs.includes(key)) analyticsTab = key;
    const back = byId('backBtn');
    back.hidden = analyticsTabs.includes(key);
    back.textContent = settingsReturn && !management.includes(settingsReturn.panel) ? 'Вернуться к анализу' : 'Назад';
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
    const title = byId('toolsTitle');
    if (settingsReturn) {
      title.setAttribute('role', 'heading');
      title.setAttribute('aria-level', '1');
    } else {
      title.removeAttribute('role');
      title.removeAttribute('aria-level');
    }
    title.textContent = settingsReturn ? sections[key] : manage ? 'Управление' : key === 'changes' ? 'История соседних периодов' : key === 'overviewPanel' ? 'Сводка' : 'Подробная аналитика';
    window.unifiedUI.activate(manage || key === 'changes' ? key : 'dashboard');
    window.referenceDetail.show(key);
    if (!manage) window.referenceFilters.syncDetails();
    disclosures();
    const focus = (opener || openingSettings) ? title : (nav.querySelector('[aria-pressed=true]') || title);
    if (moveFocus) focus.focus({preventScroll:true});
    if (openingSettings) {
      window.scrollTo(0, 0);
    } else if (opener && !settingsReturn) {
      // Collapsing filters changes the sticky offset in ResizeObserver. Scroll
      // after that layout rather than retaining the expanded filter height.
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (panel === key && change === panelChange) tools.scrollIntoView({block:'start'});
      }));
    }
  }

  function navigate(id) {
    if (id !== 's1' && id !== 's2') return;
    if (settingsReturn || id === 's2') closePanel(false);
    active = id;
    byId('s1').hidden = id !== 's1';
    byId('s2').hidden = id !== 's2';
    disclosures();
    window.referenceResults.setScreen(active);
    if (id === 's1' && !analyticsTabs.includes(panel)) openPanel(analyticsTab, null, false);
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
    document.addEventListener('keydown', event => {
      if (!(event.ctrlKey || event.metaKey) || event.code !== 'KeyP' || event.shiftKey || event.altKey ||
          byId(active).hidden || byId('sheet').open || document.querySelector('dialog[open]') ||
          (panel && tools.contains(document.activeElement))) return;
      event.preventDefault();
      byId(active).querySelector('[data-print-report]')?.click();
    });
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
    byId('refSettings').addEventListener('click', event => settingsReturn ? closePanel() : openPanel('storage', event.currentTarget));
    byId('referenceAdjacentHistory')?.addEventListener('click', event => openPanel('changes', event.currentTarget));
    byId('refAnalytics').addEventListener('click', event => {
      if (!analyticsTabs.includes(panel)) {
        openPanel(analyticsTab, event.currentTarget);
      } else {
        byId('toolsTitle').focus({preventScroll:true});
        tools.scrollIntoView({block:'start'});
      }
    });
    byId('backBtn').addEventListener('click', () => closePanel());
    root.addEventListener('keydown', event => {
      // Let the browser dismiss only the active select picker.
      if (event.key === 'Escape' && CSS.supports('selector(select:open)') && event.target.closest('select:open')) event.stopPropagation();
    }, true);
    document.addEventListener('keydown', event => {
      if (event.key !== 'Escape' || event.defaultPrevented || document.querySelector('dialog:modal')) return;
      const disclosure = event.target.closest?.('details[open]');
      if (disclosure && root.contains(disclosure) && disclosure.id !== 'advancedAnalytics') {
        if (disclosure.matches('.filter-fold')) window.referenceFilters.closeFilters();
        else disclosure.open = false;
        disclosure.querySelector(':scope > summary')?.focus({preventScroll:true});
        event.preventDefault();
      } else if (panel && !analyticsTabs.includes(panel) && tools.contains(event.target)) {
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

    const syncTheme = appearanceChoices('appearanceTheme','Тема',{system:'Как в системе'});
    const syncSize = appearanceChoices('appearanceSize','Масштаб интерфейса',{
      '80':'80%', '90':'90%', standard:'100%', large:'≈115%', xlarge:'≈130%'
    });
    byId('appearancePalette').closest('label').classList.add('appearance-swatch-label');
    const forcedColors = window.matchMedia('(forced-colors: active)');
    let captionKey;
    const syncAppearance = () => {
      syncTheme();
      syncSize();
      for (const name of ['theme','palette','design','uiSize']) root.dataset[name] = document.documentElement.dataset[name];
      // Read the final color token while the WebView animates its background.
      const fill = getComputedStyle(header.querySelector('.app-header')).getPropertyValue('--window-caption-background').trim();
      const rgb = /^#[0-9a-f]{6}$/i.test(fill) ? fill.slice(1).match(/../g).map(channel => parseInt(channel, 16)) : undefined;
      const background = forcedColors.matches ? null : rgb;
      if (background === undefined) return;
      const dark = background !== null && root.dataset.theme === 'dark';
      const key = JSON.stringify([background, dark]);
      if (key === captionKey) return;
      captionKey = key;
      window.__TAURI__.core.invoke('set_window_caption', {background, dark}).catch(error => {
        if (captionKey === key) captionKey = undefined;
        console.warn('Не удалось применить цвет заголовка окна', error);
      });
    };
    new MutationObserver(syncAppearance).observe(document.documentElement, {attributes:true,attributeFilter:['data-theme','data-palette','data-design','data-ui-size']});
    forcedColors.addEventListener('change', syncAppearance);
    syncAppearance();
    window.referenceDetail.init();
    window.referenceResults.init();
    window.referenceFilters.init();
    navigate('s1');
  }

  window.referenceDesktop = Object.freeze({init, dialog, setDialogApply, openPanel, navigate});
})();
