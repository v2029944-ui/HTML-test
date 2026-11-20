/* Native scrollbars over bounded SQL results. Offset queries can seek directly.
   Keyset queries expose one further window once the server supplies its cursor. */
(function(){
'use strict';
const states=new WeakMap();
function attach(root,options){
 if(!root?.parentElement)return null;
 const {index,size,count,hasNext,direct=false,total,axis='y',label,go}=options;
 if(!Number.isSafeInteger(index)||index<0||!Number.isSafeInteger(size)||size<1||
    !Number.isSafeInteger(count)||count<0||typeof go!=='function')return null;
 const horizontal=axis==='x';
 let state=states.get(root);
 if(!state){
  const viewport=document.createElement('div'),before=document.createElement('div'),after=document.createElement('div'),body=document.createElement('div');
  viewport.className='sql-scroll';viewport.dataset.sqlRowScroll=axis;viewport.tabIndex=0;viewport.setAttribute('role','region');
  for(const gap of [before,after]){gap.className='sql-scroll-gap';gap.setAttribute('aria-hidden','true');}
  body.className='sql-scroll-page';root.before(viewport);viewport.append(before,body,after);body.append(root);root.classList.add('sql-scroll-content');
  state={viewport,before,after,body,step:0,target:null,queued:0,options:null};states.set(root,state);
  viewport.addEventListener('keydown',event=>{
   if(event.target!==viewport||!state.options||event.altKey||event.metaKey)return;
   const horizontal=state.options.horizontal,offset=horizontal?'scrollLeft':'scrollTop';
   const visible=horizontal?viewport.clientWidth:viewport.clientHeight;
   const maximum=(horizontal?viewport.scrollWidth:viewport.scrollHeight)-visible;
   const delta={PageDown:visible,PageUp:-visible,[horizontal?'ArrowRight':'ArrowDown']:40,[horizontal?'ArrowLeft':'ArrowUp']:-40}[event.key];
   if(delta===undefined&&event.key!=='Home'&&event.key!=='End')return;
   event.preventDefault();
   viewport[offset]=event.key==='Home'?0:event.key==='End'?maximum:viewport[offset]+delta;
  });
  viewport.addEventListener('scroll',()=>{
   if(!state.options||state.queued)return;
   state.queued=requestAnimationFrame(()=>{state.queued=0;onScroll(state);});
  },{passive:true});
 }
 const {viewport,before,after,body}=state;
 const prior=state.options,requested=state.target,offsetProp=horizontal?'scrollLeft':'scrollTop',length=horizontal?'width':'height';
 const visible=horizontal?viewport.clientWidth:viewport.clientHeight;
 const maxVisible=horizontal?visible:(parseFloat(getComputedStyle(viewport).maxHeight)||visible);
 body.style.minHeight=body.style.minWidth='';
 const measured=horizontal?Math.max(body.scrollWidth,root.scrollWidth):body.getBoundingClientRect().height;
 const multi=index>0||hasNext;
 viewport.tabIndex=multi||measured>maxVisible?0:-1;
 if(!state.step||(!requested&&index===0)||(!prior&&measured))state.step=Math.max(measured,maxVisible+80,size*(horizontal?90:32));
 const pages=direct&&Number.isSafeInteger(total)?Math.max(1,Math.ceil(total/size)):index+1+(hasNext?1:0);
 // Stay below WebView2's maximum layout dimension for very large databases.
 const stride=Math.min(state.step,Math.max(1,Math.floor(8000000/pages)));
 const boundary=Math.max(80,maxVisible*.6,maxVisible-measured+80);
 const head=direct?index*stride:(index?boundary:0);
 const tail=direct?(pages-index-1)*stride:(hasNext?boundary:0);
 if(index>0&&!hasNext&&measured<maxVisible)body.style[horizontal?'minWidth':'minHeight']=Math.ceil(maxVisible)+'px';
 before.style[length]=Math.max(0,head)+'px';after.style[length]=Math.max(0,tail)+'px';
 viewport.setAttribute('aria-label',label||'Прокрутка строк');
 viewport.dataset.sqlScrollHost=root.dataset.sqlScrollHost||root.id||'';
 viewport.dataset.sqlRowScroll=axis;viewport.dataset.sqlWindow=String(index);viewport.classList.toggle('sql-scroll-multiple',Boolean(multi));
 state.options={...options,horizontal,head,stride,pages};state.target=null;
 let position=head;
 if(requested&&requested.index===index){
  if(requested.edge==='first')position=head;
  else if(requested.edge==='last'||requested.from>index)position=head+Math.max(0,measured-visible);
  else if(direct&&requested.from!==index-1)position+=Math.min(requested.inside,Math.max(0,measured-visible));
 }else if(prior&&index===prior.index&&index>0)position=head+Math.max(0,viewport[offsetProp]-prior.head);
 viewport[offsetProp]=Math.max(0,position);
 if(!multi){before.style[length]=after.style[length]='0px';}
 return viewport;
}
function onScroll(state){
 const opts=state.options;if(!opts||state.target)return;
 const {viewport,body}=state,{horizontal,index,head,stride,pages,hasNext,direct,go}=opts;
 const position=horizontal?viewport.scrollLeft:viewport.scrollTop,visible=horizontal?viewport.clientWidth:viewport.clientHeight;
 if(!visible)return;
 const extent=horizontal?body.scrollWidth:body.getBoundingClientRect().height;
 let next=index;
 if(index>0&&position<head-2)next=direct?Math.max(0,index-Math.ceil((head-position)/stride)):index-1;
 else if(hasNext&&position>head+Math.max(0,extent-visible)+2)next=direct?Math.min(pages-1,index+Math.ceil((position-head-Math.max(0,extent-visible))/stride)):index+1;
 if(next===index)return;
 const limit=(horizontal?viewport.scrollWidth:viewport.scrollHeight)-visible;
 const edge=direct&&position<1?'first':direct&&position>=limit-1?'last':null;
 state.target={index:next,from:index,inside:Math.max(0,position-next*stride),edge};
 // Clearing the old rows for asynchronous SQL must not collapse the viewport.
 body.style[horizontal?'minWidth':'minHeight']=Math.ceil(extent)+'px';
 if(body.contains(document.activeElement))viewport.focus({preventScroll:true});
 const pending=state.target;
 function rollback(){
  if(state.target!==pending)return false;
  state.target=null;body.style.minHeight=body.style.minWidth='';
  viewport[horizontal?'scrollLeft':'scrollTop']=next>index?head+Math.max(0,extent-visible):head;
  return true;
 }
 Promise.resolve().then(()=>state.target===pending?go(next):false).then(moved=>{
  if(moved===false)rollback();
 },error=>{
  if(!rollback())return;
  const message='Не удалось загрузить строки: '+String(error?.message||error);
  if(window.unifiedUI?.notice)window.unifiedUI.notice(message,true);
  else{const alert=document.createElement('p');alert.role='alert';alert.textContent=message;body.append(alert);}
 });
}
function reset(root){
 const state=states.get(root);if(!state)return;
 state.options=null;state.target=null;state.step=0;
 for(const n of [state.before,state.after]){n.style.height=n.style.width='0px';}
 state.body.style.minHeight=state.body.style.minWidth='';
 state.viewport.scrollTop=state.viewport.scrollLeft=0;state.viewport.classList.remove('sql-scroll-multiple');delete state.viewport.dataset.sqlWindow;
}
function retire(...controls){for(const control of controls){control.hidden=true;control.classList.add('sql-obsolete-control');}}
window.sqlRowScroll={attach,reset,retire};
})();
