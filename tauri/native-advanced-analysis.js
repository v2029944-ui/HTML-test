(function(){
'use strict';
const $=id=>document.getElementById('advanced'+id),api=window.desktopAPI;
const metrics={amount:'Сумма',count:'Количество / объём',average:'Средняя сумма на единицу'};
const isComposition=mode=>mode==='structure'||mode==='contributions';
const periods={day:'день',week:'неделя',month:'месяц',year:'год'};
const statuses={new:'Новая',gone:'Исчезла',existing:'Существующая',frequent:'Часто',rare:'Редко',rarer:'Стала реже',disappeared:'Исчезла',dormant:'Долгий перерыв',unknown:'Нет данных',occasional:'Периодически',uncertain:'Неполные данные',absent:'Не выполнялась',normal:'Обычно',high:'Выше',low:'Ниже',insufficient:'Недостаточно данных'};
let onStateChange=()=>{};
let scope=null,revision=null,serial=0,busy=false,page=null,positions=[null],captured=null;
const format=n=>Number.isFinite(n)?n.toLocaleString('ru-RU',{maximumSignificantDigits:12}):'—';
function controls(){
 const mode=$('Mode').value,changes=mode==='changes',regular=mode==='regularity';
 for(const id of ['MethodLabel','AmountLabel','CountLabel','Export'])$(id).hidden=!changes;
 const composition=isComposition(mode);
 $('BaselineHelp').hidden=!composition;
 const avg=$('Metric').querySelector?.('option[value="average"]');if(avg)avg.disabled=composition;
 if(composition && $('Metric').value==='average')$('Metric').value='amount';
 $('WindowLabel').hidden=changes||composition;$('PercentLabel').hidden=regular||composition;$('MetricLabel').hidden=regular;
 $('WindowTitle').textContent=regular?'Длительность перерыва, периодов':'Базовых периодов';
 $('Run').disabled=!scope||busy;$('Prev').disabled=busy||positions.length===1;
 $('Next').disabled=busy||!page?.next;$('Export').disabled=busy||!page||!changes;
 $('Page').textContent=page?'Страница '+positions.length:'';
 onStateChange();
}
function reset(){
 serial++;busy=false;page=null;captured=null;positions=[null];
 $('Head').replaceChildren();$('Rows').replaceChildren();$('Context').textContent='';
 $('Status').textContent=scope?'Выберите параметры и рассчитайте анализ.':'Сначала рассчитайте диапазон.';
 controls();
}
function row(root,values,tag){
 const tr=document.createElement('tr');
 for(const value of values){const cell=document.createElement(tag);cell.textContent=value;if(tag==='th')cell.scope='col';tr.append(cell);}
 root.append(tr);
}
function display(result,mode,request,baseline){
 const metric=$('Metric').value,q=request.query,changes=mode==='changes',regular=mode==='regularity';
 const structure=mode==='structure',composition=isComposition(mode);
 const headers=structure?['Работа / группа','Было','Стало','Доля раньше, %','Доля сейчас, %','Накопленная доля, %','Изменение, п. п.']:
 mode==='contributions'?['Работа / группа','Было','Стало','Разница','Изменение, %','Вклад в общее изменение, %']:regular?['Работа / группа','Периоды с работой','Частота, %','Базовая частота, %','Первый раз','Последний раз','Перерыв','Статус']:
 [...(changes?['База','Текущий период','Подразделение']:[]),'Работа / группа','Было','Стало','Разница','Изменение, %','Статус'];
 $('Head').replaceChildren();$('Rows').replaceChildren();row($('Head'),headers,'th');
 for(const item of result.rows){
   const label=item.name+(item.kind==='group'?' (группа)':'');
   if(composition){
     const fields=structure?['base','current','baseShare','share','cumulative','shareChange']:['base','current','delta','percent','contribution'];
     row($('Rows'),[label,...fields.map(key=>format(item[key]))],'td');
   }else if(regular)row($('Rows'),[label,format(item.hits),format(item.frequency),format(item.previousFrequency),item.first||'—',item.last||'—',format(item.trailing),item.statuses.map(s=>statuses[s]).join(', ')],'td');
   else{
     const m=item[metric];
     const flags=changes?[statuses[item.status],m.significant?'Значимое изменение':''].filter(Boolean).join(' · '):statuses[m.status];
     row($('Rows'),[...(changes?[item.baseStart,item.currentStart,item.org]:[]),label,format(m.base),format(m.current),format(m.delta),format(m.percent),flags],'td');
   }
 }
 const desc=request.displayType,filters=' · '+periods[desc]+' · '+(request.source==='daily'?'дневные отчёты':'импортированные отчёты')+' · Подразделения: '+request.organizations.join(', ')+' · Работы и группы: '+(request.selectedNames.join(', ')||'все');
 let context=$('Mode').selectedOptions[0].textContent+' · '+q.from+' — '+q.to+filters;
 if(!regular)context+=' · '+metrics[metric];
 if(!regular&&!composition)context+=' · Порог '+format(q.percent)+'%';
 if(changes){
   context+=' · '+$('Method').selectedOptions[0].textContent+' · Минимальные изменения количества и суммы: '+format(q.min_count)+' / '+format(q.min_amount);
   if(result.first_base)context+=' · Первая база: '+result.first_base+' · Последний период: '+result.last_current;
   const s=result.summary;
   $('Status').textContent='Сопоставимых строк: '+s.rows+'. Пропущено пар подразделение–период: '+s.missing_pairs+'. Пар с замечаниями импорта: '+s.import_warning_pairs+'. Некорректных отчётов: '+s.invalid_reports+', дубликатов: '+s.duplicate_issues+', пересечений: '+s.overlap_issues+'.';
   for(const pair of result.missing_examples||[]){
     const dates=[pair.missing_base?pair.base_start:null,pair.missing_current?pair.current_start:null].filter(Boolean);
     $('Status').textContent+=' '+pair.org+' — нет полного отчёта за '+dates.join(', ')+'.';
   }
 }else if(composition){
   context+=' · База: '+baseline.from+' — '+baseline.to;
   const check=result.check;
   $('Status').textContent=!result.currentComplete?'Текущий диапазон неполон.':
     structure?('Итог: '+format(result.total)+'. '+(!result.sharesValid?'Доли скрыты из-за отрицательных значений или неположительного итога. ':'')+(!result.baseComplete?'Базовый диапазон неполон.':!result.baseSharesValid?'Доли базы недоступны.':'')):
     !result.baseComplete?'Базовый диапазон неполон.':'Общее изменение: '+format(check.delta)+'. Сумма вкладов: '+format(check.sum)+'. '+(check.reconciled?'Итоги совпадают.':'Расхождение: '+format(check.residual));
   if(result.increases){
     const top=entries=>entries.map(([name,delta])=>name+' ('+format(delta)+')').join(', ')||'нет';
     $('Status').textContent+=' Наибольший рост: '+top(result.increases)+'. Наибольшее снижение: '+top(result.decreases)+'.';
   }
 }else{
   context+=' · База: '+result.baseline_from+' — '+result.baseline_to+' · Анализ: '+result.current_from+' — '+result.current_to;
   if(regular)context+=' · Долгий перерыв: '+$('Window').value+' периодов';
   $('Status').textContent='Полных периодов '+(regular?'в диапазоне':'в базе')+': '+result.used+' из '+result.expected+'. '+(regular?'Полных периодов в базе: '+result.previous_used+'.':'Для оценки обычного уровня нужны хотя бы два полных базовых периода.')+(result.current_complete?'':' Текущий диапазон неполон.');
 }
 $('Context').textContent=context;
}
function number(id){const raw=$(id).value.trim(),n=Number(raw);if(!raw||!Number.isFinite(n)||n<0)throw Error('threshold');return n;}
async function read(nextPositions){
 if(!scope||busy)return;
 const ticket=++serial,mode=$('Mode').value;
 busy=true;controls();
 try{
   const request={revision,source:scope.source,displayType:scope.displayType,organizations:scope.organizations,selectedNames:scope.selectedNames,
     query:{method:$('Method').value,from:scope.from,to:scope.to,percent:mode==='regularity'||isComposition(mode)?0:number('Percent'),min_count:mode==='changes'?number('Count'):0,min_amount:mode==='changes'?number('Amount'):0}};
   const cursor=nextPositions.at(-1);
   let before=null;
   if(isComposition(mode)){
     before=window.nativePeriodSelection.range(scope.displayType,document.getElementById('comparisonBeforeFrom').value,document.getElementById('comparisonBeforeTo').value,scope.source);
     if(!before)throw Error('baseline');
   }
   const result=before?await api.nativeCompositionPage(request,{from:before.from,to:before.to},mode,$('Metric').value,cursor):mode==='changes'?await api.nativeChangesPage(request,cursor):await api.nativePatternPage(request,mode,Number($('Window').value),cursor);
   if(ticket!==serial)return;
   page=result;captured=request;positions=nextPositions;display(result,mode,request,before);
 }catch(e){
   if(ticket===serial){reset();$('Status').textContent=e.message==='baseline'?'Укажите корректные базовые даты выше.':e.message==='threshold'?'Укажите неотрицательные числовые пороги.':'Ошибка анализа. '+window.analizUserError(e);}
 }finally{if(ticket===serial){busy=false;controls();}}
}
async function exportRows(){
 if(!captured||busy)return;
 const request=captured,ticket=serial;
 busy=true;controls();
 try{
   const path=await api.chooseCsvPath('izmeneniya-'+request.query.from+'-'+request.query.to+'.csv');
   if(!path||ticket!==serial)return;
   await api.nativeChangesCsv(request,path);
   if(ticket===serial)$('Status').textContent+=' CSV сохранён.';
 }catch(e){if(ticket===serial)$('Status').textContent+=' Ошибка экспорта. '+window.analizUserError(e);}
 finally{if(ticket===serial){busy=false;controls();}}
}
for(const id of ['comparisonBeforeFrom','comparisonBeforeTo'])for(const event of ['input','change'])
 document.getElementById(id).addEventListener(event,()=>{if(isComposition($('Mode').value))reset();});
for(const id of ['Mode','Method','Window','Metric'])$(id).addEventListener('change',reset);
for(const id of ['Percent','Count','Amount'])$(id).addEventListener('input',reset);
$('Run').addEventListener('click',()=>read([null]));$('Export').addEventListener('click',exportRows);
$('Prev').addEventListener('click',()=>read(positions.slice(0,-1)));
$('Next').addEventListener('click',()=>{if(page?.next)read([...positions,page.next]);});
window.nativeAdvancedAnalysis={isBusy:()=>busy,onStateChange(callback){onStateChange=callback;},clear(){scope=null;reset();},setContext(rev,s){revision=rev;scope=s;reset();}};
})();
