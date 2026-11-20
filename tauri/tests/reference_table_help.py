"""Exercise the rendered Tauri table headings, including one-action dismissal."""
from playwright.sync_api import expect


def check_table_help(section, expected_count):
    """Check every column hint in one rendered results table and return its headings."""
    buttons = section.locator('thead .table-help-button')
    note = section.locator('.table-header-help')
    close = note.get_by_role('button', name='Закрыть', exact=True)
    expect(buttons).to_have_count(expected_count)
    expect(note).to_be_hidden()
    labels = []

    def open_hint(button):
        label = button.get_attribute('aria-label').removeprefix('Пояснение: ')
        description = button.get_attribute('title')
        assert label and description
        assert button.get_attribute('aria-controls') == note.get_attribute('id')
        button.click()
        expect(section.locator('[aria-expanded="true"].table-help-button')).to_have_count(1)
        expect(button).to_have_attribute('aria-expanded', 'true')
        expect(note.get_by_role('status')).to_have_text(label + '. ' + description)
        expect(close).to_be_visible()
        return label

    def closed(button):
        expect(note).to_be_hidden()
        expect(button).to_have_attribute('aria-expanded', 'false')
        expect(button).to_be_focused()
        expect(section).to_be_visible()

    for i in range(expected_count):
        button = buttons.nth(i)
        labels.append(open_hint(button))
        if i:
            expect(buttons.nth(i - 1)).to_have_attribute('aria-expanded', 'false')

    last = buttons.last
    close.focus()
    close.press('Escape')
    closed(last)
    for i in range(expected_count):
        button = buttons.nth(i)
        open_hint(button)
        close.click()
        closed(button)
        open_hint(button)
        button.click()
        closed(button)
    return labels
