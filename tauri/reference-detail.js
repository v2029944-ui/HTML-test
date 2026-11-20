// Each analytic tab displays its own native SQL controls.
(function () {
  'use strict';
  const $ = id => document.getElementById(id);
  const analyticPanels = ['chartPanel', 'structurePanel', 'comparison', 'usualPanel', 'regularityPanel'];
  let active = null;
  let chartMode = 'line';
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
    if (!analyticPanels.includes(key)) return;
    const panel = $(key);
    let line = panel.querySelector(':scope > .detail-date-scope');
    if (!line) {
      line = document.createElement('p');
      line.className = 'detail-date-scope';
      const edit = document.createElement('button');
      edit.type = 'button';
      edit.textContent = 'Изменить выше';
      edit.onclick = () => {
        const fold = document.querySelector('#reportShell .filter-fold');
        fold.open = true;
        const toggle = fold.querySelector('summary');
        toggle.focus({preventScroll:true});
        fold.scrollIntoView({block:'start'});
      };
      line.append(document.createElement('span'), document.createElement('span'), edit);
      panel.querySelector(':scope > summary, :scope > .sectionbar')?.after(line);
    }
    const scope = window.unifiedUI.scope;
    const showDate = value => value.split('-').reverse().join('.');
    const interval = (lo, hi) => lo === hi ? showDate(lo) : showDate(lo) + ' — ' + showDate(hi);
    const end = raw => scope.source === 'daily' ? raw : window.unifiedOriginal.periodEnd(raw, scope.displayType);
    const [period, base] = line.children;
    period.textContent = scope ? 'Общий период ' + interval(scope.from, end(scope.to)) : 'Нет выбранного периода';
    base.hidden = !(scope?.baseFrom && scope.baseTo && ['structurePanel','comparison','regularityPanel'].includes(key));
    base.textContent = base.hidden ? '' : 'База ' + interval(scope.baseFrom, end(scope.baseTo));
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
    state.chart = chartMode;
    const orientation = $('chartOrientation').closest('label');
    orientation.classList.toggle('ref-hide-orientation', chartMode !== 'bar');
    for (const button of group.querySelectorAll('button')) {
      const selected = (button.dataset.chart || 'line') === chartMode;
      button.setAttribute('aria-pressed', String(selected));
      button.classList.toggle('active', selected);
    }
  }

  let comparisonOrigin = '', comparisonWindow = '', dates = null, dateMode = 'shared';
  const dateCaptions = ['Базовый период · С','Базовый период · По','Анализируемый период · С','Анализируемый период · По'];
  function previousEqual(lo,hi) {
    return [window.referenceCalendar.day(lo, -(1 + Math.round((Date.parse(hi)-Date.parse(lo))/86400000))),
      window.referenceCalendar.day(lo,-1)];
  }
  function priorYear(stamp) {
    const c=window.referenceCalendar,d=c.dateOf(stamp),year=d.getUTCFullYear()-1,month=d.getUTCMonth();
    return c.dateAt(year,month,Math.min(d.getUTCDate(),c.dateOf(c.dateAt(year,month+1,0)).getUTCDate()));
  }
  function compareDates() {
    const scope=window.unifiedUI.scope,toolbar=$('refComparisonDates');
    if(!scope || scope.source!=='daily' || !scope.baseFrom || !scope.baseTo) {
      if(toolbar) toolbar.hidden=true;
      $('periodComparisonFilters').classList.remove('ref-local-date-fields');
      // A pending SQL refresh can temporarily clear scope before publishing the same interval.
      if(scope){dates=null;comparisonOrigin='';}
      return;
    }
    const range=[scope.revision,scope.from,scope.to].join('/');
    const signature=[range,scope.baseFrom,scope.baseTo].join('/');
    if(signature!==comparisonOrigin) {
      if (!comparisonOrigin || comparisonWindow!==range || dateMode!=='manual') {
        dates=[scope.baseFrom,scope.baseTo,scope.from,scope.to];dateMode='shared';
      }
      comparisonOrigin=signature;comparisonWindow=range;
    }
    if(!toolbar) buildComparisonToolbar();
    const root=$('refComparisonDates'),periods=$('comparisonMode').value==='periods';
    root.hidden=!periods;
    $('periodComparisonFilters').classList.toggle('ref-local-date-fields',periods);
    if(!periods)return;
    for (const [i,button] of [...root.querySelectorAll('[data-compare-date]')].entries()) {
      button.textContent=window.referenceCalendar.stamp(dates[i]);
      button.setAttribute('aria-label',dateCaptions[i]+' · '+button.textContent);
      button.disabled=i<2&&dateMode!=='manual';
    }
    for(const button of root.querySelectorAll('[data-preset]'))button.setAttribute('aria-pressed',String(button.dataset.preset===dateMode));
    root.querySelector('[data-edit-base]').textContent=dateMode==='manual'?'Автоподбор':'Изменить';
    root.querySelector('[data-reset-base]').hidden=dateMode==='shared';
    const days=(lo,hi)=>1+Math.round((Date.parse(hi)-Date.parse(lo))/86400000);
    const lengths=[days(dates[0],dates[1]),days(dates[2],dates[3])];
    root.querySelector('[data-lengths]').textContent='Дней в базе: '+lengths[0]+' · в анализируемом периоде: '+lengths[1]+
      (lengths[0]===lengths[1]?'':' · Интервалы разной длительности.');
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
    const root=document.createElement('div');root.id='refComparisonDates';
    const presets=document.createElement('div');presets.className='ref-cmp-presets';
    const c=window.referenceCalendar;
    const button=(label,run)=>{const b=document.createElement('button');b.type='button';b.className='button secondary';b.textContent=label;b.onclick=()=>run(b);return b;};
    const commit=(next,mode)=>{
      if(!next.every(c.valid)){window.unifiedUI.notice('Выбранный интервал выходит за допустимые календарные границы.',true);return;}
      dates=next;dateMode=mode;compareDates();window.unifiedExtended.openComparison();
    };
    for(const [mode,label] of [['week','Прошлая неделя'],['month','Прошлый месяц'],['year','Год назад']]) {
      const b=button(label,()=>{
        let [,,lo,hi]=dates,base;
        if(mode==='year')base=[priorYear(lo),priorYear(hi)];
        else{
          [lo,hi]=c.period(hi,mode);
          base=mode==='month'?c.period(c.day(lo,-1),'month'):previousEqual(lo,hi);
        }
        commit([...base,lo,hi],mode);
      });
      b.dataset.preset=mode;presets.append(b);
    }
    root.append(presets);
    for(const [i,title] of ['Базовый период','Анализируемый период'].entries()){
      const group=document.createElement('fieldset');group.className='ref-comparison-card';
      const legend=document.createElement('legend');legend.textContent=title;group.append(legend);
      for(let j=0;j<2;j++){
        const index=2*i+j,field=document.createElement('div'),caption=document.createElement('span');
        caption.textContent=j?'По':'С';
        const b=button('',anchor=>c.open({anchor,title:dateCaptions[index],initial:dates[index],unit:'day',apply(value){
          const next=dates.slice();next[index]=value;
          if(next[2*i]>next[2*i+1])next[2*i+(1-j)]=value;
          let mode=dateMode;
          if(i===1&&mode!=='manual'){
            [next[0],next[1]]=mode==='year'?[priorYear(next[2]),priorYear(next[3])]:previousEqual(next[2],next[3]);
            if(mode!=='year')mode='auto';
          }
          commit(next,mode);
        }}));
        b.dataset.compareDate=String(index);b.setAttribute('aria-haspopup','dialog');field.append(caption,b);group.append(field);
      }
      if(i===0){
        const edit=button('Изменить',()=>{
          if(dateMode==='manual')commit([...previousEqual(dates[2],dates[3]),...dates.slice(2)],'auto');
          else {dateMode='manual';compareDates();group.querySelector('[data-compare-date]').focus();}
        });
        edit.dataset.editBase='';group.append(edit);
      }
      root.append(group);
    }
    const lengths=document.createElement('p');lengths.dataset.lengths='';lengths.className='muted small';
    const reset=button('Использовать общий период',()=>{comparisonOrigin='';compareDates();window.unifiedExtended.openComparison();});
    reset.dataset.resetBase='';root.append(lengths,reset);$('periodComparisonFilters').before(root);
    $('comparisonMode').addEventListener('change',compareDates);
  }
  function currentComparison() {
    if(active!=='comparison'||window.unifiedUI.scope?.source!=='daily')return null;
    compareDates();
    if(!dates)return null;
    return {before:{from:dates[0],to:dates[1]},after:{from:dates[2],to:dates[3]}};
  }

  function init() {
    if (attached) return;
    attached = true;
    $('refNative').addEventListener('keydown', closeNested);
    window.unifiedUI.onScope(() => { if (active) scopeLine(active); if(active==='comparison')compareDates(); });
  }

  function show(key) {
    if(active && active!==key)window.unifiedExtended.leaveDetail();
    if (key === 'chartPanel' && active !== 'chartPanel') {
      previousChart = window.unifiedUI.state.chart;
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

  window.referenceDetail = Object.freeze({init, show, hide, currentComparison, get active() { return active; }});
})();
