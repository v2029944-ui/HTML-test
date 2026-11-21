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
  const headings=[...document.querySelectorAll('#reportShell .filter-fold > summary strong,#reportShell .date-group-title')].filter(visible)
    .map(el=>({title:el.textContent.trim(),ratio:ratio(el)}));
  const quick=[...document.querySelectorAll('#reportShell .quick-actions button')].filter(visible).map(rect);
  const chipsAligned=quick.every(r=>Math.abs(r.height-quick[0].height)<=1 &&
    quick.every(other=>Math.abs(r.top-other.top)>=r.height-1 || Math.abs(r.top-other.top)<=1));
  const actions=document.querySelector('#s1 .summary-bottom-actions'),main=actions?.parentElement;
  let summaryGaps=null;
  if(visible(actions) && visible(main) && visible(actions.previousElementSibling)){
    summaryGaps=[rect(actions).top-rect(actions.previousElementSibling).bottom,rect(main).bottom-rect(actions).bottom];
  }
  return {width:innerWidth,headings,fields,chipsAligned,summaryGaps};
}"""


def check_shell_layout(page, *, headings=False, chart=False):
    """Called after theme/layout transitions have settled."""
    metrics = page.evaluate(SHELL_METRICS)
    if headings:
        assert len(metrics['headings']) == 3, metrics
        assert all(h['ratio'] >= 4.5 for h in metrics['headings']), metrics
        assert metrics['chipsAligned'], metrics
        gaps = metrics['summaryGaps']
        if gaps is not None:
            assert gaps[0] > 0 and abs(gaps[0] - gaps[1]) <= 2, metrics
    if chart:
        assert metrics['fields'], metrics
        assert all(f['space'] + 1 >= f['longest'] for f in metrics['fields']), metrics
    return metrics
