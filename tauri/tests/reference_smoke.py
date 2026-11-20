"""Exercise the installed report shell through its normal visible user routes.

Called by windows_smoke.py on an isolated installed WebView2, both before and
after real synthetic XLSX import. Never replace native APIs or write storage.
"""
import json
from math import isfinite
from pathlib import Path

from playwright.sync_api import TimeoutError as PlaywrightTimeoutError, expect

from reference_table_help import check_table_help


TIMEOUT = 90_000


def check_reference(page, output, has_reports):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    prefix = 'reference-reports' if has_reports else 'reference-empty'
    inventory = []
    page.wait_for_function('''() => window.appReady && Number.isSafeInteger(window.unifiedUI?.revision)
      && window.unifiedUI.snapshot''', timeout=TIMEOUT)
    assert page.evaluate('window.appReady') is True
    initial_viewport = page.viewport_size
    initial_window_size = page.evaluate('({width: window.innerWidth, height: window.innerHeight})')
    initial_zoom = page.locator('html').evaluate('(node) => node.style.zoom')
    initial_revision = page.evaluate('window.unifiedUI.revision')
    initial_theme = page.locator('#appearanceTheme').input_value()
    initial_size = page.locator('#appearanceSize').input_value()

    def record(name, root=None, scroll=True):
        root = root if root is not None else page.locator('body')
        page.wait_for_function('''() => !window.tauriOperations?.reads &&
          !window.tauriOperations?.saves && ![...document.querySelectorAll('[aria-busy="true"]')]
          .some(node => node.getClientRects().length)''', timeout=TIMEOUT)
        if scroll and root.count() == 1:
            # Large sections must start at their heading; centering them crops
            # the top of import forms and analytics tabs out of the inventory.
            large_section = root.evaluate("""node => node.tagName !== 'DIALOG' &&
              !node.closest('dialog') && node.getBoundingClientRect().height >
              document.documentElement.clientHeight""")
            if large_section:
                root.evaluate("node => node.scrollIntoView({block:'start',inline:'nearest',behavior:'instant'})")
            else:
                root.scroll_into_view_if_needed()
        controls = root.locator('button,input,select,textarea,summary,a').evaluate_all('''nodes => nodes
          .filter(node => node.getClientRects().length && getComputedStyle(node).visibility !== 'hidden')
          .map(node => ({tag:node.tagName,id:node.id,
            name:node.getAttribute('aria-label') || node.innerText || node.title || node.placeholder || '',
            type:node.type || '',disabled:Boolean(node.disabled),value:node.value ?? null,
            expanded:node.getAttribute('aria-expanded'),pressed:node.getAttribute('aria-pressed')}))''')
        inventory.append({'step': name, 'controls': controls})
        page.screenshot(path=str(output / f'{prefix}-{name}.png'))
        (output / f'{prefix}-inventory.json').write_text(
            json.dumps(inventory, ensure_ascii=False, indent=2), encoding='utf-8')

    def focused(locator):
        expect(locator).to_be_focused(timeout=TIMEOUT)

    def modal(dialog):
        expect(dialog).to_be_visible(timeout=TIMEOUT)
        assert dialog.evaluate('(node) => node.open && node.matches(":modal")')
        # Browser top-layer dialogs must intercept actual background interaction.
        try:
            page.locator('#refData').click(trial=True, timeout=600)
        except PlaywrightTimeoutError:
            pass
        else:
            raise AssertionError('An open modal did not block the background Data button')
        # Native dialogs may hand Tab to browser chrome (activeElement=body).
        # A focusable control in the underlying application must never receive it.
        for _ in range(4):
            page.keyboard.press('Tab')
            assert dialog.evaluate('(node) => document.activeElement === document.body || node.contains(document.activeElement)'), 'Modal focused a background control'

    checked_disclosures = set()

    def check_disclosures(root, step):
        ids = root.locator('details[id]').evaluate_all('nodes => nodes.map(node => node.id)')
        for key in ids:
            if key in checked_disclosures:
                continue
            box = root.locator('details[id=' + json.dumps(key) + ']')
            summary = box.locator(':scope > summary').first
            if not summary.is_visible():
                continue
            was_open = box.evaluate('node => node.open')
            modals = page.locator('dialog:modal').count()
            # Exercise a full close/reopen cycle from this actual section.
            # Restore its entry state so subsequent routes see the same screen.
            states = [False, True] if was_open else [True, False, True, False]
            for expanded in states:
                summary.click()
                expect(box).to_have_js_property('open', expanded)
                expect(summary).to_be_focused()
                expect(page.locator('dialog:modal')).to_have_count(modals)
            checked_disclosures.add(key)
            inventory.append({'step': step, 'disclosure': key, 'initiallyOpen': was_open,
                              'closedAndReopened': True, 'focusRestored': True})

    def close_sheet(trigger, how='button'):
        sheet = page.locator('#sheet')
        if how == 'escape':
            page.keyboard.press('Escape')
        else:
            sheet.get_by_role('button', name='Закрыть', exact=True).click()
        expect(sheet).not_to_be_visible(timeout=TIMEOUT)
        focused(trigger)

    def open_menu():
        trigger = page.locator('#refData')
        trigger.click()
        sheet = page.locator('#sheet')
        modal(sheet)
        for name in ['Отчёты', 'Группы работ', 'Настройки', 'Исходный макет']:
            expect(sheet.get_by_role('button', name=name, exact=True)).to_be_visible()
        return trigger, sheet

    def panel(target):
        expect(page.locator('#toolsPage')).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#' + target)).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#backBtn')).to_be_visible()

    def close_panel(trigger):
        page.locator('#backBtn').click()
        expect(page.locator('#toolsPage')).not_to_be_visible(timeout=TIMEOUT)
        focused(trigger)

    def export_available(selector, has_rows, name):
        control = page.locator(selector)
        if has_rows:
            expect(control).to_be_visible(timeout=TIMEOUT)
            expect(control).to_be_enabled(timeout=TIMEOUT)
        else:
            expect(control).to_have_count(0)
        inventory.append({'step': name, 'available': bool(has_rows),
                          'reason': None if has_rows else 'The current result has no rows to export.'})

    def chart_hover(root):
        marks = root.locator('svg rect[aria-label]:visible, svg circle[aria-label]:visible')
        for index in range(marks.count()):
            mark = marks.nth(index)
            label = mark.get_attribute('aria-label')
            value_text = label.rsplit(':', 1)[-1].strip()
            normalized = ''.join(value_text.split()).replace(',', '.')
            value = float(normalized)
            assert isfinite(value), 'A chart mark exposes a nonfinite value'
            if value == 0:
                continue
            title = mark.locator('title')
            tooltip = title.text_content() if title.count() else mark.get_attribute('data-chart-tooltip')
            if tooltip is not None:
                assert tooltip == label, 'Chart tooltip differs from its accessible data label'
            mark.hover()
            assert mark.evaluate('(node) => node.matches(":hover")'), 'The chart mark was not hovered'
            inventory.append({'step': '08-horizontal-chart-hover', 'available': True,
                              'accessibleLabel': label, 'tooltip': tooltip or label,
                              'displayedValue': value_text, 'numericValue': value})
            return
        inventory.append({'step': '08-horizontal-chart-hover', 'available': False,
                          'visibleMarks': marks.count(),
                          'reason': 'No visible nonzero SVG series mark for the current filters.'})

    def table_help(selector, name, expected_count=None):
        # A missing conditional table is meaningful only after its SQL is done.
        page.wait_for_function('''() => !window.tauriOperations?.reads &&
          !window.tauriOperations?.saves && ![...document.querySelectorAll('[aria-busy="true"]')]
          .some(node => node.getClientRects().length)''', timeout=TIMEOUT)
        section = page.locator(selector + ':visible').first
        available = section.count() > 0
        if available:
            visible_headers = section.locator('thead th:visible').count()
            assert visible_headers > 0, 'A rendered results table has no visible headings'
            expected = visible_headers if expected_count is None else expected_count
            labels = check_table_help(section, expected)
            inventory.append({'step': name, 'table': selector, 'available': True,
                              'visibleHeaders': visible_headers, 'checkedHints': labels})
        else:
            root = page.locator(selector.split(' ', 1)[0])
            reason = root.inner_text().strip()[:500] if root.count() and root.is_visible() else ''
            inventory.append({'step': name, 'table': selector, 'available': False,
                              'reason': reason or 'No visible results table for the current filters.'})

    def quality_controls():
        expect(page.locator('#qualitySummary')).to_contain_text('Найдено:', timeout=TIMEOUT)
        has_issues = page.locator('#qualityTable tbody tr').count() > 0
        export_available('#qualityExportCsv', has_issues, '04-quality-csv-availability')
        inventory.append({'step': '04-quality-advice-availability', 'available': has_issues,
                          'reason': None if has_issues else 'The current result has no issue advice.'})
        if has_issues:
            advice = page.locator('#qualityTable .quality-advice').first
            summary = advice.locator(':scope > summary')
            expect(summary).to_have_text('Как проверить и исправить')
            if advice.evaluate('(node) => node.open'):
                summary.click()
            for expanded in [True, False, True]:
                summary.click()
                expect(advice).to_have_js_property('open', expanded)
                focused(summary)
            expect(advice.locator('p')).to_be_visible()
        record('04-quality', page.locator('#qualityTable'))
        table_help('#qualityTable > section', '04-quality-table-help', 2)

    def calendar(trigger, name, mode=None, commit=False):
        """A calendar selection stays a draft until the explicit confirm button."""
        before = trigger.inner_text()
        trigger.click()
        dialog = page.locator('dialog.period-calendar[open]')
        modal(dialog)
        if mode:
            expect(dialog.locator('.cal-grid')).to_have_attribute('data-mode', mode)
        cells = dialog.locator('.cal-grid button[data-stamp]:enabled[aria-pressed="false"]')
        selected = None
        if cells.count():
            selected = cells.first.get_attribute('data-stamp')
            cells.first.click()
            expect(trigger).to_have_text(before)
        record(name + '-draft', dialog)
        dialog.get_by_role('button', name='Отмена', exact=True).click()
        expect(page.locator('dialog.period-calendar[open]')).to_have_count(0)
        expect(trigger).to_have_text(before)
        focused(trigger)
        trigger.click()
        dialog = page.locator('dialog.period-calendar[open]')
        expect(dialog).to_be_visible()
        box = dialog.bounding_box()
        # CDP-attached WebView2 pages have no emulated Playwright viewport.
        viewport = page.evaluate('({width: window.innerWidth, height: window.innerHeight})')
        assert box and viewport['width'] > 0 and viewport['height'] > 0
        # The calendar supports backdrop dismissal; click outside its box.
        point = {'x': 2, 'y': 2}
        if box['x'] <= 2 <= box['x'] + box['width'] and box['y'] <= 2 <= box['y'] + box['height']:
            point = {'x': viewport['width'] - 2, 'y': viewport['height'] - 2}
        page.mouse.click(**point)
        expect(page.locator('dialog.period-calendar[open]')).to_have_count(0)
        expect(trigger).to_have_text(before)
        focused(trigger)
        trigger.click()
        expect(page.locator('dialog.period-calendar[open]')).to_have_count(1)
        page.keyboard.press('Escape')
        expect(page.locator('dialog.period-calendar[open]')).to_have_count(0)
        expect(trigger).to_have_text(before)
        focused(trigger)
        if commit:
            trigger.click()
            dialog = page.locator('dialog.period-calendar[open]')
            if selected:
                dialog.locator(f'button[data-stamp="{selected}"]').click()
            dialog.get_by_role('button', name='Подтвердить', exact=True).click()
            expect(page.locator('dialog.period-calendar[open]')).to_have_count(0)
            focused(trigger)
            if selected and mode == 'days':
                expect(trigger).to_contain_text('.'.join(reversed(selected.split('-'))))

    def history_close(trigger, panel_id, content_id, close_id, name):
        detail = page.locator('#' + panel_id)
        content = page.locator('#' + content_id)
        trigger.click()
        expect(detail).to_be_visible(timeout=TIMEOUT)
        expect(content).to_contain_text('Динамика за', timeout=TIMEOUT)
        record(name, detail)
        page.locator('#' + close_id).click()
        expect(detail).not_to_be_visible()
        focused(trigger)
        trigger.click()
        expect(detail).to_be_visible(timeout=TIMEOUT)
        expect(content).to_contain_text('Динамика за', timeout=TIMEOUT)
        page.keyboard.press('Escape')
        expect(detail).not_to_be_visible()
        focused(trigger)

    try:
        expect(page.locator('#sharedHeader')).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#reportShell')).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#s1')).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#s2')).not_to_be_visible()
        expect(page.locator('#toolsPage')).not_to_be_visible()
        assert page.evaluate('window.unifiedUI.snapshot.reports > 0') is bool(has_reports)
        assert 'Демоданные' not in page.locator('#sharedHeader').inner_text()
        # The installed route uses the existing native document, never an iframe importer.
        expect(page.locator('#toolsFrame')).to_have_count(0)
        if has_reports:
            # The preceding empty-state run deliberately changes dates. Select
            # the freshly imported real daily reports using the visible shortcut.
            initial_fold = page.locator('#reportShell .filter-fold')
            if not initial_fold.evaluate('(node) => node.open'):
                initial_fold.locator(':scope > summary').click()
            latest_week = page.locator('#reportShell .quick-actions').get_by_role('button', name='Неделя', exact=True)
            expect(latest_week).to_be_enabled(timeout=TIMEOUT)
            latest_week.click()
        # Preserve the initial overview, including the header, before any panel scroll.
        page.evaluate('window.scrollTo(0, 0)')
        page.locator('#sharedHeader').scroll_into_view_if_needed()
        record('01-summary', page.locator('body'), scroll=False)
        for selector in ['#s1 .metric-sample', '#s1 .quality-notice button']:
            source = page.locator(selector).first
            if not source.count() or not source.is_visible():
                continue
            source.click()
            quality = page.locator('#sheet')
            modal(quality)
            if 'metric-sample' in selector:
                expect(quality.get_by_role('heading', name='Состав сравнения', exact=True)).to_be_visible()
                record('01-comparison-sample', quality)
                quality.get_by_role('button', name='Проверить отчёты', exact=True).click()
            expect(quality.locator('.coverage-matrix')).to_be_visible(timeout=TIMEOUT)
            record('01-quality-' + ('sample' if 'metric-sample' in selector else 'notice'), quality)
            close_sheet(source)

        trigger, sheet = open_menu()
        record('02-data-menu', sheet)
        close_sheet(trigger)
        trigger, sheet = open_menu()
        close_sheet(trigger, 'escape')

        trigger, sheet = open_menu()
        original = sheet.get_by_role('button', name='Исходный макет', exact=True)
        original.click()
        workspace = page.locator('#refWorkspaceDialog')
        expect(workspace).to_be_visible(timeout=TIMEOUT)
        assert workspace.evaluate('(node) => node.matches(":modal")')
        assert sheet.evaluate('(node) => node.open'), 'Underlying Data menu must remain open'
        record('03-native-workspace', workspace)
        page.locator('#refWorkspaceClose').click()
        expect(workspace).not_to_be_visible(timeout=TIMEOUT)
        expect(sheet).to_be_visible()
        focused(original)
        original.click()
        expect(workspace).to_be_visible(timeout=TIMEOUT)
        page.keyboard.press('Escape')
        expect(workspace).not_to_be_visible(timeout=TIMEOUT)
        expect(sheet).to_be_visible()
        focused(original)
        close_sheet(trigger)

        # Every real management entry is reachable through the user-facing menu.
        for title, target in [('Отчёты', 'upload'), ('Группы работ', 'groups'), ('Настройки', 'storage')]:
            trigger, sheet = open_menu()
            sheet.get_by_role('button', name=title, exact=True).click()
            expect(sheet).not_to_be_visible()
            panel(target)
            record('04-menu-' + target, page.locator('#toolsPage'))
            if target == 'upload':
                quality_controls()
            if target == 'groups':
                page.locator('#createGroup').click()
                expect(page.locator('#groupEditor')).to_have_attribute('open', '')
                focused(page.locator('#groupName'))
                page.locator('#groupName').fill('Черновик проверки интерфейса')
                record('04-group-editor', page.locator('#groups'))
                page.locator('#resetGroup').click()
                expect(page.locator('#groupName')).to_have_value('')
                if not page.locator('#groupAuditPanel').evaluate('(node) => node.open'):
                    page.locator('#groupAuditPanel > summary').click()
                if page.evaluate('Boolean(window.unifiedUI.scope)'):
                    # A completed pager avoids reading the previous range's coverage
                    # while the newly opened audit is still loading.
                    page.wait_for_function("""() => /^(Страница [0-9]+|Записей нет[.])/.test(
                      document.getElementById('groupAuditPage').textContent)""", timeout=TIMEOUT)
                    expect(page.locator('#groupAuditCoverage')).to_contain_text('Найдено:', timeout=TIMEOUT)
                    audit_total = int(page.locator('#groupAuditCoverage').inner_text()
                                      .split('Найдено:', 1)[1].split('·', 1)[0].strip())
                    export_available('#groupAuditExportCsv', audit_total > 0, '04-group-audit-csv-availability')
                else:
                    expect(page.locator('#groupAuditCoverage')).to_have_text(
                        'Выберите корректный период в аналитике.', timeout=TIMEOUT)
                    export_available('#groupAuditExportCsv', False, '04-group-audit-csv-availability')
                record('04-group-audit', page.locator('#groups'))
                if not page.locator('#groupCatalog').evaluate('(node) => node.open'):
                    page.locator('#groupCatalog > summary').click()
                record('04-group-catalog', page.locator('#groups'))
            # Menu actions disappear when the menu closes, so closure must focus
            # the durable Data trigger rather than a detached menu child.
            check_disclosures(page.locator('#' + target), '04-management-disclosures')
            close_panel(trigger)

        importer = page.locator('#refImport')
        importer.click()
        panel('upload')
        expect(page.locator('#inspectFile')).to_be_disabled()
        expect(page.locator('#saveImport')).to_be_disabled()
        expect(page.locator('#excelFile')).to_be_attached()
        mapping = page.locator('#importMappingMode')
        mapping_summary = page.locator('#upload').get_by_text('Столбцы и даты каждого набора', exact=True)
        if not mapping_summary.evaluate('(node) => node.parentElement.open'):
            mapping_summary.click()
        expect(mapping).to_be_visible()
        mapping.select_option('manual')
        expect(page.locator('#importManualWrap')).to_be_visible()
        page.locator('#addImportMapping').click()
        expect(page.locator('#importColumnMappings').get_by_role('button', name='Удалить набор', exact=True)).to_have_count(2)
        page.locator('#importColumnMappings').get_by_role('button', name='Удалить набор', exact=True).last.click()
        expect(page.locator('#importColumnMappings').get_by_role('button', name='Удалить набор', exact=True)).to_have_count(1)
        mapping.select_option('auto')
        expect(page.locator('#importManualWrap')).not_to_be_visible()
        page.locator('#importFormat').select_option('daily-wide')
        expect(page.locator('#dailyWideOptions')).to_be_visible()
        year_trigger = page.locator('#dailyYear + .ref-calendar-field')
        calendar(year_trigger, '05-wide-year', 'years')
        page.locator('#importFormat').select_option('legacy')
        page.locator('#importType').select_option('day')
        date_trigger = page.locator('#importWeek + .ref-calendar-field')
        calendar(date_trigger, '06-import-day', 'days')
        calendar(page.locator('#referenceReportYear'), '06-report-calendar-year', 'years')
        record('07-import', page.locator('#toolsPage'))
        close_panel(importer)

        # Tabs must switch actual native sections, not another application route.
        analytics = page.locator('#refAnalytics')
        analytics.click()
        panel('chartPanel')
        sections = [('Состав работ', 'structurePanel'), ('Сравнение', 'comparison'),
                    ('Отклонения', 'usualPanel'), ('Частота', 'regularityPanel'), ('Динамика', 'chartPanel')]
        for title, target in sections:
            page.locator('#toolsPage .tool-tabs').get_by_role('button', name=title, exact=True).click()
            panel(target)
            local_calendars = page.locator('#' + target).locator('.ref-calendar-field:visible:enabled, [data-compare-date]:visible:enabled')
            for index in range(local_calendars.count()):
                calendar(local_calendars.nth(index), f'08-{target}-calendar-{index}')
            if target == 'comparison':
                comparison_modes = page.locator('#refComparisonModes')
                comparison_modes.locator('[data-value="organizations"]').click()
                expect(comparison_modes.locator('[data-value="organizations"]')).to_have_attribute('aria-pressed', 'true')
                expect(page.locator('#orgComparisonFilters')).to_be_visible()
                record('08-comparison-organizations', page.locator('#comparison'))
                comparison_modes.locator('[data-value="periods"]').click()
                expect(comparison_modes.locator('[data-value="periods"]')).to_have_attribute('aria-pressed', 'true')
                local_dates = page.locator('#refComparisonDates')
                if local_dates.count() and local_dates.is_visible():
                    local_dates.locator('[data-edit-base]').click()
                    for index in [0, 1]:
                        calendar(local_dates.locator(f'[data-compare-date="{index}"]'), f'08-comparison-base-{index}')
                    local_dates.locator('[data-reset-base]').click()
            if target == 'chartPanel':
                chart = page.locator('#chartPanel')
                chart.locator('[data-chart="bar"]').click()
                expect(chart.locator('[data-chart="bar"]')).to_have_attribute('aria-pressed', 'true')
                page.locator('#chartOrientation').select_option('horizontal')
                record('08-horizontal-chart', chart)
                chart_hover(chart)
                page.locator('#chartOrientation').select_option('vertical')
                chart.locator('[data-chart="table"]').click()
                expect(chart.locator('[data-chart="table"]')).to_have_attribute('aria-pressed', 'true')
                chart.locator('[data-reference-line]').click()
                expect(chart.locator('[data-reference-line]')).to_have_attribute('aria-pressed', 'true')
            if target == 'usualPanel' and has_reports:
                work = page.locator('#usualResults .service-link').first
                expect(work).to_be_visible(timeout=TIMEOUT)
                history_close(work, 'analyticsDetails', 'analyticsDetailContent', 'closeAnalyticsDetails', '08-analytics-work-history')
                expect(page.locator('#toolsPage')).to_be_visible()
            check_disclosures(page.locator('#toolsPage'), '08-analytics-disclosures')
            record('08-analytics-' + target, page.locator('#toolsPage'))
            help_tables = {'structurePanel': '#structureResults .metric-table',
                           'usualPanel': '#usualResults .metric-table',
                           'regularityPanel': '#regularityResults .metric-table'}
            if target in help_tables:
                table_help(help_tables[target], '08-' + target + '-table-help')
            if target == 'comparison':
                contribution = page.locator('#contributions')
                contribution_summary = contribution.locator(':scope > summary')
                if contribution_summary.is_visible():
                    if not contribution.evaluate('(node) => node.open'):
                        contribution_summary.click()
                    expect(contribution).to_have_js_property('open', True)
                table_help('#contributionResults .metric-table', '08-contributions-table-help')
            if target == 'chartPanel':
                breakdown = page.locator('#chartBreakdown')
                previous_breakdown = breakdown.input_value()
                if breakdown.is_visible() and breakdown.is_enabled():
                    breakdown.select_option('organizations')
                    record('08-department-totals', page.locator('#toolsPage'))
                table_help('#chartOrgTotals .metric-table', '08-department-totals-table-help')
                if breakdown.input_value() != previous_breakdown:
                    breakdown.select_option(previous_breakdown)
        close_panel(analytics)
        analytics.click()
        panel('chartPanel')
        analytics.click()
        expect(page.locator('#toolsPage')).not_to_be_visible()
        focused(analytics)

        settings = page.locator('#refSettings')
        settings.click()
        panel('storage')
        page.locator('#appearanceTheme').select_option('light')
        page.locator('#appearanceSize').select_option('xlarge')
        update_notes = page.locator('#appUpdateWhatsNew')
        available_notes = bool(update_notes.count() and update_notes.is_visible() and update_notes.is_enabled())
        inventory.append({'step': '09-update-notes-availability', 'available': available_notes,
                          'reason': None if available_notes else 'No new version notes are offered in this session; installation is not exercised.'})
        if available_notes:
            update_notes.click()
            notes_dialog = page.locator('#appUpdateNotes')
            modal(notes_dialog)
            record('09-update-notes', notes_dialog)
            page.locator('#closeUpdateNotes').click()
            expect(notes_dialog).not_to_be_visible()
            focused(update_notes)
            update_notes.click()
            expect(notes_dialog).to_be_visible()
            page.keyboard.press('Escape')
            expect(notes_dialog).not_to_be_visible()
            focused(update_notes)
        record('09-settings-large', page.locator('#storage'))
        page.locator('#appearanceSize').select_option(initial_size)
        page.locator('#appearanceTheme').select_option(initial_theme)
        close_panel(settings)
        settings.click()
        panel('storage')
        page.locator('#appearanceTheme').focus()
        page.keyboard.press('Escape')
        expect(page.locator('#toolsPage')).not_to_be_visible()
        focused(settings)

        if has_reports:
            # The unchanged daily fixture is intentionally included by selecting
            # the native "All works" filter through the preserved workspace.
            trigger, sheet = open_menu()
            original = sheet.get_by_role('button', name='Исходный макет', exact=True)
            original.click()
            workspace = page.locator('#refWorkspaceDialog')
            expect(workspace).to_be_visible(timeout=TIMEOUT)
            workspace.locator('.tabs [data-view="changes"]').click()
            expect(page.locator('#changes')).to_be_visible()
            latest = page.evaluate('''() => window.desktopAPI.unifiedPeriodOptions(
              window.unifiedUI.revision, 'day', null, null, 1)''')['max']
            assert latest, 'The real daily import must provide a latest report date'
            page.locator('#changesType').select_option('day')
            page.locator('#changesMethod').select_option('previous')
            expect(page.locator('#changesTo')).to_have_attribute('type', 'date')
            page.locator('#changesTo').fill(latest)
            more = page.locator('#changesMoreOptions')
            if not more.evaluate('(node) => node.open'):
                more.locator(':scope > summary').click()
            if page.locator('#changesSearch').input_value():
                page.locator('#changesSearch').fill('')
            page.locator('#changesMode').select_option('all')
            page.locator('#changesDirection').select_option('all')
            work = page.locator('#changesResults .service-link').first
            expect(work).to_be_visible(timeout=TIMEOUT)
            history_close(work, 'changeDetails', 'changeDetailContent', 'closeChangeDetails', '09-native-change-history')
            expect(workspace).to_be_visible()
            page.locator('#refWorkspaceClose').click()
            focused(original)
            close_sheet(trigger)
        settings.click()
        panel('storage')
        settings.click()
        expect(page.locator('#toolsPage')).not_to_be_visible()
        focused(settings)

        # Shared date/unit and picker dialogs are tested from the actual toolbar.
        fold = page.locator('#reportShell details')
        if not fold.evaluate('(node) => node.open'):
            fold.locator(':scope > summary').click()
        types = page.locator('#reportShell [data-filter="type"]')
        modes = page.locator('#reportShell [data-filter="mode"]')
        types.get_by_role('button', name='День', exact=True).click()
        modes.get_by_role('button', name='Диапазон', exact=True).click()
        calendar(page.locator('#reportShell [data-date="start"]'), '10-shared-day', 'days', commit=True)
        for name, mode in [('Неделя', 'days'), ('Месяц', 'months'), ('Год', 'years')]:
            types.get_by_role('button', name=name, exact=True).click()
            calendar(page.locator('#reportShell [data-date="start"]'), '11-shared-' + mode + '-' + name, mode)
        calendar(page.locator('#reportShell .filter-base-edit'), '12-baseline', 'years')
        baseline = page.locator('#sum-base')
        baseline_before = baseline.input_value()
        baseline.select_option('custom')
        expect(page.locator('dialog.period-calendar[open]')).to_have_count(1)
        page.locator('dialog.period-calendar[open]').get_by_role('button', name='Отмена', exact=True).click()
        expect(baseline).to_have_value(baseline_before)
        focused(baseline)

        for kind in ['orgs', 'works']:
            picker = page.locator(f'#reportShell [data-pick="{kind}"]')
            before = picker.inner_text()
            picker.click()
            sheet = page.locator('#sheet')
            modal(sheet)
            checkboxes = sheet.locator('input[type="checkbox"]')
            if checkboxes.count():
                checkboxes.first.set_checked(not checkboxes.first.is_checked())
                expect(picker).to_have_text(before)
            record('13-picker-' + kind, sheet)
            close_sheet(picker)
            expect(picker).to_have_text(before)
            picker.click()
            close_sheet(picker, 'escape')
            picker.click()
            page.locator('#sheet').get_by_role('button', name='Применить', exact=True).click()
            expect(page.locator('#sheet')).not_to_be_visible()
            focused(picker)
            expect(picker).to_have_text(before)

        page.keyboard.press('Escape')
        assert not fold.evaluate('(node) => node.open')
        focused(fold.locator(':scope > summary'))
        fold.locator(':scope > summary').click()

        # Main tabs, help and sensitivity are ordinary route buttons.
        page.locator('#sharedHeader').get_by_role('button', name='Анализ изменений', exact=True).click()
        expect(page.locator('#s2')).to_be_visible(timeout=TIMEOUT)
        expect(page.locator('#s1')).not_to_be_visible()
        sensitivity = page.locator('#s2').get_by_role('button', name='Чувствительность:', exact=False)
        before = sensitivity.inner_text()
        sensitivity.click()
        sheet = page.locator('#sheet')
        modal(sheet)
        slider = sheet.locator('input[type="range"]')
        slider.focus()
        slider.press('End')
        expect(sensitivity).to_have_text(before)
        record('14-sensitivity-draft', sheet)
        close_sheet(sensitivity)
        expect(sensitivity).to_have_text(before)
        sensitivity.click()
        page.locator('#sheet').get_by_role('button', name='Применить', exact=True).click()
        expect(page.locator('#sheet')).not_to_be_visible()
        focused(sensitivity)
        expect(sensitivity).to_have_text(before)
        help_button = page.locator('#s2').get_by_role('button', name='Как читать результаты?', exact=True)
        help_button.click()
        modal(page.locator('#sheet'))
        record('15-changes-help', page.locator('#sheet'))
        close_sheet(help_button, 'escape')
        record('16-changes', page.locator('#s2'))
        page.locator('#sharedHeader').get_by_role('button', name='Сводка', exact=True).click()
        expect(page.locator('#s1')).to_be_visible()

        # Layout checks include classic Windows scrollbars (clientWidth, not innerWidth).
        for width, height in [(1440, 1000), (768, 1024), (390, 844), (320, 568)]:
            page.set_viewport_size({'width': width, 'height': height})
            assert page.evaluate('document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1'), width
            record(f'17-summary-{width}', page.locator('#s1'))
        page.set_viewport_size({'width': 1440, 'height': 1000})
        page.locator('html').evaluate('(node) => node.style.zoom = "1.3"')
        assert page.evaluate('document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1')
        record('18-summary-130-percent', page.locator('#s1'))
        page.locator('html').evaluate('(node, zoom) => node.style.zoom = zoom', initial_zoom)
        assert page.evaluate('window.unifiedUI.revision') == initial_revision, 'Read-only shell inventory changed storage'
    finally:
        # A failing assertion must not leave zoom/viewport settings for later native tests.
        page.locator('html').evaluate('(node, zoom) => node.style.zoom = zoom', initial_zoom)
        if initial_viewport is not None:
            page.set_viewport_size(initial_viewport)
        else:
            # Restore the actual window size before clearing responsive emulation.
            page.set_viewport_size(initial_window_size)
            session = page.context.new_cdp_session(page)
            try:
                session.send('Emulation.clearDeviceMetricsOverride')
            finally:
                session.detach()
