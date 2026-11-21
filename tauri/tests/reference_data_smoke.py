"""Real installed SQL data proof for the reference shell.

Run after import_year/check_report_views, before replacement/renaming/restoration.
The isolated fixture has 366 daily 2024 reports, four organizations, one real
work (Альфа) per organization/day, count .5 and amount 5 per row. No database
report/settings writes, API replacements, synthetic browser results or network
are used here. Native helpers may create and remove their own filter records.
CSV exporters are invoked through their real IPC API into a temporary folder;
the native Save dialog is not substituted. Only window.print is temporarily
replaced to avoid opening the OS print dialog while checking print CSS.
"""
import csv
import io
import json
import math
from pathlib import Path
import re
import tempfile

from playwright.sync_api import Error as PlaywrightError, expect

TIMEOUT = 90_000


def _number(text):
    raw = re.sub(r'[^0-9,.\-]', '', text.replace('−', '-'))
    return float(raw.replace(',', '.')) if raw else None


def _api(page, method, *args):
    return page.evaluate('({method,args}) => window.desktopAPI[method](...args)',
                         {'method': method, 'args': args})



def _check_detail_exports(page, scratch, scope, evidence):
    """Check the six real native detail tables on the known complete intervals."""
    req = {
        'revision': scope['revision'], 'source': 'daily', 'displayType': 'day',
        'organizations': scope['organizations'], 'selectedNames': [],
        'query': {'method': 'range', 'from': scope['from'], 'to': scope['to'],
                  'percent': 30, 'min_count': 5, 'min_amount': 1000},
    }
    before = {'from': scope['baseFrom'], 'to': scope['baseTo']}
    checks = {}
    for section in ['comparison', 'contributions', 'organizations', 'structure', 'usual', 'regularity']:
        payload = {'request': req, 'section': section, 'metric': 'amount'}
        if section == 'organizations':
            payload['baseOrg'] = req['organizations'][0]
        elif section in ('usual', 'regularity'):
            payload.update(window=4, options={'status': 'all', 'search': 'Альфа'})
            if section == 'regularity':
                payload['options']['baseline'] = before
            else:
                payload['options']['range_window'] = True
        else:
            payload['before'] = before
            if section == 'structure':
                payload['limit'] = 10
        path = scratch / ('detail-' + section + '.csv')
        _api(page, 'referenceDetailCsv', payload, str(path))
        raw = path.read_bytes()
        assert raw.startswith(b'\xef\xbb\xbf')
        records = list(csv.reader(io.StringIO(raw.decode('utf-8-sig')), delimiter=';'))
        rows = [r for r in records if r and r[0] == 'Альфа']
        assert len(rows) == 1, (section, records)
        row = rows[0]
        if section in ('comparison', 'contributions', 'structure'):
            assert [_number(row[1]), _number(row[2])] == [620, 580], (section, row)
            if section != 'structure':
                assert _number(row[3]) == -40, row
            if section == 'contributions':
                assert _number(row[-1]) == 100, row
        elif section == 'organizations':
            assert [_number(n) for n in row[1:]] == [145] + [145, 0, 0] * 3, row
        elif section == 'usual':
            assert _number(row[2]) == 580, row
        else:
            assert [_number(n) for n in row[1:5]] == [29, 100, 31, 100], row
        checks[section] = row
    evidence['csv']['referenceDetailCsv'] = checks


