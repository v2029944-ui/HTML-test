"""Chromium checks for the desktop changes table and cursor-backed scroll windows."""
from pathlib import Path
import unittest
from playwright.sync_api import expect, sync_playwright

TAURI = Path(__file__).resolve().parents[1] / 'tauri'


class DesktopTableScroll(unittest.TestCase):
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

    def test_changes_scrollbar_drag_seeks_without_reading_intermediate_pages_and_filter_resets(self):
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
              const total=r.search?1:91,rows=Array.from({length:Math.min(12,total-12*r.page)},(_,i)=>({
                name:r.search||'Работа '+(12*r.page+i+1),org:'A',before:1,after:2,delta:1,percent:100}));
              return {rows,total,searched:total,significant:total,comparable:['A'],counts:{up:total,down:0,gone:0,new:0}};
            }
          };
        }""")
        p.add_script_tag(path=str(TAURI/'reference-results.js'))
        p.evaluate("""async()=>{referenceResults.init();await referenceResults.refresh({revision:1,type:'day',
            from:'2024-01-02',to:'2024-01-02',baseFrom:'2024-01-01',baseTo:'2024-01-01',
            organizations:['A'],metric:'amount',source:'daily',displayType:'day'});await referenceResults.setScreen('s2');}""")
        viewport=p.locator('#s2 [data-sql-row-scroll]')
        rows=p.locator('#s2 .changes-table tbody tr')
        expect(rows).to_have_count(12)
        p.locator('#reportShell .filter-fold > summary').click()
        viewport.scroll_into_view_if_needed()
        x,y,bottom=viewport.evaluate("""e=>{const r=e.getBoundingClientRect();
           return [r.right-6,r.top+30,r.bottom+25]}""")
        p.mouse.move(x,y);p.mouse.down();p.mouse.move(x,bottom);p.mouse.up()
        expect(viewport).to_have_attribute('data-sql-window','7')
        expect(rows).to_have_count(7)
        expect(rows.last).to_contain_text('Работа 91')
        self.assertEqual(p.evaluate('reads.map(r=>r[0])'),[0,7])
        viewport.focus();viewport.press('Home')
        expect(viewport).to_have_attribute('data-sql-window','0')
        expect(rows.first).to_contain_text('Работа 1')
        self.assertTrue(viewport.evaluate('e=>e.scrollTop<2'))
        p.locator('#s2 input[placeholder]').fill('Другая работа')
        expect(rows).to_have_count(1)
        expect(rows.first).to_contain_text('Другая работа')
        self.assertEqual(p.evaluate('reads.at(-1)'),[0,'Другая работа'])
        expect(p.locator('#s2 .pager-footer input,#s2 .pager-footer button')).to_have_count(0)

    def test_cursor_windows_wheel_keyboard_retry_and_retarget(self):
        p=self.page
        p.set_content('<div id="rows"></div><p id="notice"></p>')
        self.assets()
        p.evaluate("""() => {
           window.log=[];window.failOnce=true;
           window.unifiedUI={notice:msg=>document.querySelector('#notice').textContent=msg};
           const root=document.querySelector('#rows');
           function render(index){
             log.push(index);
             root.replaceChildren(...Array.from({length:6},(_,i)=>{const n=document.createElement('div');
               n.style.height='80px';n.textContent=String(6*index+i);return n;}));
             sqlRowScroll.attach(root,{index,count:6,size:6,hasNext:index<2,go:async i=>{
               if(failOnce){failOnce=false;throw Error('test SQL failure');}
               render(i);
             }});
           }
           render(0);
         }""")
        box=p.locator('[data-sql-row-scroll]')
        box.focus();box.press('End')
        expect(p.locator('#notice')).to_contain_text('test SQL failure')
        box.hover();p.mouse.wheel(0,75)
        expect(box).to_have_attribute('data-sql-window','1')
        box.press('End');expect(box).to_have_attribute('data-sql-window','2')
        expect(p.locator('#rows > div').last).to_have_text('17')
        box.press('Home');expect(box).to_have_attribute('data-sql-window','1')
        box.press('Home');expect(box).to_have_attribute('data-sql-window','0')
        self.assertEqual(p.evaluate('log'),[0,1,2,1,0])
        expect(box).to_be_focused()

    def test_horizontal_periods_and_short_final_window_remain_reachable(self):
        p=self.page;p.set_content('<div style="width:380px"><div id="periods"></div></div>')
        self.assets()
        p.evaluate("""()=>{
          const root=document.getElementById('periods');window.calls=[];
          function render(index){
            calls.push(index);const count=index===4?1:8,table=document.createElement('table'),row=table.insertRow();
            for(let i=0;i<count;i++){const c=row.insertCell();c.style.minWidth='85px';c.textContent=String(index*8+i);}
            root.replaceChildren(table);
            sqlRowScroll.attach(root,{axis:'x',index,count,size:8,total:33,hasNext:index<4,direct:true,go:render});
          }render(0);
        }""")
        scroll=p.locator('[data-sql-scroll-host=periods]')
        scroll.focus();scroll.press('End')
        expect(scroll).to_have_attribute('data-sql-window','4')
        expect(p.locator('#periods td')).to_have_text('32')
        scroll.press('Home')
        expect(scroll).to_have_attribute('data-sql-window','0')
        expect(p.locator('#periods td').first).to_have_text('0')
        self.assertEqual(p.evaluate('calls'),[0,4,0])

    def test_sql_work_selection_survives_scroll_and_search(self):
        p=self.page;p.set_content((TAURI/'reference-shell.html').read_text())
        p.add_style_tag(path=str(TAURI/'reference-shell.css'))
        self.assets()
        p.evaluate("""()=>{
          window.calls=[];const snap={revision:1,org_names:['A','B']};
          window.unifiedUI={api:{unifiedPeriodOptions:async()=>({max:'2024-02-29'}),
            unifiedNameOptions:async(revision,search,cursor,limit)=>{
              const offset=Number(cursor||0),total=search?1:128;calls.push({search,offset,limit});
              return {total_rows:total,next:offset+limit<total?String(offset+limit):null,
              rows:Array.from({length:Math.min(limit,total-offset)},(_,i)=>({key:String(search?93:offset+i),kind:'work',name:'Работа '+(search?93:offset+i)}))};
            }},revision:1,snapshot:snap,onSnapshot(){},notice:t=>{throw Error(t)}};
          window.referenceDesktop={dialog(title,anchor){const dialog=document.querySelector('#sheet'),
            root=document.querySelector('#dialogBody');root.replaceChildren();dialog.showModal();return root;}};
          window.referenceResults={refresh:scope=>window.testScope=scope};
        }""")
        for name in ['native-period-selection','reference-calendar','reference-filters']:
            p.add_script_tag(path=str(TAURI/(name+'.js')))
        p.evaluate('referenceFilters.init()')
        p.wait_for_function('window.testScope?.revision===1')
        p.locator('[data-pick=works]').click()
        box=p.locator('[data-sql-row-scroll][data-sql-scroll-host=reference-work-options]')
        p.get_by_role('button',name='Снять выбор',exact=True).click()
        box.focus();box.press('End')
        expect(box).to_have_attribute('data-sql-window','1')
        p.get_by_role('checkbox',name='Работа 93',exact=True).check()
        box.focus();box.press('End')
        expect(box).to_have_attribute('data-sql-window','2')
        expect(box.get_by_role('checkbox')).to_have_count(28)
        box.press('Home');expect(box).to_have_attribute('data-sql-window','1')
        expect(p.get_by_role('checkbox',name='Работа 93',exact=True)).to_be_checked()
        p.get_by_placeholder('Найти работу').fill('93')
        expect(box.get_by_role('checkbox')).to_have_count(1)
        expect(box).to_have_attribute('data-sql-window','0')
        p.get_by_role('button',name='Применить',exact=True).click()
        self.assertEqual(p.evaluate('referenceFilters.scope.selectedKeys'),['93'])
        self.assertEqual(p.evaluate("calls.filter(r=>r.limit===50).map(r=>r.offset)"),[0,50,100,50,0])
