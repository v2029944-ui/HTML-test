"""Drive the shared period disclosure from a detail tab."""
from reference_data_smoke import choose_calendar_date, select_filter_period


def common_filters(page):
    fold = page.locator('#reportShell .filter-fold')
    if not fold.evaluate('node => node.open'):
        fold.locator(':scope > summary').click()
    return fold


def common_period(page, unit, start, end=None, base=None, source='daily'):
    fold = common_filters(page)
    select_filter_period(fold, unit, 'range' if end else 'single')
    if fold.locator('#referenceSource').input_value() != source:
        if not fold.locator('#referenceSource').is_visible():
            fold.locator('.filter-more > summary').click()
        fold.locator('#referenceSource').select_option(source)
    choose_calendar_date(page, fold.locator('[data-date="start"]'), start)
    if end:
        choose_calendar_date(page, fold.locator('[data-date="end"]'), end)
    if base:
        if base[0].isdigit():
            choose_calendar_date(page, fold.locator('[data-base-start]'), base)
        else:
            fold.locator('#sum-base').select_option(base)
    fold.locator(':scope > summary').click()
