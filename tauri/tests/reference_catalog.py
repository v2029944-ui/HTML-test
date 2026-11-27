"""Exercise catalog controls on the installed runner's isolated synthetic DB."""
import json
from pathlib import Path

from playwright.sync_api import expect


def check_reference_catalog(page, confirmations, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    before = page.evaluate('window.desktopAPI.nativeSettingsBootstrap()')
    panel = page.locator('#groupCatalog')
    if not panel.evaluate('node=>node.open'):
        panel.locator(':scope > summary').click()
    search = page.locator('#catalogSearch')
    work, organization = page.locator('#catalogWorkName'), page.locator('#catalogOrganization')
    save, reset = page.locator('#catalogSave'), page.locator('#catalogReset')
    feedback = page.locator('#catalogFeedback')
    page.locator('#catalogAdd').click()
    save.click()
    expect(feedback).to_contain_text('Введите название услуги.')
    expect(feedback).to_be_visible()
    assert confirmations.revision() == before['revision'], 'Validation must not write storage'
    feedback.scroll_into_view_if_needed()
    page.screenshot(path=str(output / 'reference-catalog-validation.png'))

    name = 'Проверка справочника CI ' + str(before['revision'])
    work.fill(name)
    organization.select_option(before['org_names'][0])
    confirmations.cancel_then_accept(save, 'Сохранить услугу в справочнике?')
    expect(save).to_be_enabled(timeout=120_000)
    expect(work).to_have_value('')
    expect(organization).to_have_value('')
    expect(feedback).to_contain_text('Услуга сохранена.')
    search.fill(name)
    rows = page.locator('#catalogList .catalog-row')
    expect(rows).to_have_count(1, timeout=90_000)
    expect(rows.first).to_contain_text(name)
    expect(rows.first).to_contain_text(before['org_names'][0])
    evidence = []
    initial_viewport = page.viewport_size
    initial_window = page.evaluate('({width:innerWidth,height:innerHeight})')
    try:
        for width in (1440, 390):
            page.set_viewport_size({'width': width, 'height': 1000 if width == 1440 else 844})
            search.fill('Нет такой услуги справочника CI')
            expect(rows).to_have_count(0, timeout=90_000)
            search.fill(name)
            expect(rows).to_have_count(1, timeout=90_000)
            trigger = rows.first.locator('.catalog-menu > summary')
            trigger.click()
            rows.first.get_by_role('button', name='Изменить', exact=True).click()
            expect(work).to_have_value(name)
            expect(organization).to_have_value(before['org_names'][0])
            reset.click()
            expect(work).to_have_value('')
            expect(organization).to_have_value('')
            expect(trigger).to_be_focused()
            expect(feedback).not_to_be_visible()
            trigger.press('Enter')
            expect(rows.first.get_by_role('button', name='Изменить', exact=True)).to_be_visible()
            expect(rows.first.get_by_role('button', name='Удалить', exact=True)).to_be_visible()
            geometry = panel.evaluate('''node=>{
              const rect=n=>n.getBoundingClientRect(), list=node.querySelector('#catalogList');
              const controls=node.querySelector('#catalogControls'), row=list.querySelector('.catalog-row');
              const actions=[...row.querySelectorAll('.catalog-row-actions button')].map(rect);
              return {controlsBottom:rect(controls).bottom,listTop:rect(list).top,
                panelWidth:node.clientWidth,scrollWidth:node.scrollWidth,rowHeight:rect(row).height,
                actions:actions.map(r=>({left:r.left,right:r.right,width:r.width,height:r.height})),
                panelLeft:rect(node).left,panelRight:rect(node).right};
            }''')
            assert geometry['controlsBottom'] <= geometry['listTop'] + 1, geometry
            assert geometry['scrollWidth'] <= geometry['panelWidth'] + 1, geometry
            assert len(geometry['actions']) == 2
            for action in geometry['actions']:
                assert action['width'] >= 24 and action['height'] >= 24, geometry
                assert geometry['panelLeft'] <= action['left'] < action['right'] <= geometry['panelRight'], geometry
            trigger.press('Enter')
            panel.evaluate("node=>node.scrollIntoView({block:'start'})")
            page.evaluate('window.scrollBy(0,-110)')
            screenshot = 'reference-catalog-' + str(width) + '.png'
            page.screenshot(path=str(output / screenshot))
            evidence.append({'width': width, 'geometry': geometry, 'screenshot': screenshot})
    finally:
        if initial_viewport is not None:
            page.set_viewport_size(initial_viewport)
        else:
            page.set_viewport_size(initial_window)
            session = page.context.new_cdp_session(page)
            try:
                session.send('Emulation.clearDeviceMetricsOverride')
            finally:
                session.detach()

    # Delete only the exact test row created above, through its normal UI action.
    rows.first.locator('.catalog-menu > summary').click()
    confirmations.cancel_then_accept(rows.first.get_by_role('button', name='Удалить', exact=True),
                                     'Удалить «' + name + '» из справочника? Подразделение: ' + before['org_names'][0] + '.')
    # Clearing the old rows starts the post-write refresh. Wait for the
    # management controls to unlock before checking the refreshed list.
    expect(search).to_be_enabled(timeout=120_000)
    expect(rows).to_have_count(0, timeout=120_000)
    search.fill('')
    after = page.evaluate('window.desktopAPI.nativeSettingsBootstrap()')
    for key in ('reports', 'catalog', 'org_names', 'groups', 'memberships'):
        assert after[key] == before[key], ('Catalog walkthrough left changed fixture data', key)
    (output / 'reference-catalog.json').write_text(
        json.dumps({'steps': evidence, 'revisionBefore': before['revision'],
                    'revisionAfter': after['revision'], 'catalogCount': after['catalog']},
                   ensure_ascii=False, indent=2), encoding='utf-8')
