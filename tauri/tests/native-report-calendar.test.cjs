'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
class Element {
 constructor(tag){this.tag=tag;this.children=[];this.events={};this.attrs={};this.dataset={};}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=nodes;}
 setAttribute(key,value){this.attrs[key]=value;}
 addEventListener(event,fn){this.events[event]=fn;}
 click(){return this.events.click?.();}
 focus(){this.focused=true;}
}
const ids=['reportCalendar','calendarDetails','calendarPrev','calendarNext','calendarRange','calendarStatus'];
function mount(read){
 const elements=Object.fromEntries(ids.map(id=>[id,new Element('div')]));
 const calls=[],ranges=[];
 const window={desktopAPI:{nativeReportCalendar(...args){calls.push(args);return read(...args);}},
  nativeReportList:{selectRange(...args){ranges.push(args);}}};
 const document={getElementById:id=>elements[id],createElement:tag=>new Element(tag)};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-report-calendar.js'),'utf8'),{window,document});
 return {...elements,calendar:window.nativeReportCalendar,calls,ranges};
}
const data=(year,firstYear=2023,lastYear=2025)=>({year,firstYear,lastYear,days:[]});
function descendants(node){return node.children.flatMap(child=>[child,...descendants(child)]);}
test('twelve months, leap-day drilldown, exact report list range and keyboard focus after closing',async()=>{
 const ui=mount((rev,year)=>({...data(year),days:[{date:'2024-02-29',dayReports:2,weekReports:1,monthReports:0}]}));
 await ui.calendar.setContext(7,'2024-01-01');
 assert.equal(ui.reportCalendar.children.length,12);
 const feb=ui.reportCalendar.children[1];
 feb.click();
 assert.equal(feb.attrs['aria-expanded'],'true');
 const buttons=descendants(ui.calendarDetails).filter(el=>el.tag==='button');
 assert.equal(buttons.length,30);
 buttons.at(-1).click();
 assert.deepEqual(ui.ranges,[['2024-02-29','2024-02-29']]);
 buttons[0].click();
 assert.equal(feb.attrs['aria-expanded'],'false');
 assert.equal(feb.focused,true);
 assert.equal(ui.calendarDetails.children.length,0);
});
test('first available year when no initial date, bounded year navigation and preserved year on refresh',async()=>{
 const ui=mount((rev,year)=>data(year,2001,2002));
 await ui.calendar.setContext(3,null);
 assert.equal(ui.calls.at(-1)[1],2001);
 assert.equal(ui.calendarPrev.disabled,true);
 await ui.calendarNext.click();
 assert.equal(ui.calls.at(-1)[1],2002);
 assert.equal(ui.calendarNext.disabled,true);
 await ui.calendar.setContext(4,'2001-01-01');
 assert.deepEqual(ui.calls.at(-1),[4,2002]);
});
test('stale success and failed refresh leave no outdated months or clickable details',async()=>{
 let resolve;
 const ui=mount((rev,year)=>rev===1?new Promise(done=>{resolve=done;}):
  rev===2?data(year):Promise.reject(Error('read failed')));
 const pending=ui.calendar.setContext(1,'2023-01-01');
 await ui.calendar.setContext(2,'2024-01-01');
 resolve(data(2023));await pending;
 assert.match(ui.calendarRange.textContent,/2024/);
 ui.reportCalendar.children[0].click();
 await ui.calendar.setContext(3,'2024-01-01');
 assert.equal(ui.reportCalendar.children.length,0);
 assert.equal(ui.calendarDetails.children.length,0);
 assert.equal(ui.calendarNext.disabled,true);
 assert.match(ui.calendarStatus.textContent,/Не удалось/);
});
