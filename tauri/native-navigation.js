// Tauri-only workspace navigation. A tab only changes visible SQL panels;
// it never reads report JSON or switches to the legacy renderer.
(function nativeWorkspaceNavigation(){
 'use strict';
 const links=Array.from(document.querySelectorAll('[data-nav-link]'));
 const panels=Array.from(document.querySelectorAll('[data-nav-panel]'));
 const status=document.getElementById('nativeViewStatus');
 if(!links.length||!panels.length||!status)return;
 const labels=new Map(links.map(link=>[link.getAttribute('data-nav-link'),link.textContent.trim()]));
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
  for(const panel of panels)panel.hidden=panel.getAttribute('data-nav-panel')!==target;
  for(const link of links){
   if(link.getAttribute('data-nav-link')===target)link.setAttribute('aria-current','page');
   else link.removeAttribute('aria-current');
  }
  status.textContent='Раздел: '+labels.get(target)+'. Остальные данные доступны через меню выше.';
  return target;
 }
 function sync(){select(selectedFromHash());}
 window.addEventListener('hashchange',sync);
 sync();
 window.nativeWorkspaceNavigation=Object.freeze({select,active:()=>current});
})();
