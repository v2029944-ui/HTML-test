'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

async function fixture(storage = new Map(), width = 1440) {
  const elements = new Map();
  const lists = new Map();
  function element() {
    return {value:'',dataset:{}, options: [], style: {setProperty() {},removeProperty() {}}, classList: {toggle() {}},
      append(...children) { this.options.push(...children); }, replaceChildren(...children) { this.options = children; },
      isConnected: true, focus() {}, closest() {return {close(){}};},
      addEventListener() {}, hasAttribute() {return false;}, setAttribute() {}, removeAttribute() {}, getBoundingClientRect() { return {height: 40}; },
    };
  }
  function one(selector) {
    if (!elements.has(selector)) elements.set(selector, element());
    return elements.get(selector);
  }
  const panel = one('#reportShell');
  one('[data-compare]').parentElement={before:label=>one('.filter-periods').append(label)};
  panel.querySelector = selector => selector === '#referenceSource' ? one('.filter-periods').options[0].options[0] : one(selector);
  panel.querySelectorAll = selector => {
    const n = {'[data-date]':2, '[data-shift]':2}[selector] || 0;
    if (!lists.has(selector)) lists.set(selector, Array.from({length:n}, element));
    return lists.get(selector);
  };
  one('#sharedHeader').querySelector = () => ({getBoundingClientRect: () => ({left:Math.max(32,(width-1480)/2+32)})});
  one('#refRoot').getBoundingClientRect = () => ({left:0});
  const document = {createElement: element, getElementById: id => id === 'refNative' ? null : one('#'+id)};
  const calls = [], seen = [];
  const weeks = ['2026-11-04','2026-11-06','2026-11-16'];
  const window = {addEventListener() {}, referenceResults: {beginScopeChange() {},failScopeChange() {},refresh(scope) {seen.push(scope);}}};
  const choices=[{key:'group:1',kind:'group',name:'Ремонт'},{key:'work:p',kind:'work',name:'Покраска'}];
  let dialog;
  window.referenceDesktop={dialog(){dialog=element();return dialog;},setDialogApply(action){dialog.append(action);}};
  window.sqlRowScroll={attach(){},reset(){}};
  function checkboxInDialog(text) {
    const visit=node=>node.options.some(child=>child.textContent===text)?node.options[0]:node.options.map(visit).find(Boolean);
    return visit(dialog);
  }
  function findInDialog(text) {
    const visit=node=>node.textContent===text?node:node.options.map(visit).find(Boolean);
    return visit(dialog);
  }
  const ui = window.unifiedUI = {revision:1, notice() {}, onSnapshot(fn) {this.configure = fn;}, api:{
    async unifiedNameOptions() { calls.push('catalog');return {total_rows:choices.length}; },
    async unifiedNameOptionsAtPage(){return {total_rows:choices.length,rows:choices,has_omitted_names:ui.hiddenNames===true};},
    async unifiedSelectedNameOptions(rev,keys){return {total_rows:choices.length,rows:choices.filter(row=>keys.includes(row.key))};},
    async unifiedPeriodOptions(revision,kind,group,after,limit,day) {
      calls.push([kind,day]);
      if(kind==='week'&&ui.hold)await ui.hold;
      if (kind === 'week' && ui.fail) throw Error('fixture');
      if (day) return {rows:weeks.filter(start=>start<=window.referenceCalendar.period(day,'month')[1] && day<=window.referenceCalendar.day(start,6))
        .map(start=>({start,end:window.referenceCalendar.day(start,6)}))};
      return {max: {day:'2026-11-29',week:'2026-11-16',month:'2025-02-01'}[kind]};
    }
  }};
  window.localStorage={getItem:key=>storage.get(key),setItem:(key,value)=>storage.set(key,value)};
  const getComputedStyle=()=>({getPropertyValue:key=>({
    '--filter-sidebar-min-width':'208px','--filter-sidebar-max-width':'272px','--filter-sidebar-gap':'12px',
    '--filter-at-side':one('#refRoot').dataset.filterSidebar?'1':''
  })[key]||''});
  const observer=class {observe(){}};
  const ctx={window,document,getComputedStyle,matchMedia:()=>({matches:width<=680}),ResizeObserver:observer,IntersectionObserver:observer};
  const run = name => vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../'+name+'.js'),'utf8'),ctx);
  run('reference-calendar');
  let draft;
  window.referenceCalendar={...window.referenceCalendar,open:options=>{draft=options;}};
  run('reference-filters');
  const controls = window.referenceFilters;
  const date = panel.querySelectorAll('[data-date]')[0];date.dataset={date:'start'};
  panel.querySelectorAll('[data-shift]')[1].dataset.shift='1';
  controls.init();await ui.configure({revision:1,org_names:['A']});
  const source = async val=>{const n=panel.querySelector('#referenceSource');n.value=val;n.onchange();await new Promise(r=>setImmediate(r));};
  const selectPeriod=value=>{const n=one('[data-period-selection]');n.value=value;return n.onchange();};
  return {selectPeriod, controls, calls, ui, source, seen, one, panel, choices, window, findInDialog, checkboxInDialog, draft:()=>draft, open:()=>{date.onclick();return draft;}};
}

