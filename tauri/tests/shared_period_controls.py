"""Drive the common date filters from a detail tab, including its return link."""
from playwright.sync_api import expect
from reference_data_smoke import choose_calendar_date


def common_filters(page):
    fold = page.locator('#reportShell .filter-fold')
    if not fold.evaluate('node => node.open'):
        link = page.locator('#refNative .detail-date-scope:visible button')
        if link.count():
            link.click()
            expect(fold.locator(':scope > summary')).to_be_focused()
        else:
            fold.locator(':scope > summary').click()
    return fold


def common_period(page, unit, start, end=None, base=None, source='daily'):
    fold = common_filters(page)
    fold.locator('[data-filter="type"]').get_by_role('button', name=unit, exact=True).click()
    if fold.locator('#referenceSource').input_value() != source:
        fold.locator('#referenceSource').select_option(source)
    fold.locator('[data-filter="mode"]').get_by_role(
        'button', name='Диапазон' if end else 'Один период', exact=True).click()
    choose_calendar_date(page, fold.locator('[data-date="start"]'), start)
    if end:
        choose_calendar_date(page, fold.locator('[data-date="end"]'), end)
    if base:
        if base[0].isdigit():
            choose_calendar_date(page, fold.locator('.filter-base-edit'), base)
        else:
            fold.locator('#sum-base').select_option(base)
    fold.locator(':scope > summary').click()
