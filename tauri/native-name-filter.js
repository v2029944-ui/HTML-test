// Exact, bounded work/group selector for the Tauri SQL workspace.
(function(){
'use strict';
const $=id=>document.getElementById(id),api=window.desktopAPI;
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=String(text);return el;};
let revision=null,selected=null,request=0,busy=false,locked=false;
const validName=name=>typeof name==='string'&&name.length>0&&
 new TextEncoder().encode(name).length<=512;
const notify=()=>window.nativeWorkspaceFilterChanged?.();
function selectedName(){return selected;}
function control(){
 $('analysisNameSearchRun').disabled=busy||locked||revision===null;
 $('analysisNameSearch').disabled=busy||locked||revision===null;
 $('analysisName').disabled=busy||locked||revision===null;
 $('analysisNameReset').disabled=busy||locked||revision===null||selected===null;
}
function status(message,error=false){
 $('analysisNameStatus').textContent=message;
 $('analysisNameStatus').className='filter-scope'+(error?' error':'');
}
function reset(){
 if(locked||busy||selected===null)return;
 selected=null;
 $('analysisName').value='*';
 status('Фильтр сброшен: все работы и группы.');
 control();notify();
}
function setRevision(rev){
 if(rev===revision)return;
 request++;
 revision=Number.isSafeInteger(rev)&&rev>=0?rev:null;
 selected=null;busy=false;
 $('analysisNameSearch').value='';
 const select=$('analysisName');
 select.replaceChildren();
 const all=make('option','Все работы и группы');all.value='*';select.append(all);
 select.value='*';
 status(revision===null?'Для поиска требуется открытая база SQLite.':
  'Поиск точного названия работы или группы в SQLite. Сначала выберите вариант, затем пересчитайте аналитику.');
 control();
}
async function search(){
 if(busy||locked||revision===null)return;
 const text=$('analysisNameSearch').value;
 if(typeof text!=='string'||new TextEncoder().encode(text).length>128){
  status('Поисковый запрос должен быть не длиннее 128 байт.',true);
  return;
 }
 busy=true;control();
 const ticket=++request,rev=revision;
 try{
  const response=await api.nativeAnalysisNameOptions(rev,text,50);
  if(ticket!==request||revision!==rev)return;
  if(!response||!Array.isArray(response.rows)||response.rows.length>50||
     typeof response.has_more!=='boolean')throw Error('Некорректная страница названий SQLite');
  const seen=new Set(),entries=[];
  for(const item of response.rows){
   if(!item||!validName(item.name)||!['group','work'].includes(item.kind)||
      seen.has(item.name))throw Error('Некорректные или неоднозначные названия');
   seen.add(item.name);entries.push(item);
  }
  // A search does not silently remove an active filter. Keep its current
  // exact name visible even when it does not match the new search string.
  const select=$('analysisName');
  select.replaceChildren();
  const all=make('option','Все работы и группы');all.value='*';select.append(all);
  if(selected!==null&&!seen.has(selected)){
   const opt=make('option','Выбрано: '+selected);opt.value=selected;select.append(opt);
  }
  for(const item of entries){
   const opt=make('option',(item.kind==='group'?'Группа: ':'Работа: ')+item.name);
   opt.value=item.name;select.append(opt);
  }
  select.value=selected??'*';
  status('Найдено и показано: '+entries.length+
   (response.has_more?'. Есть ещё совпадения: уточните запрос.':'.')+
   (selected? ' Применён фильтр: '+selected+'.':' Для применения выберите позицию из списка.')+
   ' Поиск учитывает написание и регистр русских букв.');
 }catch(err){
  if(ticket===request)status('Поиск работ/групп недоступен: '+String(err?.message||err),true);
 }finally{
  if(ticket===request){busy=false;control();}
 }
}
function selectionChanged(){
 if(locked||busy)return;
 const option=$('analysisName').value;
 const name=option==='*'?null:option;
 if(name!==null&&!validName(name)){
  status('Недопустимое название: выберите существующий пункт.',true);
  $('analysisName').value=selected??'*';
  return;
 }
 // No direct typing: a native <select> only permits validated SQL results.
 if(name===selected)return;
 selected=name;
 control();
 status(selected?'Выбрана работа/группа: '+selected+'. Этот фильтр действует на все результаты аналитики и календаря.':
  'Все работы и группы: фильтр не применяется.');
 notify();
}
$('analysisNameSearchRun').addEventListener('click',()=>{void search();});
$('analysisNameReset').addEventListener('click',reset);
$('analysisName').addEventListener('change',selectionChanged);
$('analysisNameSearch').addEventListener('keydown',event=>{
 if(event.key==='Enter'){
  event.preventDefault();
  void search();
 }
});
setRevision(null);
window.nativeNameFilter=Object.freeze({selectedName,setRevision,search,reset,
 setLocked(value){locked=!!value;control();}
});
})();