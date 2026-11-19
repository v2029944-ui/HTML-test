// Organization selection for the SQLite analysis and daily calendar scopes.
(function(){
'use strict';
const select=document.getElementById('analysisOrganization');
const choices=document.getElementById('analysisOrganizationChoices');
const all='*',custom='__custom__';
const make=(tag,text)=>{const node=document.createElement(tag);if(text!==undefined)node.textContent=text;return node;};
let available=[],subset=null,options=new Map(),inputs=new Map(),customOption;
let initialized=false,locked=true;
const selected=()=>subset===null?available.slice():available.filter(name=>subset.has(name));

function synchronize(){
 const names=selected();
 const value=subset===null?all:names.length===1?options.get(names[0]):custom;
 customOption.textContent=names.length?'Выбрано несколько':'Ничего не выбрано';
 customOption.hidden=value!==custom;
 select.value=value;
 for(const [name,input] of inputs)input.checked=subset===null||subset.has(name);
}
function setLocked(value){
 locked=!!value;
 select.disabled=locked||!available.length;
 choices.disabled=select.disabled;
}
function changeSelection(names,followAll){
 const previous=selected();
 subset=followAll?null:new Set(names);
 synchronize();
 if(previous.length!==names.length||previous.some(name=>!names.includes(name)))
  window.nativeWorkspaceFilterChanged?.();
}

function setOrganizations(names){
 const initialValue=select.value;
 available=names.slice();
 if(!initialized){
  initialized=true;
  if(initialValue!==all&&available.includes(initialValue))subset=new Set([initialValue]);
 }else if(subset!==null){
  subset=new Set(available.filter(name=>subset.has(name)));
 }
 options=new Map();inputs=new Map();
 const allOption=make('option','Все из настроек');allOption.value=all;
 customOption=make('option');customOption.value=custom;customOption.disabled=true;
 select.replaceChildren(allOption);
 choices.replaceChildren(make('legend','Выбрать несколько подразделений'));
 const reserved=new Set([all,custom,...available]);
 for(const name of available){
  let value=name;
  if(name===all||name===custom){
   value='organization:'+name;
   while(reserved.has(value))value='organization:'+value;
   reserved.add(value);
  }
  const option=make('option',name);option.value=value;
  select.append(option);options.set(name,value);
  const label=make('label'),input=make('input');
  input.type='checkbox';
  input.addEventListener('change',()=>{
   if(locked||inputs.get(name)!==input){synchronize();return;}
   const names=new Set(selected());
   if(input.checked)names.add(name);
   else names.delete(name);
   // Checking every box restores the dynamic all-organizations mode.
   changeSelection(Array.from(names),names.size===available.length);
  });
  label.append(input,make('span',name));choices.append(label);inputs.set(name,input);
 }
 select.append(customOption);
 synchronize();setLocked(locked);
}

select.addEventListener('change',()=>{
 if(!initialized)return;
 if(!locked){
  if(select.value===all){changeSelection(available,true);return;}
  for(const [name,value] of options){
   if(select.value===value){changeSelection([name],false);return;}
  }
 }
 synchronize();
});
window.nativeOrganizationFilter=Object.freeze({selected,setOrganizations,setLocked});
})();