def _check_trend_exports(page, scratch, scope, evidence):
    """Use the same monthly bucket as the selected reference filter scope."""
    assert (scope['displayType'], scope['from'], scope['to']) == (
        'month', '2024-02-01', '2024-02-29'), scope
    request = {
        'revision': scope['revision'], 'source': 'daily',
        'displayType': scope['displayType'],
        'organizations': scope['organizations'], 'selectedNames': [],
        'query': {'method': 'range', 'from': scope['from'], 'to': scope['to'],
                  'percent': 30, 'min_count': 5, 'min_amount': 1000},
    }
    checks = {}
    for table in ['works', 'organizations', 'organizationTotals']:
        metrics = ['amount'] if table == 'works' else ['amount', 'count', 'average']
        options = {'table': table, 'metrics': metrics, 'hideEmpty': False}
        path = scratch / ('trend-' + table + '.csv')
        _api(page, 'referenceTrendCsv', request, options, str(path))
        raw = path.read_bytes()
        assert raw.startswith(b'\xef\xbb\xbf')
        records = list(csv.reader(io.StringIO(raw.decode('utf-8-sig')), delimiter=';'))
        if table == 'works':
            assert records[0][1:] == ['2024-02'], records
            assert [row for row in records[1:] if row and row[0] == 'Альфа'] == [['Альфа', '580']], records
            assert records[1] == ['Итого по выбранным работам', '580'], records
        elif table == 'organizations':
            assert records[0][2:] == ['2024-02'], records
            data = [row for row in records[1:] if row and row[0] in scope['organizations']]
            assert len(data) == 12, records
            expected = {'Сумма': 145, 'Количество / объём': 14.5,
                        'Сумма / количество (объём)': 10}
            assert {(row[0], row[1]) for row in data} == {
                (org, metric) for org in scope['organizations'] for metric in expected}, records
            assert all(len(row) == 3 and _number(row[2]) == expected[row[1]]
                       for row in data), records
        else:
            data = records[1:]
            assert len(data) == 4 and {row[0] for row in data} == set(scope['organizations']), records
            assert all([_number(value) for value in row[1:4]] == [145, 14.5, 10]
                       and row[4] == 'Полные данные' for row in data), records
        try:
            _api(page, 'referenceTrendCsv', request, options, str(path))
        except PlaywrightError:
            pass
        else:
            raise AssertionError('Native trend CSV export overwrote a scratch file')
        assert path.read_bytes() == raw
        checks[table] = records
    evidence['csv']['referenceTrendCsv'] = checks


def choose_calendar_date(page, trigger, value):
    trigger.click()
    dialog = page.locator('dialog.period-calendar[open]')
    expect(dialog).to_be_visible(timeout=TIMEOUT)
    grid = dialog.locator('.cal-grid')
    while grid.get_attribute('data-mode') != 'years':
        dialog.locator('.cal-toolbar > button').nth(1).click()
    year = int(value[:4])
    for _ in range(30):
        years = [int(v[:4]) for v in grid.locator('button[data-stamp]').evaluate_all(
            'nodes => nodes.map(node => node.dataset.stamp)')]
        if year in years:
            break
        dialog.get_by_role('button', name='Назад по календарю' if year < min(years)
                           else 'Вперёд по календарю', exact=True).click()
    else:
        raise AssertionError('Calendar could not reach fixture year ' + value)
    grid.locator(f'button[data-stamp="{value[:4]}-01-01"]').click()
    if grid.get_attribute('data-mode') == 'months':
        grid.locator(f'button[data-stamp="{value[:7]}-01"]').click()
    if grid.get_attribute('data-mode') == 'days':
        grid.locator(f'button[data-stamp="{value}"]').click()
    dialog.get_by_role('button', name='Подтвердить', exact=True).click()
    expect(page.locator('dialog.period-calendar[open]')).to_have_count(0)


