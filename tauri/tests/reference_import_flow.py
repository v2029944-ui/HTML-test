"""Installed WebView2 acceptance for the paged folder/manual weekly import path."""
import io
import json
import re
import time
import zipfile
from pathlib import Path
from playwright.sync_api import expect
from reference_file_dialogs import OwnedFileDialogs


def two_sheet_workbook(single):
    """Distinct numeric totals make selecting the wrong sheet observable in SQLite."""
    with zipfile.ZipFile(io.BytesIO(single)) as source:
        files = {name: source.read(name) for name in source.namelist()}
    files['xl/workbook.xml'] = files['xl/workbook.xml'].replace(
        b'</sheets>', b'<sheet name="Second" sheetId="2" r:id="rId2"/></sheets>')
    files['xl/_rels/workbook.xml.rels'] = files['xl/_rels/workbook.xml.rels'].replace(
        b'</Relationships>', b'<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>')
    files['[Content_Types].xml'] = files['[Content_Types].xml'].replace(
        b'</Types>', b'<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>')
    files['xl/worksheets/sheet2.xml'] = re.sub(
        rb'<v>([^<]+)</v>', lambda match: ('<v>'+str(float(match[1])*2)+'</v>').encode(),
        files['xl/worksheets/sheet1.xml'])
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w', zipfile.ZIP_STORED) as archive:
        for name, value in files.items():
            archive.writestr(name, value)
    return output.getvalue()


