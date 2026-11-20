/* Shared, draft-only calendar for the report shell and native period fields. */
(function () {
'use strict';
const make=(tag,text)=>{const node=document.createElement(tag);if(text!==undefined)node.textContent=text;return node;};
const dateOf=value=>new Date(value+'T12:00:00Z');
const serial=date=>date.toISOString().split('T')[0];
function dateAt(year,month,day=1){
 const date=new Date(0);
 date.setUTCFullYear(year,month,day);date.setUTCHours(12,0,0,0);
 return serial(date);
}
function day(value,offset){
 const date=dateOf(value);date.setUTCDate(date.getUTCDate()+offset);return serial(date);
}
function period(value,unit){
 const date=dateOf(value),year=date.getUTCFullYear(),month=date.getUTCMonth();
 if(unit==='week'){const start=day(value,-((date.getUTCDay()+6)%7));return [start,day(start,6)];}
 if(unit==='month')return [dateAt(year,month),dateAt(year,month+1,0)];
 if(unit==='year')return [dateAt(year,0),dateAt(year,11,31)];
 return [value,value];
}
const short=value=>value.slice(8)+'.'+value.slice(5,7);
const stamp=value=>short(value)+'.'+value.slice(0,4);
const valid=value=>/^\d{4}-\d{2}-\d{2}$/.test(value)&&value.slice(0,4)!=='0000'&&!isNaN(dateOf(value))&&serial(dateOf(value))===value;
const btn=(text,run)=>{const button=make('button',text);button.type='button';button.addEventListener('click',run);return button;};
function openCalendar({anchor, title, initial, unit='day', allowed, variants, allLabel, allSelected=false, spanFor=period, apply}){
 const dialog=make('dialog');dialog.className='period-calendar';dialog.setAttribute('aria-label',title);
 const heading=make('div',title);heading.className='cal-title';
 const toolbar=make('div');toolbar.className='cal-toolbar';
 const grid=make('div');grid.className='cal-grid';
 const weekdays=make('div');weekdays.className='cal-weekdays';
 for(const name of ['Пн','Вт','Ср','Чт','Пт','Сб','Вс'])weekdays.append(make('span',name));
 const footer=make('div');footer.className='cal-footer';
 const summary=make('output');summary.setAttribute('aria-live','polite');
 const actions=make('div');actions.className='cal-actions';
 let selected=initial,view=initial,mode=unit==='year'?'years':unit==='month'?'months':'days';
 const monthName=new Intl.DateTimeFormat('ru-RU',{month:'long',timeZone:'UTC'});
 const fullDate=new Intl.DateTimeFormat('ru-RU',{day:'numeric',month:'long',year:'numeric',timeZone:'UTC'});
 const dateOf=v=>new Date(v+'T12:00Z');
 const iso=dateAt;
 const capital=v=>v.charAt(0).toUpperCase()+v.slice(1);
 const press=(label,fn)=>{const button=btn(label,fn);button.type='button';return button};
 function choose(v){allSelected=false;selected=v;view=v;draw();grid.querySelector('[data-stamp="'+v+'"]')?.focus({preventScroll:true})}
 function move(n){
  const y=Number(view.slice(0,4)),m=Number(view.slice(5,7))-1;
  const next=mode==='days'?iso(y,m+n):iso(y+n*(mode==='years'?12:1),m);
  if(/^\d{4}-/.test(next)&&+next.slice(0,4)>=1)view=next;
  draw();toolbar.children[n<0?0:2].focus({preventScroll:true});
 }
 function draw(){
  grid.replaceChildren();toolbar.replaceChildren();grid.dataset.mode=mode;weekdays.hidden=mode!=='days';
  const date=dateOf(view),y=date.getUTCFullYear(),m=date.getUTCMonth(),firstYear=y-y%12;
  const headingText=mode==='days'?capital(monthName.format(date))+' '+y:mode==='months'?String(y):firstYear+' – '+(firstYear+11);
  const titleButton=press(headingText,()=>{mode=mode==='days'?'months':'years';draw();grid.querySelector('button:not(:disabled)')?.focus()});
  titleButton.disabled=mode==='years';titleButton.title=mode==='days'?'Выбрать месяц':'Выбрать год';
  const prev=press('‹',()=>move(-1)),next=press('›',()=>move(1));prev.setAttribute('aria-label','Назад по календарю');next.setAttribute('aria-label','Вперёд по календарю');toolbar.append(prev,titleButton,next);
  const [lo,hi]=spanFor(selected,unit);
  summary.textContent=unit==='year'?selected.slice(0,4):unit==='month'?capital(monthName.format(dateOf(selected)))+' '+selected.slice(0,4):unit==='week'?short(lo)+' — '+short(hi)+' · '+hi.slice(0,4):fullDate.format(dateOf(selected));
  if(allSelected)summary.textContent=allLabel;
  if(allButton)allButton.setAttribute('aria-pressed',String(allSelected));
  confirm.disabled=!allSelected&&Boolean(allowed&&!allowed.has(selected));
  for(const button of variantRow.children)button.setAttribute('aria-pressed',String(!allSelected&&button.dataset.unit===unit));
  const monthStart=iso(y,m),start=day(monthStart,-((dateOf(monthStart).getUTCDay()+6)%7));
  const count=mode==='days'?42:12;
  for(let i=0;i<count;i++){
   const stamp=mode==='days'?day(start,i):mode==='months'?iso(y,i):iso(firstYear+i,0);
   if(!valid(stamp)){
    const gap=make('span');gap.setAttribute('aria-hidden','true');grid.append(gap);continue;
   }
   const item=press(mode==='days'?String(+stamp.slice(8)):mode==='months'?capital(monthName.format(dateOf(stamp))):stamp.slice(0,4),()=>{
    if(mode==='years'&&unit!=='year'){view=stamp;mode='months';draw();grid.firstChild.focus()}
    else if(mode==='months'&&!['month','year'].includes(unit)){view=stamp;mode='days';draw();grid.querySelector('[data-stamp="'+stamp+'"]')?.focus()}
    else choose(stamp);
   });
   item.dataset.stamp=stamp;const selectable=mode==='days'||(mode==='months'&&unit==='month')||(mode==='years'&&unit==='year');
   if(selectable&&allowed&&!allowed.has(stamp))item.disabled=true;
   const match=!allSelected&&(mode==='days'?stamp>=lo&&stamp<=hi:mode==='months'?stamp.slice(0,7)===selected.slice(0,7):stamp.slice(0,4)===selected.slice(0,4));
   item.setAttribute('aria-pressed',String(match));if(mode==='days'){item.setAttribute('aria-label',fullDate.format(dateOf(stamp)));item.classList.toggle('adjacent',stamp.slice(0,7)!==view.slice(0,7))}
   grid.append(item);
  }
  grid.onkeydown=e=>{const stride=mode==='days'?7:3,offset={ArrowLeft:-1,ArrowRight:1,ArrowUp:-stride,ArrowDown:stride}[e.key];if(!offset)return;const items=[...grid.children],nextItem=items[items.indexOf(e.target)+offset];if(nextItem?.tagName==='BUTTON'&&!nextItem.disabled){e.preventDefault();nextItem.focus()}};
 }
 function position(){
  const rect=anchor.getBoundingClientRect(),frame=anchor.ownerDocument.defaultView.frameElement;
  let left=rect.left,bottom=rect.bottom,top=rect.top;
  if(frame){const outer=frame.getBoundingClientRect(),scale=outer.width/frame.offsetWidth;left=outer.left+rect.left*scale;bottom=outer.top+rect.bottom*scale;top=outer.top+rect.top*scale}
  const w=document.documentElement.clientWidth||innerWidth,h=innerHeight,box=dialog.getBoundingClientRect();
  dialog.style.left=Math.max(8,Math.min(left,w-box.width-8))+'px';
  dialog.style.top=Math.max(8,Math.min(bottom+6+box.height<=h-8?bottom+6:top-box.height-6,h-box.height-8))+'px';
 }
 const variantRow=make('div');variantRow.className='cal-types';
 if(variants)for(const [kind,dates] of variants){
  const button=press(({day:'Дни',week:'Недели',month:'Месяцы',year:'Годы'})[kind]||kind,()=>{
   unit=kind;allowed=dates;allSelected=false;selected=dates.has(selected)?selected:dates.values().next().value;view=selected;
   mode=unit==='month'?'months':unit==='year'?'years':'days';draw();position();
  });button.dataset.unit=kind;variantRow.append(button);
 }
 const allButton=allLabel?press(allLabel,()=>{allSelected=true;draw()}):null;
 const confirm=press('Подтвердить',()=>{apply(allSelected?null:selected,unit);dialog.close()});confirm.className='cal-confirm';
 actions.append(press('Отмена',()=>dialog.close()),confirm);footer.append(summary,actions);
 const body=make('div');body.className='cal-body';body.append(weekdays,grid);
 dialog.append(heading);if(variantRow.childElementCount>1)dialog.append(variantRow);if(allButton)dialog.append(allButton);dialog.append(toolbar,body,footer);(document.getElementById('refRoot')||document.body).append(dialog);draw();dialog.showModal();position();
 const focus=grid.querySelector('[aria-pressed=true]:not(:disabled)')||grid.querySelector('button:not(:disabled)');focus?.focus({preventScroll:true});
 window.addEventListener('resize',position);dialog.addEventListener('click',e=>{if(e.target!==dialog)return;const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close()});
 dialog.addEventListener('close',()=>{window.removeEventListener('resize',position);dialog.remove();anchor.focus({preventScroll:true})},{once:true});
}

const nativeFields='input[type="date"],input[type="week"],input[type="month"],input[data-period-type="year"],#dailyYear,#qualityPeriod';
const controls=new WeakMap();
const sourceFields=new WeakMap();
function syncField(field){
 const trigger=controls.get(field);
 if(!trigger)return;
 const text=fieldText(field);
 if(trigger.textContent!==text)trigger.textContent=text;
 if(trigger.hidden!==field.hidden)trigger.hidden=field.hidden;
 const disabled=field.disabled||field.readOnly;if(trigger.disabled!==Boolean(disabled))trigger.disabled=Boolean(disabled);
 const label=field.getAttribute('aria-label')||[...(field.closest('label')?.childNodes||[])].filter(n=>n!==field&&n!==trigger).map(n=>n.textContent.trim()).filter(Boolean).join(' ');
 const caption=(label||'Выбрать период')+' · '+text;
 if(trigger.getAttribute('aria-label')!==caption)trigger.setAttribute('aria-label',caption);
}
function enhance(panel){
 function sync(){
  for(const field of panel.querySelectorAll(nativeFields)){
   let trigger=controls.get(field);
   if(!trigger){
    trigger=btn('',event=>{event.preventDefault();openField(field,trigger);});
    trigger.className='ref-calendar-field button date-field-trigger';trigger.setAttribute('aria-haspopup','dialog');
    controls.set(field,trigger);sourceFields.set(trigger,field);
    field.classList.add('ref-calendar-source');field.after(trigger);
   }
   syncField(field);
  }
 }
 const observer=new MutationObserver(sync);
 observer.observe(panel,{subtree:true,childList:true,attributes:true,attributeFilter:['type','disabled','hidden','value']});
 panel.addEventListener('change',sync);panel.addEventListener('input',sync);
 panel.addEventListener('focusin',event=>{if(sourceFields.has(event.target))sync();});
 sync();return sync;
}
function kindOf(field){return field.id==='dailyYear'||field.dataset.periodType==='year'?'year':({month:'month',week:'week'})[field.type]||'day';}
function fieldText(field){
 if(field.options)return field.selectedOptions[0]?.textContent||'Все периоды';
 const kind=kindOf(field),date=window.nativePeriodSelection.normalize(kind,field.value);
 if(!date)return kind==='year'?'Выбрать год':kind==='month'?'Выбрать месяц':'Выбрать дату';
 if(kind==='year')return date.slice(0,4);
 if(kind==='month')return new Intl.DateTimeFormat('ru-RU',{month:'long',year:'numeric',timeZone:'UTC'}).format(dateOf(date));
 if(kind==='week')return stamp(date)+' — '+stamp(period(date,kind)[1]);
 return stamp(date);
}
function openField(field,anchor){
 const title=anchor.getAttribute('aria-label'),fallback=window.referenceFilters?.scope?.from||serial(new Date());
 const commit=value=>{
  field.value=value;field.dispatchEvent(new Event('input',{bubbles:true}));field.dispatchEvent(new Event('change',{bubbles:true}));
  if(anchor.isConnected)anchor.textContent=fieldText(field);
 };
 if(field.options){
  const variants=new Map();
  for(const option of field.options){
   const [kind,date]=option.value.split(':');
   if(!valid(date)||option.disabled)continue;
   if(!variants.has(kind))variants.set(kind,new Set());variants.get(kind).add(date);
  }
  const [selectedKind,date]=field.value.split(':'),unit=variants.has(selectedKind)?selectedKind:variants.keys().next().value||'day';
  openCalendar({anchor,title,initial:date||variants.get(unit)?.values().next().value||fallback,unit,variants,allowed:variants.get(unit)||new Set(),allLabel:'Все периоды',allSelected:field.value==='all',
   spanFor:(v,k)=>k==='week'?[v,day(v,6)]:period(v,k),apply:(v,k)=>commit(v?k+':'+v:'all')});
  return;
 }
 const unit=kindOf(field),selection=window.nativePeriodSelection;
 const initial=selection.normalize(unit,field.value)||fallback;
 const allowed={has:value=>{const raw=selection.fromIsoDate(value,unit);return Boolean(raw)&&(!field.min||(unit==='year'?Number(raw)>=Number(field.min):raw>=field.min))&&(!field.max||(unit==='year'?Number(raw)<=Number(field.max):raw<=field.max));}};
 const optional=!field.required&&!field.dataset.periodType;
 openCalendar({anchor,title,initial,unit,allowed,allLabel:optional?(field.id==='dailyYear'?'Определить по Excel':'Очистить дату'):null,allSelected:optional&&!field.value,
  apply:value=>commit(value?selection.fromIsoDate(value,unit):'')});
}
window.referenceCalendar=Object.freeze({open:openCalendar,enhance,syncField,period,day,dateAt,dateOf,stamp,valid});
})();
