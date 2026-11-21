"""Chromium checks for the desktop SQL page chooser, focus and exact ranges."""
from pathlib import Path
import unittest
from playwright.sync_api import expect, sync_playwright

TAURI = Path(__file__).resolve().parents[1] / 'tauri'


class DesktopTablePager(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(executable_path='/usr/bin/chromium',
            ignore_default_args=['--hide-scrollbars'], args=['--disable-features=OverlayScrollbar'])

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()

    def setUp(self):
        self.page = self.browser.new_page(viewport={'width':1100,'height':800})
        self.addCleanup(self.page.close)

    def assets(self):
        self.page.add_style_tag(path=str(TAURI/'sql-row-scroll.css'))
        self.page.add_script_tag(path=str(TAURI/'sql-row-scroll.js'))

    def test_changes_number_picker_seeks_directly_and_preserves_position_on_short_page(self):
        p=self.page
        p.set_content((TAURI/'reference-shell.html').read_text())
        p.add_style_tag(path=str(TAURI/'reference-shell.css'))
        self.assets()
        p.evaluate("""() => {
          window.reads=[];
          const total={amount:1,count:1,average:1};
          const full={complete:true,periods:1,missing:0,total,available_reports:1,available_total:total};
          window.desktopAPI={
            nativeAnalysisCompare:async()=>({summary:{before:full,after:full},divisions:[],top_amount:[],top_count:[]}),
            nativeAnalysisOrganizations:async()=>[{org:'A',complete:true,missing:0,total}],
            nativeAnalysisRange:async()=>full,
            referenceChanges:async r=>{
              reads.push([r.page,r.search]);
              if(window.readDelay) await new Promise(resolve=>setTimeout(resolve,window.readDelay));
              const total=r.search?1:91,rows=Array.from({length:Math.min(12,total-12*r.page)},(_,i)=>({
                name:r.search||'Работа '+(12*r.page+i+1),org:'A',before:1,after:2,delta:1,percent:100}));
              return {rows,total,searched:total,significant:total,comparable:['A'],counts:{up:total,down:0,gone:0,new:0}};
            }
          };
        }""")
        p.add_script_tag(path=str(TAURI/'analysis-summary.js'))
        p.add_script_tag(path=str(TAURI/'reference-results.js'))
        p.evaluate("""async()=>{referenceResults.init();await referenceResults.refresh({revision:1,type:'day',
            from:'2024-01-02',to:'2024-01-02',baseFrom:'2024-01-01',baseTo:'2024-01-01',
            organizations:['A'],metric:'amount',source:'daily',displayType:'day'});await referenceResults.setScreen('s2');}""")
        nav=p.locator('[data-sql-pager="reference-changes"]')
        rows=p.locator('#s2 .changes-table tbody tr')
        expect(rows).to_have_count(12)
        p.locator('#reportShell .filter-fold > summary').click()
        choose=nav.locator('[data-page-action=choose]')
        choose.scroll_into_view_if_needed()
        before=choose.bounding_box()['y']
        choose.click();nav.locator('[data-page-number="8"]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 85–91 из 91')
        expect(rows).to_have_count(7)
        expect(rows.last).to_contain_text('Работа 91')
        self.assertEqual(p.evaluate('reads.map(r=>r[0])'),[0,7])
        expect(choose).to_be_focused()
        self.assertAlmostEqual(before,choose.bounding_box()['y'],delta=2)
        expect(nav.locator('[data-page-action=next]')).to_be_disabled()
        nav.locator('[data-page-action=previous]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 73–84 из 91')
        p.evaluate('window.readDelay=100')
        previous=nav.locator('[data-page-action=previous]')
        previous.focus();previous.press('Enter')
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 61–72 из 91')
        expect(previous).to_be_focused()
        category=p.locator('#s2 .kpi').first
        category.focus();category.press('Enter')
        expect(p.locator('#s2 .changes-heading h2')).to_have_text('Рост')
        expect(category).to_be_focused()
        category.press('Enter')
        search=p.locator('#s2 input[placeholder]')
        search.focus()
        expect(p.locator('#s2 .changes-heading h2')).to_have_text('Значимые изменения')
        expect(search).to_be_focused()
        search.fill('Другая работа')
        expect(rows).to_have_count(1)
        expect(rows.first).to_contain_text('Другая работа')
        self.assertEqual(p.evaluate('reads.at(-1)'),[0,'Другая работа'])
        expect(nav.locator('[data-page-total]')).to_have_text('из 1')
        expect(choose).to_be_disabled()

    def test_large_direct_chooser_escape_validation_failure_retry_and_focus_in_dialog(self):
        p=self.page
        p.set_viewport_size({'width':360,'height':500})
        p.set_content('<div id="refRoot" style="--ui-accent:#315e49"><dialog id="sheet"><div id="rows"></div></dialog></div>')
        self.assets()
        p.add_style_tag(path=str(TAURI/'reference-shell.css'))
        p.evaluate("""()=>{
          const sheet=document.getElementById('sheet'),root=document.getElementById('rows');
          sheet.showModal();window.calls=[];window.fail=true;
          function show(index){
            root.textContent='Страница '+(index+1);
            sqlRowScroll.attach(root,{index,size:2,count:index===999999?1:2,total:1999999,go:async next=>{
              calls.push(next);if(fail){fail=false;throw Error('SQL read failed');}show(next);
            }});
          }show(0);
        }""")
        nav=p.locator('[data-sql-pager="rows"]')
        choose=nav.locator('[data-page-action=choose]')
        entry=nav.locator('[data-page-input]')
        expect(choose).to_have_css('background-color','rgb(49, 94, 73)')
        choose.click()
        self.assertLessEqual(nav.locator('[data-page-number]').count(),200)
        expect(entry).to_be_focused()
        entry.fill('918262');entry.press('Escape')
        expect(p.locator('#sheet')).to_have_attribute('open','')
        expect(entry).not_to_be_visible()
        expect(choose).to_be_focused()
        choose.click();expect(entry).to_have_value('1')
        entry.fill('4');nav.locator('[data-page-action=cancel]').click()
        expect(choose).to_be_focused()
        choose.click();expect(entry).to_have_value('1')
        entry.fill('1000001');entry.press('Enter');expect(entry).to_be_visible()
        self.assertEqual(p.evaluate('calls'),[])
        entry.fill('918262');entry.press('Enter')
        expect(nav.get_by_role('alert')).to_contain_text('SQL read failed')
        expect(choose).to_be_focused()
        choose.click();entry.fill('918262');entry.press('Enter')
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 1836523–1836524 из 1999999')
        choose.click();nav.locator('[data-page-action="ranges"]').click()
        for _ in range(6):
            if nav.locator('[data-page-number="1000000"]').count():
                break
            self.assertLessEqual(nav.locator('[data-page-from]').count(),10)
            nav.locator('[data-page-to="1000000"]').click()
        self.assertEqual(p.evaluate('calls'),[918261,918261])
        self.assertLessEqual(nav.locator('[data-page-number]').count(),200)
        nav.locator('[data-page-number="1000000"]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 1999999–1999999 из 1999999')
        expect(nav.locator('[data-page-action=next]')).to_be_disabled()
        self.assertEqual(p.evaluate('calls'),[918261,918261,999999])

    def test_mouse_opens_every_page_of_a_200_page_list_with_enlarged_type(self):
        p=self.page;p.set_viewport_size({'width':380,'height':500})
        p.set_content('<div style="font-size:24px"><div id="rows"></div></div>')
        self.assets()
        p.evaluate("""()=>{
          const root=document.getElementById('rows');window.requested=[];
          function render(index){
            root.textContent='Строка '+(index+1);
            sqlRowScroll.attach(root,{index,size:1,count:1,total:200,go:i=>{requested.push(i);render(i);}});
          }render(0);
        }""")
        nav=p.locator('[data-sql-pager=rows]');choose=nav.locator('[data-page-action=choose]')
        choose.click();numbers=nav.locator('[data-page-number]')
        expect(numbers).to_have_count(200)
        nav.locator('[data-page-number="7"]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 7–7 из 200')
        choose.click();choices=nav.locator('.sql-page-choices')
        self.assertTrue(choices.evaluate('e=>e.scrollHeight>e.clientHeight && e.scrollWidth<=e.clientWidth+1'))
        menu=nav.locator('[popover]').bounding_box()
        self.assertGreaterEqual(menu['x'],0)
        self.assertLessEqual(menu['x']+menu['width'],381)
        choices.hover();p.mouse.wheel(0,4000)
        nav.locator('[data-page-number="198"]').click()
        expect(choose).to_be_focused()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 198–198 из 200')
        self.assertEqual(p.evaluate('requested'),[6,197])

    def test_controls_only_banner_keeps_chart_layout_and_focus_after_redraw(self):
        p=self.page;p.set_content('<div class="metric-plot"><div id="chart-pages" style="min-height:1em"></div></div>')
        self.assets()
        p.evaluate("""()=>{
          const root=document.getElementById('chart-pages');
          function render(index){
            root.replaceChildren();
            sqlRowScroll.attach(root,{index,size:5,count:5,total:15,footer:root,controlsOnly:true,go:render});
          }render(0);window.initialNav=root.firstElementChild;
        }""")
        nav=p.locator('[data-sql-pager="chart-pages"]');choose=nav.locator('[data-page-action=choose]')
        choose.click();nav.locator('[data-page-number="3"]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 11–15 из 15')
        expect(choose).to_be_focused()
        self.assertTrue(p.evaluate("document.querySelector('#chart-pages > nav')===initialNav"))
        self.assertTrue(p.locator('#chart-pages').evaluate("r=>!r.matches('.sql-page-content,[data-page-axis]')"))
        p.evaluate("sqlRowScroll.reset(document.getElementById('chart-pages'))")
        self.assertEqual(p.locator('#chart-pages').evaluate("r=>r.style.minHeight"),'1em')

    def test_visible_chooser_only_targets_current_scope_and_handles_empty_results(self):
        p=self.page;p.set_content('<div id="rows"></div><button id="filter">Фильтр</button>')
        self.assets()
        p.evaluate("""()=>{
          const root=document.getElementById('rows');window.reads=[];
          function initial(){sqlRowScroll.attach(root,{index:0,size:50,count:50,total:305,go:async index=>{
            reads.push(index);await new Promise((_resolve,reject)=>window.failOld=reject);
          }});}
          document.getElementById('filter').onclick=()=>{
            sqlRowScroll.reset(root);root.textContent='Совпадений нет';
            sqlRowScroll.attach(root,{index:0,size:50,count:0,total:0,go:()=>reads.push('unexpected')});
          };initial();
        }""")
        nav=p.locator('[data-sql-pager=rows]')
        choose=nav.locator('[data-page-action=choose]')
        choose.click();expect(nav.locator('[data-page-input]')).to_be_visible()
        choose.click();expect(nav.locator('[data-page-input]')).not_to_be_visible()
        nav.locator('[data-page-action=next]').click()
        expect(nav).to_have_attribute('aria-busy','true')
        p.locator('#filter').click()
        p.evaluate("failOld(Error('stale read failed'))")
        expect(nav).to_be_hidden()
        expect(p.locator('#rows')).to_have_text('Совпадений нет')
        expect(nav.locator('[role=alert]')).not_to_be_visible()
        expect(p.locator('#filter')).to_be_focused()
        self.assertEqual(p.evaluate('reads'),[1])

    def test_horizontal_periods_keep_real_horizontal_scroll_and_direct_pages(self):
        p=self.page;p.set_content('<div style="width:380px"><div id="periods"></div></div>')
        self.assets()
        p.evaluate("""()=>{
          const root=document.getElementById('periods');window.calls=[];
          function render(index){
            calls.push(index);const count=index===4?1:8,table=document.createElement('table'),row=table.insertRow();
            for(let i=0;i<count;i++){const c=row.insertCell();c.style.minWidth='85px';c.textContent=String(index*8+i);}
            root.replaceChildren(table);
            sqlRowScroll.attach(root,{axis:'x',index,count,size:8,total:33,go:render});
          }render(0);
        }""")
        nav=p.locator('[data-sql-pager=periods]')
        self.assertTrue(p.locator('#periods').evaluate("e=>e.scrollWidth>e.clientWidth"))
        p.locator('#periods').evaluate('e=>e.scrollLeft=e.scrollWidth')
        expect(nav).to_have_attribute('data-sql-window','0')
        nav.locator('[data-page-action=choose]').click();nav.locator('[data-page-number="5"]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Периоды 33–33 из 33')
        expect(p.locator('#periods td')).to_have_text('32')
        nav.locator('[data-page-action=choose]').click();nav.locator('[data-page-number="1"]').click()
        expect(p.locator('#periods td').first).to_have_text('0')
        self.assertEqual(p.evaluate('calls'),[0,4,0])

    def load_reference_filters(self):
        p=self.page;p.set_content((TAURI/'reference-shell.html').read_text())
        p.add_style_tag(path=str(TAURI/'reference-shell.css'))
        self.assets()
        p.evaluate("""()=>{
          window.calls=[];const snap={revision:1,org_names:['A','B']};
          window.unifiedUI={api:{unifiedPeriodOptions:async()=>({max:'2024-02-29'}),
            unifiedNameOptions:async()=>({total_rows:128}),
            unifiedSelectedNameOptions:async(revision,keys)=>({total_rows:128,rows:keys.filter(k=>k==='93'&&revision<3)
              .map(key=>({key,kind:'work',name:'Работа 93'}))}),
            unifiedNameOptionsAtPage:async(revision,search,index,limit)=>{
              const offset=index*limit,total=search?1:128;calls.push({search,offset,limit});
              return {total_rows:total,next:offset+limit<total?String(offset+limit):null,
              rows:Array.from({length:Math.min(limit,total-offset)},(_,i)=>({key:String(search?93:offset+i),kind:'work',name:'Работа '+(search?93:offset+i)}))};
            }},revision:1,snapshot:snap,onSnapshot(fn){this.updateSnapshot=fn},notice:t=>{window.lastNotice=t}};
          window.referenceDesktop={dialog(title,anchor){const dialog=document.querySelector('#sheet'),
            root=document.querySelector('#dialogBody');root.replaceChildren();dialog.showModal();return root;}};
          window.referenceResults={beginScopeChange(){},failScopeChange(message){throw Error(message);},refresh:scope=>window.testScope=scope};
        }""")
        for name in ['native-period-selection','reference-calendar','reference-filters']:
            p.add_script_tag(path=str(TAURI/(name+'.js')))
        p.evaluate('referenceFilters.init()')
        p.wait_for_function('window.testScope?.revision===1')
        return p

    def test_imported_week_scope_uses_the_saved_weekday(self):
        p=self.load_reference_filters()
        for start,end,last,base in (
                ('2026-11-09','2026-11-15','2026-11-09','2026-11-02'),
                ('2026-11-11','2026-11-17','2026-11-11','2026-11-04'),
                ('2026-11-11','2026-11-24','2026-11-18','2026-10-28')):
            p.evaluate("([from,to])=>referenceFilters.openImportedPeriod({kind:'week',from,to})", [start,end])
            scope=p.evaluate('referenceFilters.scope')
            self.assertEqual((scope['from'],scope['to'],scope['displayTo'],scope['baseFrom']), (start,last,end,base))
        p.locator('[data-shift="1"]').click()
        self.assertEqual(p.evaluate('referenceFilters.scope.from'),'2026-11-25')
        p.evaluate("referenceFilters.openImportedPeriod({kind:'week',from:'2026-11-04',to:'2026-12-01',mixedWeekStarts:true})")
        self.assertEqual(p.evaluate('[referenceFilters.scope.from,referenceFilters.scope.to]'), ['2026-11-25']*2)
        self.assertIn('последняя загруженная неделя',p.evaluate('lastNotice'))

    def test_sql_work_selection_survives_page_changes_search_and_revisions(self):
        p=self.load_reference_filters()
        p.locator('[data-pick=works]').click()
        nav=p.locator('[data-sql-pager=reference-work-options]')
        box=p.locator('#sheet .filter-checkboxes')
        p.get_by_role('button',name='Снять выбор',exact=True).click()
        nav.locator('[data-page-action=next]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 51–100 из 128')
        p.get_by_role('checkbox',name='Работа 93',exact=True).check()
        nav.locator('[data-page-action=next]').click()
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 101–128 из 128')
        expect(box.get_by_role('checkbox')).to_have_count(28)
        nav.locator('[data-page-action=previous]').click()
        expect(p.get_by_role('checkbox',name='Работа 93',exact=True)).to_be_checked()
        p.get_by_placeholder('Найти работу').fill('93')
        expect(box.get_by_role('checkbox')).to_have_count(1)
        expect(nav.locator('[data-page-range]')).to_have_text('Позиции 1–1 из 1')
        p.get_by_role('button',name='Применить',exact=True).click()
        self.assertEqual(p.evaluate('referenceFilters.scope.selectedKeys'),['93'])
        self.assertEqual(p.evaluate("calls.filter(r=>r.limit===50).map(r=>r.offset)"),[0,50,100,50,0])
        for revision,expected in [(2,['93']),(3,[])]:
            p.evaluate('''async rev=>{
              await unifiedUI.updateSnapshot(null);
              unifiedUI.revision=rev;unifiedUI.snapshot.revision=rev;
              await unifiedUI.updateSnapshot(unifiedUI.snapshot);
            }''',revision)
            self.assertEqual(p.evaluate('referenceFilters.scope.selectedKeys'),expected)
            self.assertFalse(p.evaluate('referenceFilters.scope.allNames'))
