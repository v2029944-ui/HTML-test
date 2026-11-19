// Tauri-only workspace navigation. A tab only changes visible SQL panels;
// it never reads report JSON or switches to the legacy renderer.
(function nativeWorkspaceNavigation(){
 'use strict';
 const links=Array.from(document.querySelectorAll('[data-nav-link]'));
 const panels=Array.from(document.querySelectorAll('[data-nav-panel]'));
 const status=document.getElementById('nativeViewStatus');
 if(!links.length||!panels.length||!status)return;
 const labels=new Map(links.map(link=>[link.getAttribute('data-nav-link'),(link.querySelector?.('span')?.textContent||link.textContent).trim()]));
 const keys=new Set(labels.keys());
 let current='analysis';
 function selectedFromHash(){
  const hash=typeof window.location.hash==='string'?window.location.hash:'';
  const name=hash.startsWith('#')?hash.slice(1):'';
  return keys.has(name)?name:'analysis';
 }
 function select(name){
  const target=keys.has(name)?name:'analysis';
  current=target;
  for(const panel of panels)panel.hidden=!panel.getAttribute('data-nav-panel').split(/\s+/).includes(target);
  for(const link of links){
   if(link.getAttribute('data-nav-link')===target)link.setAttribute('aria-current','page');
   else link.removeAttribute('aria-current');
  }
  const menu=document.getElementById('managementNav'),toggle=document.getElementById('managementToggle');
  toggle?.classList?.toggle('active',['reports','groups','backup','excel','metrics'].includes(target));
  if(menu)menu.open=false;
  status.textContent='Раздел: '+labels.get(target)+'. Остальные разделы доступны через меню «Управление».';
  return target;
 }
 // Update visibility synchronously on a real tab click. Hashchange is
 // asynchronous in WebView2, so consumers must not briefly see the old tab.
 for(const link of links)link.addEventListener('click',()=>{
  const name=link.getAttribute('data-nav-link');
  if(keys.has(name))select(name);
 });
 function sync(){select(selectedFromHash());}
 window.addEventListener('hashchange',()=>{
  sync();
  // A newly visible section must start at the top rather than retaining
  // a deep scroll position from an unrelated section.
  if(typeof window.scrollTo==='function')window.scrollTo(0,0);
 });
 document.addEventListener?.('keydown',event=>{if(event.key==='Escape'){const menu=document.getElementById('managementNav');if(menu?.open){menu.open=false;document.getElementById('managementToggle')?.focus();}}});
 document.addEventListener?.('click',event=>{const menu=document.getElementById('managementNav');if(menu?.open&&!menu.contains(event.target))menu.open=false;});
 sync();
 window.nativeWorkspaceNavigation=Object.freeze({select,active:()=>current});
})();
