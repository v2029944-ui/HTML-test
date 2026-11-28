/* One page of SQL results is mounted at a time. Controllers accept an absolute,
   zero-based index and provide the exact count for their current query. */
(function(){
'use strict';
const states=new WeakMap();
let nextId=0;
function element(tag,text){
 const node=document.createElement(tag);
 if(text!==undefined)node.textContent=text;
 return node;
}
function button(action,title,text){
 const node=element('button',text);node.type='button';node.dataset.pageAction=action;
 node.setAttribute('aria-label',title);node.title=title;return node;
}
function paint(state){
 const {options,pages,previous,next,choose,range,total,nav}=state;
 const {index,size,count}=options;
 const page=pages?index+1:0,locked=Boolean(state.pending);
 state.bar.hidden=pages<=1;
 state.first.hidden=state.last.hidden=!options.edgeLinks||pages<3;
 state.first.setAttribute('aria-disabled',String(locked||page<=1));
 state.last.setAttribute('aria-disabled',String(locked||page>=pages));
 previous.setAttribute('aria-disabled',String(locked||page<=1));
 next.setAttribute('aria-disabled',String(locked||page>=pages));
 choose.setAttribute('aria-disabled',String(locked||pages<=1));
 choose.setAttribute('aria-label','Выбрать страницу, текущая '+page+' из '+pages);
 state.value.textContent=String(page);total.textContent='из '+pages;
 const shown=count?(index*size+1)+'–'+(index*size+count):'0';
 range.textContent=(options.axis==='x'?'Периоды ':'Позиции ')+shown+' из '+options.total;
 nav.setAttribute('aria-busy',String(locked));nav.dataset.sqlWindow=String(index);
}
function closeMenu(state,focus){
 if(state.menu.matches(':popover-open'))state.menu.hidePopover();
 if(focus)state.choose.focus({preventScroll:true});
}
function navigate(state,index,trigger){
 const {root,options,pages}=state;
 if(!options||state.pending||!Number.isSafeInteger(index)||index<0||index>=pages||index===options.index)return;
 // Keep the controls at the same position when SQL clears or shortens a page.
 if(!options.controlsOnly)root.style.minHeight='min('+Math.ceil(root.getBoundingClientRect().height)+'px, 58vh, 36rem)';
 const pending={trigger};state.pending=pending;state.error.hidden=true;paint(state);
 Promise.resolve().then(()=>state.pending===pending&&options.go(index)).then(()=>{
  if(state.pending===pending){state.pending=null;paint(state);}
 },error=>{
  if(state.pending!==pending)return;
  state.pending=null;paint(state);state.error.hidden=false;
  state.error.textContent='Не удалось загрузить страницу: '+String(error?.message||error);
 });
}
function showChoices(state){
 const {pages,options,input,choices,choose,ranges}=state,current=options.index+1;
 input.max=String(pages);input.value=String(current);input.setCustomValidity('');
 // Ordinary lists contain every page. Huge lists descend through at most
 // ten contiguous ranges per level before displaying the final 200 numbers.
 const block=200,first=Math.floor((current-1)/block)*block+1;
 const parents=pages>block?[[1,pages]]:[];
 function draw(start,end,focus){
  let step=1;
  if(end-start+1>block){
   step=block;
   while(Math.ceil((end-start+1)/step)>10)step*=10;
  }
  ranges.hidden=!parents.length;
  choices.dataset.layout=step>1||end>=1e7?'wide':end>=1e5?'medium':'standard';
  const entries=[];
  for(let n=start;n<=end;n+=step){
   const last=n+Math.min(step-1,end-n);
   if(step===1){
    const entry=button('page','Страница '+n,String(n));entry.dataset.pageNumber=String(n);
    if(n===current)entry.setAttribute('aria-current','page');
    entry.onclick=()=>{closeMenu(state,true);navigate(state,n-1,choose);};entries.push(entry);
   }else{
    const text=n+'–'+last,entry=button('range','Страницы '+text,text);
    entry.dataset.pageFrom=String(n);entry.dataset.pageTo=String(last);
    entry.onclick=()=>{parents.push([start,end]);draw(n,last,true);};entries.push(entry);
   }
  }
  choices.replaceChildren(...entries);choices.scrollTop=0;
  if(focus){positionMenu(state);entries[0].focus({preventScroll:true});}
 }
 ranges.onclick=()=>{const parent=parents.pop();if(parent)draw(...parent,true);};
 draw(first,Math.min(pages,first+block-1),false);
}
function positionMenu({menu,choose}){
 // Position against the visible viewport, including when hosted by a dialog.
 const rect=choose.getBoundingClientRect(),zoom=menu.currentCSSZoom||1;
 menu.style.setProperty('--sql-menu-zoom',String(zoom));
 const {width,height}=menu.getBoundingClientRect();
 const below=rect.bottom+4;
 menu.style.left=Math.max(4,Math.min(rect.left,document.documentElement.clientWidth-width-4))/zoom+'px';
 menu.style.top=Math.max(4,below+height<=innerHeight?below:rect.top-height-4)/zoom+'px';
}
function create(root){
 const nav=element('nav'),bar=element('div'),previous=button('previous','Предыдущая страница','‹');
 const choose=button('choose','Выбрать страницу'),value=element('span'),total=element('span'),next=button('next','Следующая страница','›');
 const range=element('span'),error=element('span'),menu=element('div'),choices=element('div');
 const label=element('label','Страница'),input=element('input'),accept=button('apply','Перейти на страницу','→');
 const cancel=button('cancel','Закрыть выбор страницы','×'),ranges=button('ranges','К диапазонам страниц','К диапазонам'),formRow=element('div');
 nav.className='sql-pager';bar.className='sql-pager-controls';
 range.dataset.pageRange='';range.setAttribute('role','status');total.dataset.pageTotal='';
 error.setAttribute('role','alert');error.hidden=true;
 menu.popover='auto';menu.id='sql-page-options-'+(++nextId);menu.className='sql-page-options';
 menu.setAttribute('role','dialog');menu.setAttribute('aria-label','Выбор страницы');
 input.type='number';input.step='1';input.min='1';input.required=true;input.inputMode='numeric';input.dataset.pageInput='';
 choices.className='sql-page-choices';formRow.className='sql-page-entry';label.append(input);formRow.append(label,accept,cancel);
 menu.append(formRow,ranges,choices);
 choose.append(value);choose.setAttribute('aria-haspopup','dialog');choose.setAttribute('aria-controls',menu.id);choose.setAttribute('aria-expanded','false');
 choose.popoverTargetElement=menu;
 const first=button('first','Первая страница','«'),last=button('last','Последняя страница','»');
 bar.append(first,previous,choose,total,next,last);nav.append(bar,range,error,menu);
 const state={nav,bar,root,first,last,previous,next,choose,range,total,error,menu,input,value,choices,ranges,pages:0,pending:null,options:null};
 first.onclick=()=>navigate(state,0,first);last.onclick=()=>navigate(state,state.pages-1,last);
 previous.onclick=()=>navigate(state,state.options.index-1,previous);next.onclick=()=>navigate(state,state.options.index+1,next);
 choose.onclick=event=>{
  event.preventDefault();
  if(choose.getAttribute('aria-disabled')==='true')return;
  if(menu.matches(':popover-open')){closeMenu(state,true);return;}
  showChoices(state);
  menu.showPopover();
  positionMenu(state);
  const selected=choices.querySelector('[aria-current=page]');
  if(selected)choices.scrollTop+=selected.getBoundingClientRect().top-choices.getBoundingClientRect().top;
  input.focus({preventScroll:true});input.select();
 };
 menu.addEventListener('beforetoggle',event=>choose.setAttribute('aria-expanded',String(event.newState==='open')));
 choices.addEventListener('keydown',event=>{
  const entries=Array.from(choices.children),index=entries.indexOf(event.target);
  if(index<0)return;
  const columns=Number(getComputedStyle(choices).getPropertyValue('--sql-choice-columns'));
  const moves={ArrowLeft:-1,ArrowRight:1,ArrowUp:-columns,ArrowDown:columns,Home:-index,End:entries.length-1-index};
  const move=moves[event.key];
  if(!Number.isFinite(move))return;
  event.preventDefault();
  entries[Math.max(0,Math.min(entries.length-1,index+move))]?.focus();
 });
 function apply(){
  if(!input.reportValidity())return;
  const index=input.valueAsNumber-1;
  if(!Number.isSafeInteger(index))return;
  closeMenu(state,true);navigate(state,index,choose);
 }
 accept.onclick=apply;cancel.onclick=()=>closeMenu(state,true);
 menu.addEventListener('keydown',event=>{
  if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeMenu(state,true);}
  if(event.key==='Enter'&&event.target===input){event.preventDefault();event.stopPropagation();apply();}
 });
 return state;
}
function attach(root,options){
 if(!root?.parentElement)return null;
 const {index,size,count,total,go,footer}=options;
 if(!Number.isSafeInteger(index)||index<0||!Number.isSafeInteger(size)||size<1||
    !Number.isSafeInteger(total)||total<=0||!Number.isSafeInteger(count)||count<0||count>size||
    typeof go!=='function'){
  reset(root);return null;
 }
 let state=states.get(root);
 if(!state){state=create(root);states.set(root,state);}
 const {nav,pending}=state;
 if(!pending&&!options.controlsOnly)root.style.minHeight='';
 closeMenu(state,false);state.pending=null;state.options=options;state.pages=Math.ceil(total/size);
 if(footer){if(nav.parentNode!==footer)footer.append(nav);}
 else if(root.nextElementSibling!==nav)root.after(nav);
 if(!options.controlsOnly){root.classList.add('sql-page-content');root.dataset.pageAxis=options.axis||'y';}
 nav.dataset.sqlPager=root.dataset.sqlScrollHost||root.id||'';
 nav.setAttribute('aria-label',options.label||'Страницы таблицы');nav.hidden=false;state.error.hidden=true;paint(state);
 if(pending&&(!document.activeElement||document.activeElement===document.body))
  pending.trigger.focus({preventScroll:true});
 return nav;
}
function reset(root){
 const state=states.get(root);if(!state)return;
 if(state.options&&!state.options.controlsOnly)root.style.minHeight='';
 closeMenu(state,false);state.options=null;state.pending=null;state.nav.hidden=true;
}
function retire(...controls){for(const node of controls){node.hidden=true;node.classList.add('sql-obsolete-control');}}
window.sqlRowScroll={attach,reset,retire};
})();
