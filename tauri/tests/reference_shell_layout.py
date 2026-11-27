"""Visual acceptance measurements shared by Chromium and the installed WebView2."""
from playwright.sync_api import expect


SHELL_METRICS = """() => {
  const rect = el => el.getBoundingClientRect(), visible = el => el && rect(el).width && rect(el).height;
  const luminance = color => {
    const rgb=color.match(/[\\d.]+/g).slice(0,3).map(n=>Number(n)/255)
      .map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);
    return .2126*rgb[0]+.7152*rgb[1]+.0722*rgb[2];
  };
  const ratio = el => {
    let node=el;
    while(node){
      const background=getComputedStyle(node).backgroundColor;
      if(background!=='transparent' && background!=='rgba(0, 0, 0, 0)'){
        const a=luminance(background),b=luminance(getComputedStyle(el).color);
        return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
      }node=node.parentElement;
    }
    throw Error('No opaque theme background for '+el.textContent);
  };
  const ctx=document.createElement('canvas').getContext('2d');
  const fields=[...document.querySelectorAll('#chartPanel .chart-view-controls select')].filter(visible).map(el=>{
    const s=getComputedStyle(el);ctx.font=s.fontWeight+' '+s.fontSize+' '+s.fontFamily;
    const longest=Math.max(...Array.from(el.options,o=>ctx.measureText(o.text).width));
    const space=el.clientWidth-parseFloat(s.paddingLeft)-parseFloat(s.paddingRight)-16;
    return {id:el.id,longest,space};
  });
  const headings=[...document.querySelectorAll('#reportShell .filter-fold > summary strong,#reportShell .date-group-title,#reportShell .preset-title')].filter(visible)
    .map(el=>({title:el.textContent.trim(),ratio:ratio(el)}));
  const captions=[...document.querySelectorAll('#reportShell .filter-summary, #s1 .report-range a[href="#reportShell"]')].filter(visible)
    .map(el=>({text:el.textContent.trim(),ratio:ratio(el)}));
  const quick=[...document.querySelectorAll('#reportShell .quick-actions button')].filter(visible).map(rect);
  const chipsAligned=quick.every(r=>Math.abs(r.height-quick[0].height)<=1 &&
    quick.every(other=>Math.abs(r.top-other.top)>=r.height-1 || Math.abs(r.top-other.top)<=1));
  const actions=document.querySelector('#s1 .summary-bottom-actions'),main=actions?.parentElement;
  let summaryGaps=null;
  if(visible(actions) && visible(main) && visible(actions.previousElementSibling)){
    summaryGaps=[rect(actions).top-rect(actions.previousElementSibling).bottom,rect(main).bottom-rect(actions).bottom];
  }
  return {width:innerWidth,headings,captions,fields,chipsAligned,summaryGaps};
}"""


def check_shell_layout(page, *, headings=False, chart=False, folded=False):
    """Called after theme/layout transitions have settled."""
    metrics = page.evaluate(SHELL_METRICS)
    if headings:
        assert len(metrics['headings']) == 4, metrics
        assert all(h['ratio'] >= 4.5 for h in metrics['headings']), metrics
        assert metrics['chipsAligned'], metrics
        gaps = metrics['summaryGaps']
        if gaps is not None:
            assert gaps[0] > 0 and abs(gaps[0] - gaps[1]) <= 2, metrics
    if headings or folded:
        assert len(metrics['captions']) == 2, metrics
        assert all(part['ratio'] >= 4.5 for part in metrics['captions']), metrics
    if chart:
        assert metrics['fields'], metrics
        assert all(f['space'] + 1 >= f['longest'] for f in metrics['fields']), metrics
    return metrics