def check_reference_import_flow(page, temporary, confirmations, output):
    # Import here avoids a module cycle in the installed runner.
    from windows_smoke import api, invoke, navigate, set_folder, wait_for_text, daily_workbook_bytes
    temporary, output = Path(temporary), Path(output)
    folder = temporary / 'import-flow-101'
    folder.mkdir()
    single = daily_workbook_bytes()
    for index in range(100):
        (folder / f'{index:03}.xlsx').write_bytes(single)
    book = folder / 'zz-two-sheet.xlsx'
    book.write_bytes(two_sheet_workbook(single))
    before = api(page, 'nativeSettingsBootstrap')
    assert before['reports'] == 0, 'This acceptance requires an isolated empty database'
    previous = api(page, 'list')['folder']
    navigate(page, 'upload')
    expect(page.locator('#unifiedUploadImport')).to_have_attribute('aria-pressed', 'true')
    expect(page.locator('#uploadStep [aria-current="step"]')).to_contain_text('Файл')
    expect(page.locator('#inspectFileHint')).to_have_text('Сначала выберите файл.')
    expect(page.locator('#uploadReviewCard')).to_be_hidden()
    expect(page.locator('#unifiedImportStatus')).to_be_empty()
    listing = set_folder(page, folder, [p.name for p in folder.glob('*.xlsx')])
    assert len(listing['files']) == 101 and listing['files'][100]['name'] == book.name
    file = listing['files'][100]
    rule = {'format': 'auto', 'type': 'week', 'start': '2031-01-08', 'sheet': 'Second'}
    api(page, 'nativeSetRuleChecked', file['name'], rule, listing['revision'], listing['folder'], file['nativeSignature'])
    page.evaluate('window.unifiedUI.refresh()')
    # The Windows file dialog can return a DOS path instead of the folder's verbatim path.
    # Start with this entry point before the folder row has populated any saved choices.
    dialogs = OwnedFileDialogs(confirmations)
    page.locator('#uploadSourceFile').click()
    expect(page.locator('#desktopSource')).to_be_hidden()
    expect(page.locator('#uploadSourceFile')).to_have_attribute('aria-pressed', 'true')
    page.locator('#excelFile').click()
    title = 'Выберите XLSX для нативного импорта'
    hwnd, owner = dialogs.find(title)
    dialogs.open_file_for_preview(hwnd, owner, title, book, output / 'import-flow-xlsx-picker.png')
    wait_for_text(page, '#excelFileStatus', book.name)
    expect(page.locator('#sheetSelect')).to_have_value('xl/worksheets/sheet2.xml')
    expect(page.locator('#importType')).to_have_value('week')
    expect(page.locator('#importWeek')).to_have_value(rule['start'])
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 90_000)
    expect(page.locator('#uploadStep [aria-current="step"]')).to_contain_text('Сохранение')
    expect(page.locator('#saveImport')).to_have_text('Сохранить в личную базу')
    picker_counts = page.locator('#importPreview .kpi strong').all_text_contents()[:2]
    expect(page.locator('#importPreview')).to_contain_text('2031-01-08 — 2031-02-18')
    expect(page.locator('#sheetSelect')).to_be_enabled()
    page.locator('#importType').select_option('day')
    expect(page.locator('#importPreview')).to_be_empty()
    page.locator('#uploadSourceFolder').click()
    page.locator('#sourceFolderRefresh').click()
    expect(page.locator('#sourceFolderRefresh')).to_be_enabled()
    wait_for_text(page, '#sourceFolderCount', '101')
    pager = page.locator('[data-sql-pager="sourceFolderRows"]')
    pager.locator('[data-page-action="choose"]').click()
    # A file watcher may deliver another scan while the page picker is open.
    page.evaluate('window.unifiedReports.refreshFolder()')
    expect(pager.locator('[data-page-action="choose"]')).to_have_attribute('aria-expanded', 'true')
    pager.locator('[data-page-number="2"]').click()
    expect(pager.locator('[data-page-range]')).to_contain_text('51–100 из 101')
    expect(page.locator('#sourceFolderRows tr')).to_have_count(50)
    pager.locator('[data-page-action="choose"]').click()
    pager.locator('[data-page-number="3"]').click()
    expect(pager.locator('[data-page-range]')).to_contain_text('101–101 из 101')
    row = page.locator('#sourceFolderRows tr').filter(has_text=book.name)
    expect(row).to_be_visible()
    expect(page.locator('#excelFile')).to_be_hidden()
    expect(page.locator('#uploadSourceFolder')).to_have_attribute('aria-pressed', 'true')
    expect(page.locator('#sourceFolderRows tr')).to_have_count(1)
    row.get_by_role('button', name='Открыть для проверки '+book.name, exact=True).click()
    expect(page.locator('#uploadSelectedBook')).to_contain_text(book.name)
    expect(page.locator('#sheetSelect')).to_have_value('xl/worksheets/sheet2.xml')
    expect(page.locator('#importType')).to_have_value('week')
    expect(page.locator('#importWeek')).to_have_value(rule['start'])
    listing = api(page, 'list')
    file = next(f for f in listing['files'] if f['name'] == book.name)
    lease = api(page, 'nativeFolderImportSession', True)
    try:
        auto = api(page, 'nativeFolderImportPreview', {'session': lease, 'folder': listing['folder'],
                   'revision': listing['revision'], 'name': file['name'], 'signature': file['nativeSignature']})
    finally:
        api(page, 'nativeFolderImportSession', False)
    started = time.monotonic()
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 90_000)
    assert auto['reportCount'] == 6 and auto['rowCount'] == 12
    assert auto['periodRange'] == {'kind': 'week', 'from': '2031-01-08', 'to': '2031-02-18'}
    expect(page.locator('#importPreview')).to_contain_text('2031-01-08 — 2031-02-18')
    assert picker_counts == [str(auto['reportCount']), str(auto['rowCount'])], picker_counts
    assert page.locator('#importPreview .kpi strong').first.inner_text() == str(auto['reportCount'])
    assert page.locator('#importPreview .kpi strong').nth(1).inner_text() == str(auto['rowCount'])
    expect(page.locator('#saveDailyFolderRule')).not_to_be_visible()
    # Invalidating parameters must remove the old figures at once and block writes.
    page.locator('#importType').select_option('day')
    expect(page.locator('#importPreview')).to_be_empty()
    expect(page.locator('#saveImport')).to_be_disabled()
    page.locator('#importType').select_option('week')
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 90_000)
    # A different sheet has a deliberately delayed last set. Only the dates
    # and doubled amounts of the newly reviewed second sheet may reach SQLite.
    def choose_date(trigger, value):
        page.locator(trigger).click()
        calendar = page.locator('dialog.period-calendar[open]')
        calendar.locator('[data-stamp="' + value + '"]').click()
        calendar.get_by_role('button', name='Подтвердить', exact=True).click()

    page.locator('#sheetSelect').select_option('xl/worksheets/sheet1.xml')
    choose_date('#importWeek + .ref-calendar-field', '2031-01-01')
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 90_000)
    if not page.locator('#importAdvanced').evaluate('node=>node.open'):
        page.locator('#importAdvanced > summary').click()
    choose_date('#importPeriodDates input[data-ordinal="5"] + .ref-calendar-field', '2031-02-26')
    page.locator('#inspectFile').click()
    expect(page.locator('#importPreview')).to_contain_text('2031-01-01 — 2031-03-04', timeout=90_000)
    page.locator('#sheetSelect').select_option('xl/worksheets/sheet2.xml')
    choose_date('#importWeek + .ref-calendar-field', rule['start'])
    expect(page.locator('#saveImport')).to_be_disabled()
    page.locator('#inspectFile').click()
    expect(page.locator('#importPreview')).to_contain_text('2031-01-08 — 2031-02-18', timeout=90_000)
    for control in ('unifiedAcceptOverlap', 'unifiedAcceptQuality'):
        field = page.locator('#'+control)
        if field.is_visible():
            field.check()
    confirmations.cancel_then_accept(page.locator('#saveImport'),
        'Повторно проверить Excel и атомарно сохранить 6 отчётов')
    wait_for_text(page, '#unifiedImportStatus', 'Сохранено отчётов: 6', 90_000)
    page.get_by_role('button', name='Открыть загруженный период', exact=True).click()
    page.wait_for_function('''() => {const s=window.referenceFilters.scope;
      return s?.type==='week' && s.source==='imported' && s.from==='2031-01-08'
        && s.to==='2031-02-12' && s.displayTo==='2031-02-18'
        && s.baseFrom==='2030-11-27' && s.baseTo==='2031-01-01';}''')
    snapshot = api(page, 'nativeSettingsBootstrap')
    scope = page.evaluate('window.referenceFilters.scope')
    summary = api(page, 'managementReportSummary', snapshot['revision'],
                  {'report_type': scope['type'], 'from': scope['from'], 'to': scope['to']})
    assert summary['reports'] == 6 and abs(summary['amount']-5460) < .000001, summary
    saved = api(page, 'managementReportWindow', snapshot['revision'], {'report_type': 'week'}, 0, 50)
    starts = sorted(r['start'] for r in saved['rows'])
    assert starts == ['2031-01-08', '2031-01-15', '2031-01-22', '2031-01-29', '2031-02-05', '2031-02-12'], starts
    page.screenshot(path=str(output / 'import-flow-second-sheet-week.png'))
    (output / 'import-flow.json').write_text(json.dumps({'folderRows': 101, 'selectedSheet': 'Second',
        'nativePickerPreview': {'sheet': 'Second', 'reportCount': int(picker_counts[0]), 'rowCount': int(picker_counts[1])},
        'automaticPreview': {k: auto[k] for k in ('reportCount', 'rowCount', 'periodRange')},
        'savedWeeklyStarts': starts, 'savedWeeklySummary': summary, 'manualPreviewAndSaveSeconds': time.monotonic()-started,
        'scope': scope}, ensure_ascii=False, indent=2), encoding='utf-8')
    # The SAME valid auto rule must detect a daily book after its source changes.
    # This is preview-only; the six saved weekly reports and revision stay intact.
    from test_daily_wide import wide_xlsx
    book.write_bytes(wide_xlsx(2024))
    navigate(page, 'upload')
    page.locator('#uploadSourceFolder').click()
    page.locator('#sourceFolderRefresh').click()
    expect(page.locator('#sourceFolderRefresh')).to_be_enabled()
    pager = page.locator('[data-sql-pager="sourceFolderRows"]')
    if not page.locator('#sourceFolderRows tr').filter(has_text=book.name).is_visible():
        pager.locator('[data-page-action="choose"]').click()
        pager.locator('[data-page-number="3"]').click()
    expect(pager.locator('[data-page-range]')).to_contain_text('101–101 из 101')
    page.locator('#sourceFolderRows tr').filter(has_text=book.name).get_by_role(
        'button', name='Открыть для проверки '+book.name, exact=True).click()
    expect(page.locator('#importFormat')).to_have_value('daily-wide', timeout=90_000)
    expect(page.locator('#sheetSelect')).not_to_be_visible()
    page.locator('#inspectFile').click()
    wait_for_text(page, '#unifiedImportStatus', 'Проверка завершена', 90_000)
    assert page.locator('#importPreview .kpi strong').first.inner_text() == '366'
    listing = api(page, 'list')
    file = next(f for f in listing['files'] if f['name'] == book.name)
    lease = api(page, 'nativeFolderImportSession', True)
    try:
        auto_daily = api(page, 'nativeFolderImportPreview', {'session': lease, 'folder': listing['folder'],
            'revision': listing['revision'], 'name': file['name'], 'signature': file['nativeSignature']})
    finally:
        api(page, 'nativeFolderImportSession', False)
    assert auto_daily['reportCount'] == 366
    expect(page.locator('#importPreview')).to_contain_text(
        auto_daily['periodRange']['from']+' — '+auto_daily['periodRange']['to'])
    assert api(page, 'nativeRevision') == snapshot['revision']
    page.screenshot(path=str(output / 'import-flow-auto-daily-preview.png'))
    # Remove only the six synthetic reports just created, preserving all pre-test settings.
    api(page, 'unifiedClearReports', snapshot['revision'])
    invoke(page, 'set_folder', {'folder': previous})
    assert page.evaluate('window.unifiedUI.refresh()')['reports'] == 0
    navigate(page, 'upload')
    page.locator('#uploadSourceFolder').click()
    page.locator('#sourceFolderRefresh').click()
