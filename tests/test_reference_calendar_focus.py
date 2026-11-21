"""Keyboard navigation while an imported week's dates are still loading."""
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