def check_reference_data(page, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    snapshot = _api(page, 'nativeSettingsBootstrap')
    revision, organizations = snapshot['revision'], snapshot['org_names']
    assert snapshot['reports'] == 366 and len(organizations) == 4, snapshot
    original = _api(page, 'managementReportSummary', revision, {'report_type': 'day'})
    assert original['reports'] == 366 and math.isclose(original['amount'], 7320), original
    assert page.evaluate('window.unifiedOriginal.state.reports.length') == 0
    evidence = {'revision': revision, 'organizations': organizations, 'checks': [], 'csv': {}}
    initial_viewport = page.viewport_size

    def record(name, root='#s1'):
        page.locator(root).scroll_into_view_if_needed()
        if name == '01-complete-current-and-base':
            page.evaluate('window.scrollTo(0, 0)')
        page.screenshot(path=str(output / ('reference-data-' + name + '.png')))
        evidence['checks'].append(name)
        (output / 'reference-data-proof.json').write_text(
            json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')

    def scope():
        return page.evaluate('window.referenceFilters.scope')

    def settled(screen=None):
        page.wait_for_function('''() => window.referenceFilters.scope &&
            document.querySelector('#s1').getAttribute('aria-busy') !== 'true' &&
            (document.querySelector('#s2').hidden ||
             document.querySelector('#s2').getAttribute('aria-busy') !== 'true')''', timeout=TIMEOUT)
        if screen:
            expect(page.locator(screen)).to_be_visible(timeout=TIMEOUT)

    def navigate(name):
        page.locator('#sharedHeader .main-nav').get_by_role('button', name=name, exact=True).click()
        settled('#s2' if name == 'Анализ изменений' else '#s1')

    def fold():
        root = page.locator('#reportShell .filter-fold')
        if not root.evaluate('(node) => node.open'):
            root.locator(':scope > summary').click()

    def select_period(unit, start, end=None, baseline='prev'):
        fold()
        page.locator('#reportShell [data-filter="type"]').get_by_role('button', name=unit, exact=True).click()
        page.locator('#reportShell [data-filter="mode"]').get_by_role(
            'button', name='Диапазон' if end else 'Один период', exact=True).click()
        choose_calendar_date(page, page.locator('#reportShell [data-date="start"]'), start)
        settled()
        if end:
            choose_calendar_date(page, page.locator('#reportShell [data-date="end"]'), end)
            settled()
        page.locator('#sum-base').select_option(baseline)
        settled()
        current = scope()
        assert current['displayFrom'] == start, current
        return current

    def summary(amount, count, average=10):
        values = page.locator('#s1 .metric-value').all_text_contents()
        assert len(values) == 3, values
        actual = [_number(v) for v in values]
        expected = [amount, count, average]
        for got, want in zip(actual, expected):
            assert got is None if want is None else got is not None and math.isclose(got, want, abs_tol=.005), (actual, expected)

    def request(kind='all', **overrides):
        current = scope()
        result = {'revision': revision, 'reportType': current['type'],
                  'from': current['from'], 'to': current['to'],
                  'baseFrom': current['baseFrom'], 'baseTo': current['baseTo'],
                  'organizations': current['organizations'], 'metric': current['metric'],
                  'percent': 30, 'minCount': 5, 'minAmount': 1000,
                  'kind': kind, 'search': '', 'sort': 'delta', 'page': 0}
        result.update(overrides)
        return result

    def thresholds(percent):
        page.locator('#s2 .filter-threshold').click()
        dialog = page.locator('#sheet')
        expect(dialog).to_be_visible(timeout=TIMEOUT)
        slider = dialog.locator('input[type="range"]')
        slider.focus()
        slider.press('Home' if percent == 0 else 'End')
        assert slider.input_value() == str(percent)
        for field in dialog.locator('input[type="number"]').all():
            field.fill('0')
        dialog.get_by_role('button', name='Применить', exact=True).click()
        expect(dialog).not_to_be_visible(timeout=TIMEOUT)
        settled('#s2')

    def print_view(screen):
        # Capture the dedicated print document synchronously while it exists.
        # Change-table printing prepares all filtered SQL pages asynchronously.
        title = ('Сводка выполненных работ' if screen == '#s1' else
                 page.locator('#s2 .changes-heading h2').inner_text())
        periods = page.locator(screen + ' .report-range span').all_text_contents()
        source = '#s1 .data-card table tbody tr' if screen == '#s1' else '#s2 .changes-table tbody tr'
        expected_rows = page.locator(source).evaluate_all(
            '(rows) => rows.map(row => [...row.cells].slice(0, row.closest("#s1") ? 4 : 6).map(cell => cell.textContent))')
        expected_values = page.locator('#s1 .metric-value').all_text_contents() if screen == '#s1' else None
        positions = re.search(r'Позиции\s+(\d+)[–—-](\d+)\s+из\s+(\d+)', page.locator('#s2 .pager-footer').inner_text()) if screen == '#s2' else None
        first_row, total_rows = (int(positions[1]) - 1, int(positions[3])) if positions else (0, len(expected_rows))
        page.emulate_media(media='print')
        page.evaluate("""() => {window.__referenceSmokePrint=window.print;
            window.__referenceSmokePrintFrames=[];window.print=()=>{
              const visible=id=>{const node=document.querySelector(id);return Boolean(node &&
                node.getClientRects().length && getComputedStyle(node).visibility!=='hidden');};
              const root=document.getElementById('referencePrintDocument');
              const rows=selector=>Array.from(root?.querySelectorAll(selector)||[],row=>
                Array.from(row.cells,cell=>cell.textContent));
              window.__referenceSmokePrintFrames.push({screen:document.body.dataset.referencePrint,
                visible:Object.fromEntries(['#s1','#s2','#reportShell','#sharedHeader','#toolsPage','#referencePrintDocument']
                  .map(id=>[id,visible(id)])),title:root?.querySelector('h1')?.textContent,
                organizations:rows('.print-organizations tbody tr'),changes:rows('.print-changes tbody tr'),
                observed:rows('.print-observed tbody tr'),comparison:rows('.print-comparison tbody tr'),
                context:root?.textContent});
            };}""")
        try:
            # Print CSS hides the trigger itself. Invoke that production element's
            # actual handler, then await its bounded native SQL reads before capture.
            page.locator(screen).get_by_role('button', name='Печать / PDF', exact=True, include_hidden=True).first.evaluate('(node) => node.click()')
            page.wait_for_function('() => window.__referenceSmokePrintFrames.length === 1', timeout=TIMEOUT)
            frames = page.evaluate('window.__referenceSmokePrintFrames')
            assert len(frames) == 1 and frames[0]['screen'] == 'document', frames
            frame = frames[0]
            assert frame['visible'] == {key: key == '#referencePrintDocument' for key in frame['visible']}, frame
            assert frame['title'] == title, frame
            assert len(periods) >= 2 and 'Анализируемый период: ' + periods[0] in frame['context'], frame
            assert 'Базовый период: ' + periods[1].removeprefix('Сравнение с ') in frame['context'], frame
            rows = frame['organizations' if screen == '#s1' else 'changes']
            assert len(rows) == total_rows and rows[first_row:first_row + len(expected_rows)] == expected_rows, frame
            assert len({tuple(row[:2]) for row in rows}) == len(rows), frame
            if screen == '#s2':
                assert 'Строк: ' + str(total_rows) + ' ·' in frame['context'], frame
            assert len(frame['comparison']) == 3, frame
            assert 'Область работ: Все работы' in frame['context'], frame
            if expected_values is not None:
                assert [row[1] for row in frame['observed']] == expected_values, frame
            page.wait_for_function("() => !document.body.hasAttribute('data-reference-print') && !document.getElementById('referencePrintDocument')", timeout=TIMEOUT)
            evidence['checks'].append('print-media-' + screen[1:])
            evidence.setdefault('print', {})[screen] = frame
        finally:
            page.emulate_media(media='screen')
            page.evaluate("""() => {window.print=window.__referenceSmokePrint;
                delete window.__referenceSmokePrint;delete window.__referenceSmokePrintFrames;}""")

    def open_quality(trigger):
        trigger.click()
        sheet = page.locator('#sheet')
        expect(sheet).to_contain_text('Состав сравнения')
        sheet.get_by_role('button', name='Проверить отчёты', exact=True).click()
        expect(sheet).to_contain_text('Качество отчётов')
        return sheet

    def csv_export(method, path, *, empty=False):
        _api(page, method, request(), str(path))
        raw = path.read_bytes()
        assert raw.startswith(b'\xef\xbb\xbf')
        rows = list(csv.reader(io.StringIO(raw.decode('utf-8-sig')), delimiter=';'))
        if method == 'referenceSummaryCsv':
            assert rows[0][1:] == [scope()['from'], scope()['to']], rows
            assert rows[1][1:] == [scope()['baseFrom'], scope()['baseTo']], rows
            header = next(i for i, row in enumerate(rows) if row[:2] == ['Подразделение', 'Сумма'])
            data = rows[header + 1:]
            assert len(data) == 4 and {r[0] for r in data} == set(organizations), rows
            if empty:
                assert all(row[1:5] == ['', '', '', ''] and row[5] == 'нет данных' for row in data), rows
            else:
                assert all(_number(row[1]) == 145 and _number(row[2]) == 14.5 for row in data), rows
                assert all(_number(row[3]) == 25 for row in data), rows
        else:
            header = next(i for i, row in enumerate(rows) if row[:2] == ['Работа', 'Подразделение'])
            data = rows[header + 1:]
            assert len(data) == (0 if empty else 4), rows
            if not empty:
                assert all(row[0] == 'Альфа' and _number(row[2]) == 155 and
                           _number(row[3]) == 145 and _number(row[4]) == -10 for row in data), rows
        sample = next(row for row in rows if row and row[0] == 'Сопоставимые подразделения')
        assert sample[1:3] == [('0' if empty else '4'), '4'], rows
        assert sample[3] == ('' if empty else ', '.join(organizations)), rows
        try:
            _api(page, method, request(), str(path))
        except PlaywrightError:
            pass
        else:
            raise AssertionError('Native CSV export overwrote an existing scratch file')
        assert path.read_bytes() == raw
        evidence['csv'][method + ('-empty' if empty else '-data')] = rows

    try:
        page.set_viewport_size({'width': 1440, 'height': 1000})
        navigate('Сводка')
        fold()
        page.locator('#referenceSource').select_option('daily')
        # Use the real organization picker to establish all four organizations.
        page.locator('#reportShell [data-pick="orgs"]').click()
        dialog = page.locator('#sheet')
        dialog.get_by_role('button', name='Выбрать все', exact=True).click()
        dialog.get_by_role('button', name='Применить', exact=True).click()
        page.locator('#reportShell [data-measure]').select_option('amount')
        select_period('День', '2024-02-02')
        current = scope()
        assert (current['from'], current['to'], current['baseFrom'], current['baseTo']) == (
            '2024-02-02', '2024-02-02', '2024-02-01', '2024-02-01'), current
        summary(20, 2)
        expect(page.locator('#s1 .metric-sample').first).to_have_text('по 4 из 4')
        expect(page.locator('#s1 .quality-notice')).not_to_be_visible()
        assert page.locator('#s1 table tbody tr').count() == 4
        assert all('частичные' not in text for text in page.locator('#s1 table tbody tr').all_text_contents())
        oracle = _api(page, 'referenceChanges', request())
        assert oracle['comparable'] == organizations and oracle['total'] == 4, oracle
        assert all(row['before'] == 5 and row['after'] == 5 and row['delta'] == 0 for row in oracle['rows']), oracle
        complete = _api(page, 'nativeAnalysisCompare', revision, 'day',
                        '2024-02-01', '2024-02-01', '2024-02-02', '2024-02-02', organizations)
        assert all(complete['summary'][part]['complete'] and complete['summary'][part]['missing'] == 0
                   for part in ['before', 'after']), complete
        evidence['complete_comparison'] = oracle
        quality = page.locator('#s1 .metric-sample').first
        sheet = open_quality(quality)
        expect(sheet.locator('td.coverage-ok')).to_have_count(4, timeout=TIMEOUT)
        expect(sheet.locator('.coverage-matrix thead')).to_contain_text('02.02.2024')
        sheet.get_by_role('button', name='База сравнения', exact=True).click()
        expect(sheet.locator('.coverage-matrix thead')).to_contain_text('01.02.2024', timeout=TIMEOUT)
        expect(sheet.locator('td.coverage-ok')).to_have_count(4)
        sheet.get_by_role('button', name='Закрыть', exact=True).click()
        expect(quality).to_be_focused()
        record('01-complete-current-and-base')
        print_view('#s1')

        # February and January are independently complete, deliberately unequal intervals.
        current = select_period('Месяц', '2024-02-01', baseline='prevMonth')
        assert (current['to'], current['baseFrom'], current['baseTo']) == ('2024-02-29', '2024-01-01', '2024-01-31'), current
        summary(580, 58)
        expect(page.locator('#reportShell .period-length-note')).to_be_visible()
        navigate('Анализ изменений')
        expect(page.locator('#s2 .table-empty')).to_contain_text('Значимых изменений нет')
        expect(page.locator('#s2 .match-note')).to_contain_text('Значимые позиции · 0')
        page.locator('#s2 .match-note').get_by_role('button', name='Показать все 4', exact=True).click()
        settled('#s2')
        expect(page.locator('#s2 .changes-table tbody tr')).to_have_count(4)
        for cells in page.locator('#s2 .changes-table tbody tr').all():
            values = cells.locator('td').all_text_contents()
            assert values[0] == 'Альфа' and values[1] in organizations, values
            assert [_number(v) for v in values[2:5]] == [155, 145, -10], values
            assert math.isclose(_number(values[5]), -6.5, abs_tol=.01), values
        expect(page.locator('#s2 .pager-footer')).to_contain_text('Позиции 1–4 из 4')
        expect(page.locator('#s2 [data-page-action="next"]')).to_have_attribute('aria-disabled', 'true')
        search = page.locator('#s2 input[placeholder]')
        search.fill('no such fixture work')
        expect(page.locator('#s2 .table-empty')).to_contain_text('Совпадений нет', timeout=TIMEOUT)
        expect(page.locator('#s2 [data-export-changes]')).to_be_disabled()
        search.fill('Альфа')
        expect(page.locator('#s2 .changes-table tbody tr')).to_have_count(4, timeout=TIMEOUT)
        for sort in ['name', 'percent', 'delta']:
            page.locator('#s2 [data-sort]').select_option(sort)
            settled('#s2')
            expect(page.locator('#s2 .changes-table tbody tr')).to_have_count(4)
        search.fill('')
        thresholds(0)
        expect(page.locator('#s2 .kpi [data-count]').nth(1)).to_have_text('▼ 4')
        page.locator('#s2 .kpi').nth(1).click()
        settled('#s2')
        expect(page.locator('#s2 .changes-table tbody tr')).to_have_count(4)
        thresholds(200)
        expect(page.locator('#s2 .kpi [data-count]').nth(1)).to_have_text('▼ 0')
        expect(page.locator('#s2 .changes-table tbody tr')).to_have_count(0)
        thresholds(0)
        expect(page.locator('#s2 .match-note').get_by_role('button', name='Показать все 4', exact=True)).to_have_count(0)
        page.locator('#s2 .match-note').get_by_role('button', name='Только значимые', exact=True).click()
        expect(page.locator('#s2 .match-note')).to_contain_text('Значимые позиции · 4')
        settled('#s2')
        print_view('#s2')
        record('02-unequal-interval-changes', '#s2')
        with tempfile.TemporaryDirectory(prefix='analiz-reference-data-') as scratch:
            scratch = Path(scratch)
            csv_export('referenceChangesCsv', scratch / 'changes.csv')
            csv_export('referenceSummaryCsv', scratch / 'summary.csv')
            _check_detail_exports(page, scratch, scope(), evidence)
            _check_trend_exports(page, scratch, scope(), evidence)
            # Partially observed current interval and wholly absent baseline.
            navigate('Сводка')
            select_period('День', '2023-12-31', '2024-01-01')
            summary(20, 2)
            expect(page.locator('#s1 .metric-sample').first).to_have_text('по 0 из 4')
            expect(page.locator('#s1 .quality-notice')).to_contain_text('Итоги частичные')
            assert all('частичные данные' in text for text in page.locator('#s1 table tbody tr').all_text_contents())
            record('03-partial-adjacent-year')
            select_period('День', '2023-12-29', '2023-12-30')
            summary(None, None, None)
            csv_export('referenceSummaryCsv', scratch / 'empty-summary.csv', empty=True)
            csv_export('referenceChangesCsv', scratch / 'empty-changes.csv', empty=True)
            navigate('Анализ изменений')
            expect(page.locator('#s2 .table-empty')).to_contain_text('Нет сопоставимых позиций')
            expect(page.locator('#s2 [data-export-changes]')).to_be_disabled()
            record('04-empty-not-zero', '#s2')
            # An unset organization selection is valid UI state, not an export of zeros.
            fold()
            page.locator('#reportShell [data-pick="orgs"]').click()
            dialog = page.locator('#sheet')
            dialog.get_by_role('button', name='Снять выбор', exact=True).click()
            dialog.get_by_role('button', name='Применить', exact=True).click()
            expect(page.locator('#s2 .table-empty')).to_contain_text('Выберите периоды и подразделения')
            bad = request(organizations=[])
            path = scratch / 'unset.csv'
            try:
                _api(page, 'referenceSummaryCsv', bad, str(path))
            except PlaywrightError:
                pass
            else:
                raise AssertionError('Unset organizations produced an invented CSV summary')
            assert not path.exists()
            page.locator('#reportShell [data-pick="orgs"]').click()
            dialog.get_by_role('button', name='Выбрать все', exact=True).click()
            dialog.get_by_role('button', name='Применить', exact=True).click()

        # Real year matrix has twelve 31-day pages, without adding test reports.
        navigate('Сводка')
        select_period('Год', '2024-01-01', baseline='year')
        summary(7320, 732)
        trigger = page.locator('#s1 .metric-sample').first
        sheet = open_quality(trigger)
        expect(sheet).to_be_visible()
        periods = sheet.locator('[data-sql-pager="coverage-periods"]')
        expect(periods).to_have_attribute('data-sql-window', '0', timeout=TIMEOUT)
        expect(sheet.locator('td.coverage-ok')).to_have_count(124)
        # Jump directly to December, whose final window contains 25 days.
        periods.locator('[data-page-action="choose"]').click()
        periods.locator('[data-page-number="12"]').click()
        expect(periods).to_have_attribute('data-sql-window', '11', timeout=TIMEOUT)
        expect(sheet.locator('.coverage-matrix thead')).to_contain_text('31.12.2024')
        expect(sheet.locator('td.coverage-ok')).to_have_count(100)
        # Cross one SQL boundary backwards, then activate the first cell. Its
        # automatic scroll into view must keep the selected November window.
        periods.locator('[data-page-action="previous"]').click()
        expect(periods).to_have_attribute('data-sql-window', '10', timeout=TIMEOUT)
        expect(sheet.locator('td.coverage-ok')).to_have_count(124)
        november_cell = sheet.locator('.coverage-cell-action').first
        november_cell.click()
        inspection = sheet.locator('.coverage-inspection')
        expect(inspection.locator('.coverage-report')).to_have_count(1, timeout=TIMEOUT)
        expect(inspection.get_by_role('heading').first).to_contain_text('06.11.2024')
        expect(periods).to_have_attribute('data-sql-window', '10')
        inspection.get_by_role('button', name='Закрыть подробности', exact=True).click()
        expect(november_cell).to_be_focused()
        periods.locator('[data-page-action="choose"]').click()
        periods.locator('[data-page-number="1"]').click()
        expect(periods).to_have_attribute('data-sql-window', '0', timeout=TIMEOUT)
        expect(sheet.locator('.coverage-matrix thead')).to_contain_text('01.01.2024')
        expect(sheet.locator('td.coverage-ok')).to_have_count(124)
        cell = sheet.locator('.coverage-cell-action').first
        cell.click()
        inspection = sheet.locator('.coverage-inspection')
        expect(inspection).to_be_visible()
        expect(inspection.locator('.coverage-report')).to_have_count(1, timeout=TIMEOUT)
        detail = inspection.locator('.coverage-report > details')
        detail.locator(':scope > summary').click()
        expect(inspection.locator('.coverage-source-evidence')).to_contain_text('Лист отчёта: Q1', timeout=TIMEOUT)
        expect(inspection).to_contain_text('Признак явного нуля: нет.')
        expect(inspection).to_contain_text('Полнота источника не установлена.')
        source_notes = inspection.get_by_role('button', name='Замечания источника', exact=True)
        evidence['source_notes_available'] = bool(source_notes.count())
        if source_notes.count():
            source_notes.click()
            expect(inspection).to_contain_text('Признак нуля отчёта с общими замечаниями', timeout=TIMEOUT)
            inspection.get_by_role('button', name='Замечания выбранного отчёта', exact=True).click()
            expect(inspection).to_contain_text('Признак явного нуля: нет.', timeout=TIMEOUT)
        expect(inspection.get_by_role('button', name='Следующие замечания →', exact=True)).to_have_count(0)
        expect(inspection.locator('[data-sql-pager="coverage-notes"] [data-page-action="next"]')).to_have_attribute('aria-disabled', 'true')
        evidence['diagnostic_paging'] = 'No further diagnostic rows in the zero-issue fixture.'
        record('05-current-quality-evidence', '#sheet')
        detail.locator(':scope > summary').focus()
        page.keyboard.press('Escape')
        assert not detail.evaluate('(node) => node.open')
        expect(inspection).to_be_visible()
        expect(sheet).to_be_visible()
        page.keyboard.press('Escape')
        expect(inspection).not_to_be_visible()
        expect(sheet).to_be_visible()
        expect(cell).to_be_focused()
        sheet.get_by_role('button', name='База сравнения', exact=True).click()
        expect(sheet.locator('td.coverage-missing')).to_have_count(124, timeout=TIMEOUT)
        sheet.locator('.coverage-cell-action').first.click()
        expect(sheet.locator('.coverage-inspection')).to_contain_text('Нет сохранённого отчёта', timeout=TIMEOUT)
        sheet.get_by_role('button', name='Закрыть подробности', exact=True).click()
        expect(sheet).to_be_visible()
        page.keyboard.press('Escape')
        expect(sheet).not_to_be_visible()
        expect(trigger).to_be_focused()
        # Reopening and explicit dismissal use the same normal route.
        open_quality(trigger)
        expect(sheet.locator('td.coverage-ok')).to_have_count(124, timeout=TIMEOUT)
        sheet.get_by_role('button', name='Закрыть', exact=True).click()
        expect(sheet).not_to_be_visible()
        record('06-baseline-quality-and-dismissal')
        assert _api(page, 'nativeSettingsBootstrap')['revision'] == revision
        final = _api(page, 'managementReportSummary', revision, {'report_type': 'day'})
        assert final['reports'] == 366 and math.isclose(final['amount'], 7320), final
        assert page.evaluate('window.unifiedOriginal.state.reports.length') == 0
    finally:
        page.emulate_media(media='screen')
        if page.evaluate('Boolean(window.__referenceSmokePrint)'):
            page.evaluate('window.print=window.__referenceSmokePrint;delete window.__referenceSmokePrint')
        for dialog in ['dialog.period-calendar[open]', '#sheet[open]']:
            if page.locator(dialog).count():
                page.locator(dialog).evaluate('(node) => node.close()')
        # Restore through the real visible reset route; native data are untouched.
        fold()
        page.locator('#reportShell .filter-reset').click()
        page.locator('#sharedHeader .main-nav').get_by_role('button', name='Сводка', exact=True).click()
        if initial_viewport:
            page.set_viewport_size(initial_viewport)
        else:
            # Installed WebView2 connected over CDP has no configured viewport.
            session = page.context.new_cdp_session(page)
            try:
                session.send('Emulation.clearDeviceMetricsOverride')
            finally:
                session.detach()
