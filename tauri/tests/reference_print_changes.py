"""Check the filtered rows handed to the native print UI on the installed application."""
import json
from playwright.sync_api import expect
from reference_print_probe import probe_reference_print


def check_print_all_changes(page, temporary, output, pid, api, make_workbook, open_day):
    snapshot = api(page, 'nativeSettingsBootstrap')
    names = [f'Позиция {i:02d}' for i in range(1, 14)]
    totals = [130, 1300, 130, 1300 + 1000 * sum(range(1, 14))]
    rows = [['Работа', '2019-02-01', '2019-02-01', '2019-02-02', '2019-02-02'],
            ['', 'Количество', 'Сумма', 'Количество', 'Сумма'],
            [snapshot['org_names'][0], *totals]]
    rows.extend([name, 10, 100, 10, 100 + i * 1000] for i, name in enumerate(names, 1))
    rows.extend([org, 0, 0, 0, 0] for org in snapshot['org_names'][1:])
    rows.append(['Итого', *totals])
    source = temporary / 'print-thirteen.xlsx'
    source.write_bytes(make_workbook(rows))
    options = {'year': 2019, 'sheets': [], 'mappings': {}, 'organizations': snapshot['org_names']}
    preview = api(page, 'nativeWideImportPreview', str(source), options)
    assert preview['reportCount'] == 2 and preview['conflicts']['new_reports'] == 2, preview
    saved = api(page, 'nativeWideImportApply', str(source), options, preview['reviewToken'],
                {'replaceExact': False, 'acceptCrossType': False, 'acceptQuality': False})
    assert saved['reports'] == 2, saved
    page.evaluate('window.unifiedUI.refresh()')
    open_day(page, '2019-02-02')
    page.locator('#sharedHeader .main-nav').get_by_role('button', name='Анализ изменений', exact=True).click()
    page.locator('#s2 [data-sort]').select_option('name')
    on_screen = page.locator('#s2 .changes-card tbody tr')
    expect(on_screen).to_have_count(12)
    expect(on_screen.first).to_contain_text(names[0])
    expect(on_screen.last).to_contain_text(names[11])
    search = page.locator('#s2 input[placeholder]')
    # Capture action availability in the input event, before the debounced SQL read.
    search.evaluate("""input => input.addEventListener('input', () => {
      const screen = document.getElementById('s2');
      const print = Array.from(screen.querySelectorAll('button')).find(b => b.textContent.includes('Печать'));
      const csv = screen.querySelector('[data-export-changes]');
      window.searchActionState = [
        screen.getAttribute('aria-busy'),
        Boolean(print.disabled || print.closest('[inert]')),
        Boolean(csv.disabled || csv.closest('[inert]'))
      ];
    }, {once:true})""")
    search.fill(names[-1])
    assert page.evaluate('window.searchActionState') == ['true', True, True]
    expect(on_screen).to_have_count(1)
    expect(on_screen.first).to_contain_text(names[-1])
    search.fill('')
    expect(on_screen).to_have_count(12)
    expect(on_screen.first).to_contain_text(names[0])
    page.evaluate("""() => {
      window.printedChangeRows = [];
      addEventListener('beforeprint', () => {
        window.printedChangeRows.push(Array.from(document.querySelectorAll(
          '#referencePrintDocument .print-changes tbody tr'), row => row.cells[0].textContent));
      });
    }""")
    probe_dir = output / 'reference-print-changes'
    probe = probe_reference_print(page, probe_dir, pid, screen='#s2')
    assert probe['status'] == 'observed-cancel-reopen-escape-and-focus', probe['status']
    captured = page.evaluate('window.printedChangeRows')
    assert len(captured) >= 2 and all(batch == names for batch in captured), captured
    expect(on_screen).to_have_count(12)
    expect(on_screen.first).to_contain_text(names[0])
    (output / 'reference-print-13.json').write_text(json.dumps(captured, ensure_ascii=False), encoding='utf-8')