test('an imported week and its comparison base can use different saved weekdays', async () => {
  const {controls,source,open,one,selectPeriod,draft} = await fixture();
  selectPeriod('week:single');
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
  const {controls,selectPeriod,source,one}=await fixture();
  selectPeriod('month:range');
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

test('a pending or failed source read cannot shift dates from the previous source', async () => {
  const {controls,panel,source,ui,one} = await fixture();
  let release;
  ui.hold=new Promise(resolve=>{release=resolve;});
  await source('imported');
  const next=panel.querySelectorAll('[data-shift]')[1];
  assert.equal(next.disabled,true);
  release();await new Promise(r=>setImmediate(r));
  assert.equal(controls.scope.from,'2026-11-16');
  assert.equal(next.disabled,false);
  await source('daily');ui.fail=true;await source('imported');
  assert.equal(next.disabled,true);
  ui.fail=false;await one('[data-retry-periods]').onclick();
  assert.equal(controls.scope.from,'2026-11-16');
  next.onclick();assert.equal(controls.scope.from,'2026-11-23');
});

test('a daily calendar draft cannot change a new snapshot with the same revision', async () => {
  const {controls,open,ui}=await fixture();
  const draft=open();
  await ui.configure({revision:1,org_names:['B']});
  const current=controls.scope;
  draft.apply('2026-11-05','week');
  assert.equal(controls.scope,current);
});

test('the all-works label does not undercount groups with names also used by works', async () => {
  const {one}=await fixture();
  assert.equal(one('[data-pick=works]').textContent,'Все работы и группы');
});

test('an open work or organization picker cannot apply its draft to a replacement snapshot', async () => {
  const {controls,ui,one,findInDialog}=await fixture();
  for (const type of ['works','orgs']) {
    one('[data-pick='+type+']').onclick();
    await new Promise(r=>setImmediate(r));
    findInDialog('Снять выбор').onclick();
    await ui.configure({revision:1,org_names:['Новое подразделение']});
    const current=controls.scope;
    findInDialog('Применить').onclick();
    assert.equal(controls.scope,current);
  }
});

test('selected group keys survive refresh in the same shared folder and reset when changing databases', async () => {
  const {controls,ui,one,findInDialog,checkboxInDialog,window,choices}=await fixture();
  window.sharedWorkspace={state:{mode:'shared',folder:'A',workspace_token:'A:1'}};
  await ui.configure({revision:1,org_names:['Первое','Второе']});
  one('[data-pick=works]').onclick();
  await new Promise(r=>setImmediate(r));
  findInDialog('Снять выбор').onclick();
  const check=checkboxInDialog('Ремонт (группа)');
  check.checked=true;check.onchange();
  findInDialog('Применить').onclick();
  await controls.setOrganizations(['Первое']);
  window.sharedWorkspace.state.workspace_token='A:2';
  ui.revision=2;
  await ui.configure({revision:2,org_names:['Первое','Второе']});
  assert.deepEqual([...controls.scope.selectedKeys],['group:1']);
  assert.deepEqual([...controls.scope.organizations],['Первое']);
  window.sharedWorkspace.state={mode:'shared',folder:'B',workspace_token:'B:2'};
  choices[0]={key:'group:1',kind:'group',name:'Монтаж'};
  await ui.configure({revision:2,org_names:['Первое','Другое']});
  assert.equal(controls.scope.allNames,true);
  assert.deepEqual([...controls.scope.selectedKeys],[]);
  assert.deepEqual([...controls.scope.organizations],['Первое','Другое']);
});

test('all works can be restored when no names are eligible for individual selection', async () => {
  const {controls,choices,ui,one,findInDialog}=await fixture();
  choices.length=0;
  await ui.configure({revision:1,org_names:['A']});
  one('[data-pick=works]').onclick();
  await new Promise(r=>setImmediate(r));
  findInDialog('Снять выбор').onclick();
  const all=findInDialog('Выбрать все');
  assert.equal(all.disabled,false);
  all.onclick();
  findInDialog('Применить').onclick();
  assert.equal(controls.scope.allNames,true);
});

test('calendar-month comparisons from daily reports include the leap day in the comparison month', async () => {
  const {controls,selectPeriod,open,one}=await fixture();
  selectPeriod('month:single');
  open().apply('2025-02-01','month');
  const compare=one('[data-compare]');compare.value='year';compare.onchange();
  assert.equal(controls.scope.from,'2025-02-01');
  assert.equal(controls.scope.to,'2025-02-28');
  assert.equal(controls.scope.baseFrom,'2024-02-01');
  assert.equal(controls.scope.baseTo,'2024-02-29');
  selectPeriod('day:single');
  open().apply('2025-02-28','day');
  assert.equal(controls.scope.baseTo,'2024-02-28');
});

test('deselecting a visible work cannot silently discard an unlisted work and group with the same name', async()=>{
  const {controls,ui,choices,one,checkboxInDialog,findInDialog}=await fixture();
  choices.splice(0,choices.length,{key:'work:Уборка',kind:'work',name:'Уборка'});
  ui.hiddenNames=true;
  one('[data-pick=works]').onclick();await new Promise(r=>setImmediate(r));
  const work=checkboxInDialog('Уборка');
  assert.equal(work.disabled,true);
  findInDialog('Применить').onclick();
  assert.equal(controls.scope.allNames,true);
  one('[data-pick=works]').onclick();await new Promise(r=>setImmediate(r));
  findInDialog('Снять выбор').onclick();
  const individually=checkboxInDialog('Уборка');
  assert.notEqual(individually.disabled,true);
  individually.checked=true;individually.onchange();findInDialog('Применить').onclick();
  assert.deepEqual([...controls.scope.selectedNames],['Уборка']);
});

test('reflecting works already applied in overview preserves its local metric choices',async()=>{
  const {controls,window}=await fixture();
  const updates=[];
  window.referenceDetail={active:'overviewPanel'};
  window.unifiedAnalytics={applyReferenceScope:scope=>updates.push(scope)};
  await controls.setWorks(false,new Map(),true);
  assert.equal(controls.scope.allNames,false);
  await controls.syncDetails();
  assert.equal(updates.length,0);
  await controls.setWorks(true,new Map());
  assert.equal(updates.length,1);
  assert.equal(updates[0].allNames,true);
});

test('filter placement stays on this device and falls back to the top in a narrow window', async () => {
  const storage=new Map();
  const wide=await fixture(storage,2048);
  const picker=wide.one('#appearanceFilterPlacement');
  assert.equal(picker.value,'top');
  const before=wide.controls.scope;
  picker.value='side';
  picker.onchange();
  assert.equal(wide.controls.scope,before);
  assert.equal(wide.controls.atSide(),true);
  assert.equal((await fixture(storage,2048)).controls.atSide(),true);
  const narrow=await fixture(storage,1440);
  assert.equal(narrow.one('#appearanceFilterPlacement').value,'side');
  assert.equal(narrow.controls.atSide(),false);
});


test('baseline dates can be edited separately without changing the current period', async () => {
  const {controls,one,draft,panel} = await fixture();
  const current=[controls.scope.from,controls.scope.to];
  one('[data-base-start]').onclick();
  const start=draft();
  assert.equal(start.title,'Начало базового периода');
  start.apply('2026-10-05','week');
  assert.equal(controls.scope.baseFrom,'2026-10-05');
  assert.equal(controls.scope.baseTo,'2026-10-11');
  one('[data-base-end]').onclick();
  const end=draft();
  assert.equal(end.title,'Конец базового периода');
  end.apply('2026-10-26','week');
  assert.equal(controls.scope.baseFrom,'2026-10-05');
  assert.equal(controls.scope.baseTo,'2026-11-01');
  assert.deepEqual([controls.scope.from,controls.scope.to],current);
  assert.equal(one('[data-compare]').value,'custom');
  end.apply('2026-09-14','week');
  assert.equal(controls.scope.baseTo,'2026-11-01','invalid earlier end must not overwrite prior range');
  const select=one('[data-compare]');select.value='prev';select.onchange();
  assert.notEqual(controls.scope.baseTo,'2026-11-01');
});

test('baseline manual end supports leap February while keeping a single month intact', async () => {
  const {controls,one,selectPeriod,open,draft}=await fixture();
  selectPeriod('month:single');
  open().apply('2025-02-01','month');
  one('[data-base-start]').onclick();draft().apply('2024-01-01','month');
  selectPeriod('month:range');
  one('[data-base-end]').onclick();draft().apply('2024-02-01','month');
  assert.equal(controls.scope.baseFrom,'2024-01-01');
  assert.equal(controls.scope.baseTo,'2024-02-29');
});

test('editing imported weekly baseline end keeps the actual report weekday', async () => {
  const {controls,selectPeriod,source,open,one,draft}=await fixture();
  selectPeriod('week:single');
  await source('imported');
  open().apply('2026-11-07','week','2026-11-04');
  one('[data-base-start]').onclick();
  draft().apply('2026-11-07','week','2026-11-04');
  one('[data-base-end]').onclick();
  const end=draft();
  assert.equal(end.spanFor('2026-11-18','week')[0],'2026-11-18');
  end.apply('2026-11-18','week','2026-11-18');
  assert.equal(controls.scope.baseFrom,'2026-11-04');
  assert.equal(controls.scope.displayBaseTo,'2026-11-24');
  end.apply('2026-11-16','week','2026-11-16');
  assert.equal(controls.scope.displayBaseTo,'2026-11-24','a different weekday must not corrupt the baseline');
});

test('one selection applies the year and single-period mode and shows actual dates for both sides', async () => {
  const {controls, selectPeriod, one, seen, open}=await fixture();
  const before=seen.length;
  selectPeriod('year:single');
  assert.equal(seen.length,before+1);
  assert.equal(controls.scope.displayType,'year');
  assert.equal(controls.scope.overviewMode,'single');
  open().apply('2025-01-01','year');
  assert.equal(one('[data-base-start]').textContent,'02.01 — 31.12.2024');
  assert.equal(one('[data-date=start]').textContent,'01.01 — 31.12.2025');
  const compare=one('[data-compare]');
  compare.value='year';compare.onchange();
  assert.equal(one('[data-base-start]').textContent,'01.01 — 31.12.2024');
});
