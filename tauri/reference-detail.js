// Each analytic tab displays its own native SQL controls.
(function () {
  'use strict';
  const $ = id => document.getElementById(id);
  const analyticPanels = ['chartPanel', 'structurePanel', 'comparison', 'usualPanel', 'regularityPanel'];
  let active = null;
  let chartMode = 'line';
  let singlePeriodChart = false;
  let previousChart = null;

  let attached = false;

  function closeNested(event) {
    if (event.key !== 'Escape' || event.defaultPrevented || event.target.closest('dialog')?.id === 'sheet') return;
    const target = event.target;
    const detail = target.closest('details[open]');
    const panel = target.closest('#analyticsDetails, #changeDetails');
    let close;
    if (detail && detail.id !== 'advancedAnalytics' && detail.id !== active && (!panel || panel.contains(detail))) {
      close = () => { detail.open = false; detail.querySelector(':scope > summary')?.focus(); };
    } else if (panel) {
      close = () => $(panel.id === 'analyticsDetails' ? 'closeAnalyticsDetails' : 'closeChangeDetails').click();
    } else if (target.closest('#calendarDetails, #reportCalendar')) {
      const day = $('calendarDetails').querySelector('.calendar-day-details > button');
      const month = $('calendarDetails').querySelector(':scope > button');
      if (day || month) close = () => (day || month).click();
    }
    if (close) {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  }

  function scopeLine(key) {
    if (!analyticPanels.includes(key) && !['overviewPanel','changes'].includes(key)) return;
    const panel = key === 'changes' ? $('changesHeading').parentElement : $(key);
    let line = panel.querySelector(':scope > .detail-date-scope');
    if (!line) {
      line = document.createElement('p');
      line.className = 'detail-date-scope';
      const edit = document.createElement('button');
      edit.type = 'button';
      edit.textContent = 'Изменить период';
      edit.onclick = () => window.referenceFilters.openFilters();
      line.append(document.createElement('span'), document.createElement('span'), edit);
      (panel.querySelector(':scope > summary, :scope > .sectionbar, :scope > h2') || panel.firstElementChild).after(line);
    }
    const selected = window.unifiedUI.scope;
    const analytics = window.unifiedAnalytics;
    const displayed = key === 'chartPanel' ? analytics?.displayedChartScope : key === 'overviewPanel' ? analytics?.displayedOverviewScope : null;
    const scope = displayed || selected;
    const pendingPeriod = scope && selected && (scope.from !== selected.from || scope.to !== selected.to ||
      scope.displayType !== selected.displayType || scope.source !== selected.source ||
      (key === 'overviewPanel' && (scope.baseFrom !== selected.baseFrom || scope.baseTo !== selected.baseTo)));
    const showDate = value => value.split('-').reverse().join('.');
    const interval = (lo, hi) => lo === hi ? showDate(lo) : showDate(lo) + ' — ' + showDate(hi);
    const end = raw => scope.source === 'daily' ? raw : window.unifiedOriginal.periodEnd(raw, scope.displayType);
    const [period, base] = line.children;
    period.textContent = scope ? (pendingPeriod ? 'Показан период ' : 'Общий период ') + interval(scope.from, end(scope.to)) : 'Нет выбранного периода';
    base.hidden = !(scope?.baseFrom && scope.baseTo && ['structurePanel','comparison','regularityPanel','overviewPanel'].includes(key));
    const before = key === 'comparison' && $('comparisonMode').value === 'periods' ? comparisonBase(scope) : null;
    if (key === 'comparison' && !before) base.hidden = true;
    base.textContent = base.hidden ? '' : (before && localBase ? 'Локальная база ' : 'База ') +
      interval(before?.from || scope.baseFrom, end(before?.to || scope.baseTo));
  }

  function chartButtons() {
    const state = window.unifiedUI.state, bar = document.querySelector('#chartPanel [data-chart="bar"]');
    const group = bar.parentElement;
    if (!group.querySelector('[data-reference-line]')) {
      const line = document.createElement('button');
      line.type = 'button';
      line.dataset.referenceLine = '';
      line.textContent = 'Линейный график';
      line.onclick = () => {
        state.chart = chartMode = 'line';
        $('workViewMode').dispatchEvent(new Event('change'));
        chartButtons();
      };
      group.prepend(line);
      for (const button of group.querySelectorAll('[data-chart]')) button.addEventListener('click', () => {
        if (active === 'chartPanel') { chartMode = button.dataset.chart; chartButtons(); }
      });
    }
    const single = singlePeriodChart || window.referenceFilters?.scope?.overviewMode === 'single';
    const line = group.querySelector('[data-reference-line]');
    line.hidden = single;
    line.disabled = single;
    line.title = single ? 'Для линейного графика выберите несколько периодов' : '';
    const displayMode = single && chartMode === 'line' ? 'bar' : chartMode;
    state.chart = displayMode;
    const orientation = $('chartOrientation').closest('label');
    orientation.classList.toggle('ref-hide-orientation', displayMode !== 'bar');
    for (const button of group.querySelectorAll('button')) {
      const selected = (button.dataset.chart || 'line') === displayMode;
      button.setAttribute('aria-pressed', String(selected));
      button.classList.toggle('active', selected);
    }
  }

  let comparisonOrigin = '', localBase = null;
  function comparisonBase(scope) {
    if (!scope?.baseFrom || !scope.baseTo) return null;
    const origin = JSON.stringify([window.sharedWorkspace?.state?.workspace_token, scope.revision,
      scope.source, scope.displayType, scope.from, scope.to]);
    if (comparisonOrigin !== origin) { localBase = null; comparisonOrigin = origin; }
    return localBase || {from:scope.baseFrom, to:scope.baseTo};
  }
  function visibleEnd(scope, date) {
    return scope.source === 'daily' ? date : window.unifiedOriginal.periodEnd(date,scope.displayType);
  }
  function compareDates() {
    const scope = window.unifiedUI.scope, before = comparisonBase(scope);
    if (!$('refComparisonDates')) buildComparisonToolbar();
    const root = $('refComparisonDates');
    root.hidden = !before || $('comparisonMode').value !== 'periods';
    if (root.hidden) return;
    const last = visibleEnd(scope,before.to), length = (a,b) => 1 + Math.round((Date.parse(b)-Date.parse(a))/86400000);
    const days = length(before.from,last), currentDays = length(scope.from,visibleEnd(scope,scope.to));
    root.querySelector('[data-lengths]').textContent = 'Дней в базе: ' + days + ' · в анализируемом периоде: ' + currentDays +
      (days === currentDays ? '' : ' · Интервалы разной длительности.');
    for (const [i,date] of [before.from,last].entries()) {
      const field = root.querySelector('[data-compare-date="'+i+'"]');
      field.textContent = window.referenceCalendar.stamp(date);
      field.setAttribute('aria-label', (i ? 'Базовый период · По · ' : 'Базовый период · С · ') + field.textContent);
    }
  }
  function comparisonTabs() {
    const select = $('comparisonMode');
    let tabs = $('refComparisonModes');
    if (!tabs) {
      tabs = document.createElement('div');
      tabs.id = 'refComparisonModes';
      tabs.className = 'segmented';
      tabs.setAttribute('role','group');
      tabs.setAttribute('aria-label','Что сравниваем');
      for (const option of select.options) {
        const tab = document.createElement('button');
        tab.type = 'button';
        tab.textContent = option.textContent;
        tab.dataset.value = option.value;
        tab.onclick = () => {
          select.value = option.value;
          select.dispatchEvent(new Event('change',{bubbles:true}));
        };
        tabs.append(tab);
      }
      select.closest('label').after(tabs);
      select.addEventListener('change', comparisonTabs);
    }
    for(const tab of tabs.children) {
      const on=tab.dataset.value===select.value;
      tab.setAttribute('aria-pressed',String(on));
      tab.classList.toggle('active',on);
    }
  }
  function buildComparisonToolbar() {
    const root = document.createElement('div'); root.id = 'refComparisonDates';
    const editor = document.createElement('details'), toggle = document.createElement('summary');
    toggle.dataset.editBase = ''; toggle.textContent = 'Изменить базу сравнения'; editor.append(toggle);
    const group = document.createElement('div'); group.className = 'ref-comparison-card';
    for (const [index,label] of ['С','По'].entries()) {
      const field = document.createElement('div'), caption = document.createElement('span');
      caption.textContent = label;
      const button = document.createElement('button');
      button.type = 'button'; button.dataset.compareDate = String(index); button.setAttribute('aria-haspopup','dialog');
      button.onclick = () => {
        const scope = window.unifiedUI.scope, before = comparisonBase(scope);
        if (!before) return;
        const unit = scope.source === 'daily' ? 'day' : scope.displayType;
        const calendar=window.referenceCalendar, weekly=unit==='week';
        const initial=index ? before.to : before.from;
        const weekday=calendar.dateOf(initial).getUTCDay();
        const current=()=>scope===window.unifiedUI.scope;
        const spanFor=(date,kind)=>calendar.period(date,kind,weekday);
        const weeksForDay=weekly?calendar.savedWeeksForDay(window.unifiedUI.api,scope.revision,current):undefined;
        calendar.open({anchor:button, title:'Базовый период · '+label, unit, initial, spanFor, weeksForDay,
          apply(value,kind,weekStart) {
            if (!current()) return;
            const start=weekStart||spanFor(value,kind)[0];
            if(weekly&&calendar.dateOf(start).getUTCDay()!==weekday){
              localBase={from:start,to:start};
              if(index?start>before.from:start<before.to)window.unifiedUI.notice('Открыта неделя с другим днём начала. Для диапазона выберите вторую неделю с тем же днём начала.');
            }else{
              localBase=index ? {from:before.from < start ? before.from : start,to:start}
                : {from:start,to:before.to > start ? before.to : start};
            }
            compareDates();scopeLine('comparison');window.unifiedExtended.openComparison();
          }});
      };
      field.append(caption,button);group.append(field);
    }
    const reset = document.createElement('button');reset.type = 'button';reset.className = 'button secondary';
    reset.textContent = 'Использовать общую базу'; reset.dataset.resetBase = '';
    reset.onclick = () => {
      localBase = null;editor.open = false;compareDates();scopeLine('comparison');toggle.focus();
      window.unifiedExtended.openComparison();
    };
    group.append(reset);editor.append(group);
    const lengths = document.createElement('p');lengths.dataset.lengths = '';lengths.className = 'muted small';
    root.append(editor,lengths);$('periodComparisonFilters').before(root);
    $('comparisonMode').addEventListener('change',() => {compareDates();scopeLine('comparison');});
  }
  function currentComparison() {
    const scope = window.unifiedUI.scope;
    if (active !== 'comparison') return null;
    const before = comparisonBase(scope);
    return before ? {before,after:{from:scope.from,to:scope.to}} : null;
  }

  function init() {
    if (attached) return;
    attached = true;
    $('refNative').addEventListener('keydown', closeNested);
    window.unifiedUI.onScope(() => { if (active) scopeLine(active); if(active==='chartPanel') { singlePeriodChart=false; chartButtons(); } if(active==='comparison')compareDates(); });
  }

  function show(key) {
    if(active && active!==key)window.unifiedExtended.leaveDetail();
    if (key === 'chartPanel' && active !== 'chartPanel') {
      previousChart = window.unifiedUI.state.chart;
      singlePeriodChart = false;
      chartButtons();
    } else if (key !== 'chartPanel' && previousChart !== null) {
      window.unifiedUI.state.chart = previousChart;
      previousChart = null;
    }
    active = key;
    const native = $('refNative');
    native.dataset.panel = key;
    init();
    scopeLine(key);
    if(key==='comparison'){comparisonTabs();compareDates();}
    const analytic = analyticPanels.includes(key);
    const advanced = $('advancedAnalytics');
    advanced.open = analytic;
    $('analyticsDetails').hidden = true;
    $('changeDetails').hidden = true;
    for (const id of ['structurePanel', 'usualPanel', 'regularityPanel']) {
      $(id).open = id === key;
    }
    if (key !== 'comparison') $('contributions').open = false;
    if (analytic) {
      const mode = $('workViewMode');
      mode.value = key === 'structurePanel' ? 'composition' : 'trend';
      mode.dispatchEvent(new Event('change'));
      if (key === 'comparison') window.unifiedExtended.openComparison();
    }
  }

  function hide() {
    window.unifiedExtended.leaveDetail();
    active = null;
    if (previousChart !== null) {
      window.unifiedUI.state.chart = previousChart;
      previousChart = null;
    }
    delete $('refNative').dataset.panel;
    for (const id of ['advancedAnalytics','structurePanel','usualPanel','regularityPanel','contributions']) $(id).open = false;
    $('analyticsDetails').hidden = true;
    $('changeDetails').hidden = true;
    const mode = $('workViewMode');
    mode.value = 'trend';
    $('chartPanel').hidden = false;
    $('structurePanel').hidden = true;
  }

  window.referenceDetail = Object.freeze({init, show, hide, currentComparison,
    setChartPeriodCount(count) {
      if (active !== 'chartPanel') return;
      singlePeriodChart = count <= 1;
      chartButtons();
    },
    refreshChartCaption() { if (active === 'chartPanel') scopeLine(active); },
    refreshOverviewCaption() { if (active === 'overviewPanel') scopeLine(active); },
    get active() { return active; }});
})();
