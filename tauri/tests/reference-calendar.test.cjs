'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('programmatic calendar period changes update the visible weekly range before focus or a DOM mutation', () => {
  function element() {
    const attributes = new Map();
    return {
      dataset: {}, classList: {add() {}}, value: '', textContent: '', children: [],
      setAttribute(name, value) { attributes.set(name, value); },
      getAttribute(name) { return attributes.get(name) ?? null; },
      closest() { return null; }, addEventListener() {},
      append(...children) { this.children.push(...children); },
      after(other) { this.trigger = other; },
    };
  }
  const source = element();
  source.id = 'structureBase';
  source.type = 'week';
  source.setAttribute('aria-label', 'Базовый период');
  const panel = element();
  panel.querySelectorAll = () => [source];
  const document = {createElement: element, querySelector: () => panel};
  const window = {desktopAPI: {}, unifiedOriginal: {
    state: {}, el: element, $: id => id === source.id ? source : null,
  }};
  const context = {window, document, tableCsvButton() {}, MutationObserver: class { observe() {} }};
  for (const name of ['native-period-selection', 'unified-base', 'reference-calendar']) {
    vm.runInNewContext(fs.readFileSync(path.join(__dirname, '..', name + '.js'), 'utf8'), context);
  }
  window.referenceCalendar.enhance(panel);
  assert.equal(source.trigger.textContent, 'Выбрать дату');
  window.unifiedUI.setPeriodInput(source.id, 'week', '2026-11-09');
  assert.equal(source.trigger.textContent, '09.11.2026 — 15.11.2026');
  window.unifiedUI.setPeriodInput(source.id, 'week', '2026-11-16');
  assert.equal(source.trigger.getAttribute('aria-label'), 'Базовый период · 16.11.2026 — 22.11.2026');
  source.dataset.rangeBoundary = 'start';
  window.referenceCalendar.syncField(source);
  assert.equal(source.trigger.textContent, '16.11.2026');
  source.dataset.rangeBoundary = 'end';
  window.referenceCalendar.syncField(source);
  assert.equal(source.trigger.textContent, '22.11.2026');
  window.unifiedUI.setPeriodInput(source.id, 'month', '2024-02-01');
  assert.equal(source.trigger.textContent, '29.02.2024');
  assert.equal(source.value, '2024-02', 'calendar month remains the SQL input');
  assert.equal(source.dataset.unifiedTouched, undefined);
});
