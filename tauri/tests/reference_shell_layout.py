"""Visual acceptance measurements shared by Chromium and the installed WebView2."""
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
    selector = '#reportShell .filter-content'
    try:
        page.set_viewport_size({'width':1440,'height':650})
        placement.select_option('top',force=True)
        # Leave the initially expanded controls in the document and scroll
        # past them, just as someone reading the summary with the mouse would.
        page.evaluate("window.scrollTo(0,0)")
        if not fold.evaluate('el => el.open'):
            trigger.click()
        page.locator('#sharedHeader .main-nav button').first.focus()
        page.evaluate('window.scrollTo(0,Math.min(560,document.documentElement.scrollHeight-innerHeight))')
        page.wait_for_function("!!document.querySelector('#reportShell[data-auto-folded]')")
        y = page.evaluate('scrollY')
        assert y > 150
        trigger.press('Enter')
        page.wait_for_function("document.querySelector('.filter-fold').open")
        assert abs(page.evaluate('scrollY') - y) <= 2
        report_body = page.locator(selector).bounding_box()
        assert 0 <= report_body['y'] < 160, report_body
        assert (report_body['y'] + report_body['height']) <= 650, report_body
        record('18-filter-top-open', fold, scroll=False)
        trigger.press('Escape')
        assert not fold.evaluate('el => el.open')
        assert abs(page.evaluate('scrollY') - y) <= 2

        placement.select_option('side',force=True)
        scope = page.evaluate('JSON.stringify(window.referenceFilters.scope)')
        for scale in ('80','90','standard','xlarge'):
            size.select_option(scale,force=True)
            page.wait_for_function('window.referenceFilters.atSide()')
            page.evaluate('window.scrollTo(0,Math.min(560,document.documentElement.scrollHeight-innerHeight))')
            page.wait_for_function("document.querySelector('#reportShell').getBoundingClientRect().top < 2")
            sidebar = page.locator('#reportShell').bounding_box()
            report = page.locator('#s1').bounding_box()
            assert sidebar['x'] + sidebar['width'] <= report['x'] + 2, (scale,sidebar,report)
            if scale == 'standard':
                record('18-filter-side', fold, scroll=False)
            reset = fold.locator('.filter-reset')
            # Keyboard focus near the end scrolls the panel to its last action.
            reset.focus()
            bounds = reset.bounding_box()
            assert 0 <= bounds['y'] < bounds['y'] + bounds['height'] <= 651, (scale,bounds)
            assert page.evaluate('document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1')
        page.set_viewport_size({'width':720,'height':650})
        page.wait_for_function('!window.referenceFilters.atSide()')
        assert placement.input_value() == 'side'
        assert page.evaluate('JSON.stringify(window.referenceFilters.scope)') == scope
    finally:
        size.select_option(previous_size,force=True)
        placement.select_option(previous_placement,force=True)
        page.evaluate('window.scrollTo(0,0)')
        if fold.evaluate('el => el.open') != previous_open:
            trigger.click()