def check_filter_placement(page, record):
    """Exercise the same report controls as a pinned bar and as a sidebar."""
    placement = page.locator('#appearanceFilterPlacement')
    size = page.locator('#appearanceSize')
    fold = page.locator('#reportShell .filter-fold')
    trigger = fold.locator(':scope > summary')
    previous_size, previous_placement = size.input_value(), placement.input_value()
    previous_open = fold.evaluate('el => el.open')
    content = fold.locator('.filter-content')
    sidebar = page.locator('#reportShell')
    settings = page.locator('#storage')
    settings_already_open = settings.is_visible()
    # Like the other CDP geometry checks, wait for layout directly. Chromium's
    # wait_for_function polling can require page-side eval, disallowed by Tauri.
    settle = '() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)))'
    settings_button = page.locator('#refSettings')
    back = page.locator('#backBtn')

    def appearance(*, mode=None, scale=None):
        if not settings.is_visible():
            settings_button.click()
        expect(settings).to_be_visible()
        expect(sidebar).to_be_hidden()
        if scale is not None:
            size.focus()
            size.select_option(scale)
            expect(size).to_be_focused()
            expect(page.locator('#refRoot')).to_have_attribute('data-ui-size',scale)
        if mode is not None:
            placement.select_option(mode)
        back.click()
        expect(fold).to_be_visible()
        page.evaluate(settle)

    try:
        page.set_viewport_size({'width':1440,'height':650})
        appearance(mode='top')
        # Leave the initially expanded controls in the document and scroll
        # past them, just as someone reading the summary with the mouse would.
        page.evaluate("window.scrollTo(0,0)")
        if not fold.evaluate('el => el.open'):
            trigger.click()
        page.locator('#sharedHeader .main-nav button').first.focus()
        assert sidebar.evaluate("el => getComputedStyle(el).position") == 'sticky'
        page.evaluate('window.scrollTo(0,Math.min(560,document.documentElement.scrollHeight-innerHeight))')
        page.evaluate(settle)
        expect(fold).to_have_js_property('open', True)
        assert abs(sidebar.bounding_box()['y'] - page.locator('#sharedHeader').bounding_box()['height']) <= 2
        y = page.evaluate('scrollY')
        assert y > 150
        trigger.press('Enter')
        expect(fold).to_have_js_property('open', False)
        trigger.press('Enter')
        expect(fold).to_have_js_property('open', True)
        page.evaluate(settle)
        assert abs(page.evaluate('scrollY') - y) <= 2
        report_body = content.bounding_box()
        assert 0 <= report_body['y'] < 160, report_body
        assert (report_body['y'] + report_body['height']) <= 650, report_body
        record('18-filter-top-open', fold, scroll=False)
        trigger.press('Escape')
        assert not fold.evaluate('el => el.open')
        assert abs(page.evaluate('scrollY') - y) <= 2

        page.set_viewport_size({'width':2600,'height':650})
        scope = page.evaluate('JSON.stringify(window.referenceFilters.scope)')
        reset = fold.locator('.filter-reset')
        for scale in ('80','90','standard','large','xlarge'):
            appearance(mode='top', scale=scale)
            page.evaluate("scrollTo({top:0,behavior:'instant'})")
            report = page.locator('#s1 main')
            header = page.locator('#sharedHeader')
            brand = header.locator('.brand')
            page.evaluate(settle)
            original = [element.bounding_box() for element in (report, header, brand)]
            appearance(mode='side')
            expect(sidebar).to_have_css('--filter-at-side','1')
            page.evaluate(settle)
            for element, before in zip((report, header, brand), original):
                after = element.bounding_box()
                assert abs(after['x'] - before['x']) <= 2 and abs(after['width'] - before['width']) <= 2, (
                    scale, before, after
                )
            origin = sidebar.bounding_box()['y']
            # Measure against the real scroll position; a short document cannot
            # scroll past its available content even when a larger Y is requested.
            page.evaluate("y => scrollTo({top:y,behavior:'instant'})", max(560,origin+40))
            page.evaluate(settle)
            scrolled_y = page.evaluate('scrollY')
            expected_top = max(header.bounding_box()['height'], origin - scrolled_y)
            assert abs(header.bounding_box()['y']) <= 2
            sidebar_box = fold.bounding_box()
            report_box = page.locator('#s1 .title-row').bounding_box()
            assert abs(sidebar_box['y']-expected_top) <= 2, (scale,origin,scrolled_y,expected_top,sidebar_box)
            assert sidebar_box['x'] + sidebar_box['width'] <= report_box['x'] + 2, (scale,sidebar_box,report_box)
            assert scrolled_y > origin + 2, ('Analytics should scroll past the header', scale,origin,scrolled_y)
            if scale == 'standard':
                record('18-filter-side', fold, scroll=False)
            # Enter the final action by keyboard from its preceding available
            # control. Both focus transitions must leave the document pinned.
            previous_button = content.locator('button:enabled:visible').nth(-2)
            previous_button.focus()
            expect(previous_button).to_be_focused()
            page.keyboard.press('Tab')
            expect(reset).to_be_focused()
            page.evaluate(settle)
            bounds = reset.bounding_box()
            body = content.bounding_box()
            visible_bottom = min(page.evaluate('innerHeight'), body['y']+body['height'])
            assert bounds['y'] >= body['y'] - 2 and bounds['y']+bounds['height'] <= visible_bottom + 2, (
                scale,bounds,body,visible_bottom,
                content.evaluate('el => ({scrollTop:el.scrollTop,scrollHeight:el.scrollHeight,clientHeight:el.clientHeight})')
            )
            assert abs(page.evaluate('scrollY')-scrolled_y) <= 2, (scale,scrolled_y,page.evaluate('scrollY'))
            assert page.evaluate('document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1')
        # At the same window width, increasing the interface scale can use
        # up the free left margin. The saved preference still applies again
        # when there is room.
        page.set_viewport_size({'width':1920,'height':650})
        appearance(scale='standard')
        expect(sidebar).to_have_css('--filter-at-side','1')
        appearance(scale='xlarge')
        page.evaluate(settle)
        assert not page.evaluate('window.referenceFilters.atSide()')
        appearance(scale='standard')
        expect(sidebar).to_have_css('--filter-at-side','1')
        page.set_viewport_size({'width':720,'height':650})
        page.evaluate(settle)
        assert not page.evaluate('window.referenceFilters.atSide()')
        assert placement.input_value() == 'side'
        assert page.evaluate('JSON.stringify(window.referenceFilters.scope)') == scope
    finally:
        appearance(mode=previous_placement, scale=previous_size)
        page.evaluate('window.scrollTo(0,0)')
        if fold.evaluate('el => el.open') != previous_open:
            trigger.click()
        if settings_already_open:
            settings_button.click()


