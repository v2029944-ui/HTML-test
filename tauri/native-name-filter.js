// Exact, bounded work/group selector for the Tauri SQL workspace.
(function(){
'use strict';
const $=id=>document.getElementById(id),api=window.desktopAPI;
const make=(tag,text)=>{const el=document.createElement(tag);if(text!==undefined)el.textContent=String(text);return el;};
const limit=50;
let revision=undefined,request=0,busy=false,locked=false,options=new Set();
const selected=new Set();
const validName=name=>typeof name==='string'&&name.length>0&&
 new TextEncoder().encode(name).length<=512;
const notify=()=>window.nativeWorkspaceFilterChanged?.();
const unavailable=()=>busy||locked||revision===null;
const selectedNames=()=>Array.from(selected).sort();
const selectedName=()=>selected.size===1?selected.values().next().value:null;
function control(){
 const disabled=unavailable(),name=$('analysisName').value;
 $('analysisNameSearchRun').disabled=disabled;
 $('analysisNameSearch').disabled=disabled;
 $('analysisName').disabled=disabled;
 $('analysisNameAdd').disabled=disabled||!options.has(name)||selected.has(name)||selected.size>=limit;
 $('analysisNameReset').disabled=disabled||selected.size===0;
 for(const row of $('analysisNameSelected').children)row.children[1].disabled=disabled;
}
function status(message,error=false){
 $('analysisNameStatus').textContent=message;
 $('analysisNameStatus').className='filter-scope'+(error?' error':'');
}
function selectionStatus(){
 return selected.size===0?'Фильтр сброшен: все работы и группы.':
  'Выбрано работ и групп: '+selected.size+' из '+limit+'.'+
  (selected.size===limit?' Достигнут предел. Удалите позицию, чтобы добавить другую.':'');
}
function displaySelection(){
 const list=$('analysisNameSelected');
 list.replaceChildren();
 list.hidden=selected.size===0;
 for(const name of selected){
  const row=make('li'),remove=make('button','Удалить');
  remove.type='button';
  remove.setAttribute?.('aria-label','Удалить из фильтра: '+name);
  remove.addEventListener('click',()=>{
   if(unavailable()||!selected.delete(name))return;
   selectionChanged();
   $('analysisName')?.focus?.();
  });
  row.append(make('span',name),remove);
  list.append(row);
 }
}
function selectionChanged(){
 displaySelection();control();status(selectionStatus());notify();
}
function reset(){
 if(unavailable()||!selected.size)return;
 selected.clear();
 $('analysisName').value='';
 selectionChanged();
 $('analysisName')?.focus?.();
}
function displayOptions(entries){
 const select=$('analysisName');
 const placeholder=make('option','Выберите название');placeholder.value='';
 select.replaceChildren(placeholder);
 options=new Set();
 for(const item of entries){
  options.add(item.name);
  const opt=make('option',(item.kind==='group'?'Группа: ':'Работа: ')+item.name);
  opt.value=item.name;select.append(opt);
 }
 select.value='';
}
function setRevision(rev){
 if(rev===revision)return;
 request++;
 revision=Number.isSafeInteger(rev)&&rev>=0?rev:null;
 selected.clear();busy=false;
 $('analysisNameSearch').value='';
 displayOptions([]);displaySelection();
 status(revision===null?'Для поиска требуется открытая база SQLite.':
  'Найдите точное название и добавьте его в фильтр. Можно выбрать до '+limit+' работ и групп.');
 control();
}
async function search(){
 if(unavailable())return;
 const text=$('analysisNameSearch').value;
 if(typeof text!=='string'||new TextEncoder().encode(text).length>128){
  status('Поисковый запрос должен быть не длиннее 128 байт.',true);
  return;
 }
 busy=true;control();
 const ticket=++request,rev=revision;
 try{
  const response=await api.nativeAnalysisNameOptions(rev,text,limit);
  if(ticket!==request||revision!==rev)return;
  if(!response||!Array.isArray(response.rows)||response.rows.length>limit||
     typeof response.has_more!=='boolean')throw Error('Некорректная страница названий SQLite');
  const seen=new Set();
  for(const item of response.rows){
   if(!item||!validName(item.name)||!['group','work'].includes(item.kind)||
      seen.has(item.name))throw Error('Некорректные или неоднозначные названия');
   seen.add(item.name);
  }
  displayOptions(response.rows);
  status('Найдено и показано: '+response.rows.length+
   (response.has_more?'. Есть ещё совпадения: уточните запрос.':'.')+
   (selected.size?' '+selectionStatus():' Для применения выберите и добавьте название из списка.')+
   ' Поиск учитывает написание и регистр русских букв.');
 }catch(err){
  if(ticket===request)status('Поиск работ/групп недоступен: '+String(err?.message||err),true);
 }finally{
  if(ticket===request){busy=false;control();}
 }
}
function add(){
 if(unavailable())return;
 if(selected.size>=limit){status(selectionStatus(),true);return;}
 const name=$('analysisName').value;
 if(!options.has(name)||selected.has(name))return;
 selected.add(name);
 $('analysisName').value='';
 selectionChanged();
 $('analysisName')?.focus?.();
}
$('analysisNameSearchRun').addEventListener('click',()=>{void search();});
$('analysisNameReset').addEventListener('click',reset);
$('analysisNameAdd').addEventListener('click',add);
$('analysisName').addEventListener('change',control);
$('analysisNameSearch').addEventListener('keydown',event=>{
 if(event.key==='Enter'){
  event.preventDefault();
  void search();
 }
});
setRevision(null);
window.nativeNameFilter=Object.freeze({selectedName,selectedNames,setRevision,search,reset,
 setLocked(value){locked=!!value;control();}
});
})();
