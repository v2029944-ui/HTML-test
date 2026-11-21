"""Shared and detailed calendars in the desktop report shell."""
import re
import subprocess
import unittest
from pathlib import Path

from playwright.sync_api import expect, sync_playwright


class CalendarFocus(unittest.TestCase):
    def test_pending_week_keeps_toolbar_focus_and_displays_both_years(self):
        calendar = Path(__file__).resolve().parents[1] / 'tauri' / 'reference-calendar.js'
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch()
            page = browser.new_page()
            page.set_content('<button id="period">Период</button>')
            page.add_script_tag(path=str(calendar))
            page.evaluate("""() => referenceCalendar.open({
                anchor: document.querySelector('#period'), title: 'Период', unit: 'week',
                initial: '2026-12-30', apply() {},
                weeksForDay: () => new Promise(resolve => window.completeWeekRead = resolve)
            })""")
            forward = page.get_by_role('button', name='Вперёд по календарю')
            forward.focus()
            page.keyboard.press('Enter')
            expect(forward).to_be_focused()
            page.evaluate("completeWeekRead([{start:'2026-12-30', end:'2027-01-05'}])")
            expect(page.locator('.cal-footer output')).to_have_text('30.12.2026 — 05.01.2027')
            expect(forward).to_be_focused()
            page.keyboard.press('Escape')
            expect(page.locator('#period')).to_be_focused()
            browser.close()

    def test_comparison_uses_the_saved_week_of_its_base(self):
        repo = Path(__file__).resolve().parents[1]
        html = subprocess.check_output([
            'node', '-e', "const {buildUnified}=require('./tauri/unified-build.cjs');"
            "process.stdout.write(buildUnified(require('node:fs').readFileSync('index.html','utf8')).html)"
        ], cwd=repo).decode('utf-8')
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch()
            page = browser.new_page()
            page.set_content(re.sub(r'<script\b[^>]*>.*?</script>', '', html, flags=re.S))
            for name in ('reference-calendar', 'reference-detail'):
                page.add_script_tag(path=str(repo / 'tauri' / (name + '.js')))
            page.evaluate("""() => {
                const scope={revision:1,source:'imported',type:'week',displayType:'week',
                    from:'2026-11-16',to:'2026-11-16',baseFrom:'2026-11-06',baseTo:'2026-11-06'};
                window.unifiedUI={scope, onScope(fn){this.changed=fn}, api:{
                    unifiedPeriodOptions:async()=>({rows:[
                        {start:'2026-11-04',end:'2026-11-10'},
                        {start:'2026-11-06',end:'2026-11-12'}
                    ]})}};
                window.unifiedOriginal={periodEnd:key=>referenceCalendar.day(key,6)};
                window.unifiedExtended={openComparison(){},leaveDetail(){}};
                document.body.append(document.querySelector('#refNative'));
                const comparison=document.querySelector('#comparison');
                document.body.append(comparison);comparison.hidden=false;comparison.open=true;
                document.querySelector('#comparisonMode').value='periods';
                referenceDetail.show('comparison');
            }""")
            edit = page.locator('#refComparisonDates summary')
            edit.click()
            end = page.locator('[data-compare-date="1"]')
            expect(end).to_have_text('12.11.2026')
            expect(page.locator('[data-lengths]')).to_contain_text('Дней в базе: 7')
            end.click()
            dialog = page.locator('dialog.period-calendar')
            dialog.locator('[data-stamp="2026-11-07"]').click()
            options = dialog.get_by_label('Сохранённая неделя')
            expect(options).to_be_visible()
            expect(dialog.get_by_role('button', name='Подтвердить')).to_be_disabled()
            options.select_option('2026-11-04')
            dialog.get_by_role('button', name='Подтвердить').click()
            expect(end).to_have_text('10.11.2026')
            self.assertEqual(page.evaluate('referenceDetail.currentComparison().before'),
                             {'from':'2026-11-04','to':'2026-11-04'})
            page.evaluate("""() => {
                unifiedUI.scope={...unifiedUI.scope, baseFrom:'2026-10-19',baseTo:'2026-10-19'};
                unifiedUI.changed();
            }""")
            expect(end).to_have_text('10.11.2026')
            reset = page.locator('[data-reset-base]')
            reset.click()
            expect(edit).to_be_focused()
            expect(page.locator('#comparison .detail-date-scope')).to_contain_text('25.10.2026')
            edit.click()
            expect(end).to_have_text('25.10.2026')
            browser.close()