def check_sticky_table_header(page):
    """Verify that the table's date range and column labels stay together while scrolling."""
    result = page.evaluate("""() => {
      const host = document.createElement('div');
      host.className = 'sql-page-content';
      host.dataset.pageAxis = 'y';
      host.style.width = '340px';
      const dates = '01.01.2026 — 31.01.2026';
      const rows = Array.from({length:50}, (_, i) => ['Работа ' + (i + 1), String(i)]);
      window.unifiedUI.table(host, 'amount', dates, ['Работа','Показатель'], rows);
      const viewport = host.querySelector('.table-scroll');
      const table = viewport.querySelector('table');
      table.style.minWidth = '700px';
      const head = table.tHead, labels = head.rows[0], period = head.rows[1], body = table.tBodies[0];
      document.body.append(host);
      try {
        const initialLeft = labels.cells[1].getBoundingClientRect().left;
        const initialBodyTop = body.rows[0].getBoundingClientRect().top;
        viewport.scrollTop = 100;
        viewport.scrollLeft = 70;
        const rect = head.getBoundingClientRect(), periodRect = period.getBoundingClientRect();
        return {
          vertical: Math.abs(rect.top - viewport.getBoundingClientRect().top) < 2 &&
            initialBodyTop - body.rows[0].getBoundingClientRect().top > 90,
          horizontal: initialLeft - labels.cells[1].getBoundingClientRect().left > 60 &&
            Math.abs(labels.cells[1].getBoundingClientRect().left - body.rows[0].cells[1].getBoundingClientRect().left) < 2,
          dates: period.textContent === dates && table.caption.textContent === dates &&
            periodRect.top > rect.top && Math.abs(periodRect.bottom - rect.bottom) < 2,
        };
      } finally {
        host.remove();
      }
    }""")
    assert result == {'vertical': True, 'horizontal': True, 'dates': True}, result
