const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../reference-detail.js'), 'utf8');

function fixture() {
  const nodes = new Map();
  const create = tag => ({
    tagName: tag.toUpperCase(), children: [], dataset: {}, listeners: {}, hidden: false,
    disabled: false, open: false, value: '', title: '', textContent: '',
    classList: {toggle(){}}, setAttribute(){}, focus(){},
    append(...children){this.children.push(...children);},
    addEventListener(name, callback){(this.listeners[name] ??= []).push(callback);},
    dispatchEvent(event){for(const callback of this.listeners[event.type] || [])callback(event);},
  });
  const get = id => {
    if (!nodes.has(id)) nodes.set(id, create('div'));
    return nodes.get(id);
  };
  const scopeLine = create('p');
  scopeLine.children = [create('span'), create('span')];
  const chartPanel = get('chartPanel');
  chartPanel.querySelector = selector => selector === ':scope > .detail-date-scope' ? scopeLine : null;

  const chartGroup = create('div');
  chartGroup.querySelector = selector =>
    selector === '[data-reference-line]' ?
      chartGroup.children.find(button => 'referenceLine' in button.dataset) || null : null;
  chartGroup.querySelectorAll = selector =>
    chartGroup.children.filter(button =>
      selector === 'button' || (selector === '[data-chart]' && 'chart' in button.dataset));
  chartGroup.prepend = button => { chartGroup.children.unshift(button); button.parentElement = chartGroup; };
  for (const mode of ['bar', 'table']) {
    const button = create('button');
    button.dataset.chart = mode;
    button.parentElement = chartGroup;
    chartGroup.children.push(button);
  }
  const orientationLabel = create('label');
  get('chartOrientation').closest = selector => selector === 'label' ? orientationLabel : null;
  get('workViewMode').dispatchEvent = () => {};

  const document = {
    getElementById: get,
    createElement: create,
    querySelector: selector => selector === '#chartPanel [data-chart="bar"]' ?
      chartGroup.children.find(button => button.dataset.chart === 'bar') : null,
  };
  const state = {chart: 'line'};
  let onScope;
  const referenceFilters = {scope: {overviewMode: 'range'}};
  const window = {
    unifiedUI: {
      state,
      scope: {from: '2026-01-05', to: '2026-01-06', source: 'daily', displayType: 'day'},
      onScope(callback){onScope = callback;},
    },
    referenceFilters,
    unifiedOriginal: {periodEnd: date => date},
    unifiedExtended: {leaveDetail(){}},
  };
  vm.runInNewContext(source, {window, document, Event: class Event {constructor(type){this.type=type;}}});
  return {detail: window.referenceDetail, state, referenceFilters, chartGroup, getLine: () =>
    chartGroup.children.find(button => 'referenceLine' in button.dataset), changedScope: () => onScope()};
}

test('single-period chart hides the line control without losing the user\'s preferred mode', () => {
  const f = fixture();
  f.detail.show('chartPanel');
  assert.equal(f.state.chart, 'line');
  assert.equal(f.getLine().hidden, false);
  f.detail.setChartPeriodCount(1);
  assert.equal(f.getLine().hidden, true);
  assert.equal(f.getLine().disabled, true);
  assert.equal(f.state.chart, 'bar');
  f.detail.setChartPeriodCount(2);
  assert.equal(f.getLine().hidden, false);
  assert.equal(f.getLine().disabled, false);
  assert.equal(f.state.chart, 'line');
});

test('single selection mode disables line before SQL data arrives', () => {
  const f = fixture();
  f.referenceFilters.scope.overviewMode = 'single';
  f.detail.show('chartPanel');
  assert.equal(f.getLine().hidden, true);
  assert.equal(f.state.chart, 'bar');
  f.referenceFilters.scope.overviewMode = 'range';
  f.changedScope();
  assert.equal(f.getLine().hidden, false);
  assert.equal(f.state.chart, 'line');
});
