'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

async function fixture() {
  const elements = new Map();
  const lists = new Map();
  function element() {
    return {options: [], style: {setProperty() {}}, classList: {toggle() {}},
      append(child) { this.options.push(child); }, replaceChildren() { this.options = []; },
      setAttribute() {}, removeAttribute() {}, getBoundingClientRect() { return {height: 40}; },
    };
  }
  function one(selector) {
    if (!elements.has(selector)) elements.set(selector, element());
    return elements.get(selector);
  }
  const panel = one('#reportShell');
  panel.querySelector = selector => selector === '#referenceSource' ? one('.filter-periods').options[0].options[0] : one(selector);
  panel.querySelectorAll = selector => {
    const n = {'[data-filter="type"] button':4, '[data-filter="mode"] button':2,
      '[data-filter=type] button':4, '[data-filter=mode] button':2, '[data-date]':2}[selector] || 0;
    if (!lists.has(selector)) lists.set(selector, Array.from({length:n}, element));
    return lists.get(selector);
  };
  const document = {createElement: element, getElementById: id => id === 'refNative' ? null : one('#'+id)};
  const calls = [], seen = [];
  const weeks = ['2026-11-04','2026-11-06','2026-11-16'];
  const window = {referenceResults: {beginScopeChange() {},failScopeChange() {},refresh(scope) {seen.push(scope);}}};
  const ui = window.unifiedUI = {revision:1, notice() {}, onSnapshot(fn) {this.configure = fn;}, api:{
    async unifiedNameOptions() { calls.push('catalog');return {total_rows:1}; },
    async unifiedPeriodOptions(revision,kind,group,after,limit,day) {
      calls.push([kind,day]);
      if (kind === 'week' && ui.fail) throw Error('fixture');
      if (day) return {rows:weeks.filter(start=>start<=window.referenceCalendar.period(day,'month')[1] && day<=window.referenceCalendar.day(start,6))
        .map(start=>({start,end:window.referenceCalendar.day(start,6)}))};
      return {max: {day:'2026-11-29',week:'2026-11-16',month:'2025-02-01'}[kind]};
    }
  }};
  const ctx={window,document,matchMedia:()=>({matches:false}),ResizeObserver:class {observe(){}}};
  const run = name => vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../'+name+'.js'),'utf8'),ctx);
  run('reference-calendar');
  let draft;
  window.referenceCalendar={...window.referenceCalendar,open:options=>{draft=options;}};
  run('reference-filters');
  const controls = window.referenceFilters;
  const date = panel.querySelectorAll('[data-date]')[0];date.dataset={date:'start'};
  controls.init();await ui.configure({revision:1,org_names:['A']});
  const source = async val=>{const n=panel.querySelector('#referenceSource');n.value=val;n.onchange();await new Promise(r=>setImmediate(r));};
  return {controls, calls, ui, source, seen, one, panel, draft:()=>draft, open:()=>{date.onclick();return draft;}};
}

test('an imported week and its comparison base can use different saved weekdays', async () => {
  const {controls,source,open,one,panel,draft} = await fixture();
  panel.querySelectorAll('[data-filter=mode] button')[0].onclick();
  await source('imported');
  const d=open(), candidates=await d.weeksForDay('2026-11-07');
  assert.deepEqual(candidates.map(x=>x.start),['2026-11-04','2026-11-06']);
  assert.equal((await d.weeksForDay('2026-11-16'))[0].start,'2026-11-16');
  d.apply('2026-11-07','week','2026-11-04');
  assert.equal(controls.scope.from,'2026-11-04');
  assert.equal(controls.scope.displayTo,'2026-11-10');
  const compare=one('[data-compare]');compare.value='custom';compare.onchange();
  draft().apply('2026-11-07','week','2026-11-06');
  assert.equal(controls.scope.baseFrom,'2026-11-06');
  assert.equal(controls.scope.baseTo,'2026-11-06');
  assert.equal(controls.scope.displayBaseTo,'2026-11-12');
});

test('year-ago monthly scope and CSV keys describe the whole leap February', async () => {
  const {controls,panel,source,one}=await fixture();
  panel.querySelectorAll('[data-filter=type] button')[2].onclick();
  await source('imported');
  const compare=one('[data-compare]');compare.value='year';compare.onchange();
  assert.equal(controls.scope.displayBaseTo,'2024-02-29');
  assert.equal(controls.scope.baseTo,'2024-02-01');
});

test('a failed bounds read retries before publishing; catalog reads stay tied to the current snapshot', async () => {
  const {controls,calls,source,ui}=await fixture();
  ui.fail=true;await source('imported');
  assert.equal(controls.scope,null);
  await controls.setMetric('count');
  assert.equal(controls.scope,null);
  ui.fail=false;await controls.setMetric('amount');
  assert.equal(controls.scope.from,'2026-11-16');
  assert.equal(calls.filter(v=>v==='catalog').length,1);
  await ui.configure({revision:1,org_names:['B']});
  assert.equal(calls.filter(v=>v==='catalog').length,2);
});
