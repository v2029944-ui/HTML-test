/* Reference summary and changes presentation over bounded native SQL queries. */
(function(){
'use strict';
const api=window.desktopAPI,$=selector=>document.querySelector(selector),all=selector=>[...document.querySelectorAll(selector)];
const {titles,observed,incomplete}=window.analysisSummary,categories={up:'Рост',down:'Снижение',new:'Новые работы',gone:'Без выполнений'};
let initialized=false,scope=null,screen='s1',generation=0,changesTicket=0,modalTicket=0,searchTimer=null,summaryData=null,changeData=null,changeRequest=null;
let modalHeading=null,shownSummaryRevision=null,shownChangesRevision=null,scopeBlocked=false;
let kind='significant',page=0,percent=30,minCount=5,minAmount=1000;
const el=(tag,text,cls)=>{const n=document.createElement(tag);if(text!==undefined&&text!==null)n.textContent=String(text);if(cls)n.className=cls;return n;};
const button=(text,run,cls='btn')=>{const n=el('button',text,cls);n.type='button';n.addEventListener('click',run);return n;};
const fmt=(n,metric='amount')=>Number.isFinite(n)?new Intl.NumberFormat('ru-RU',{minimumFractionDigits:metric==='count'?3:2,maximumFractionDigits:metric==='count'?3:2}).format(n===0?0:n):'—';
const displayedChange=(delta,metric)=>window.analysisSummary.displayedChange(delta,metric,fmt);
function pct(n,summary=false){
 if(!Number.isFinite(n))return '—';
 if(Math.abs(n)<0.05)return summary?'< 0,1%':Math.abs(n)<1e-9?'0,0%':'≈ 0,0%';
 return (n>0?'+':'−')+new Intl.NumberFormat('ru-RU',{maximumFractionDigits:1,minimumFractionDigits:1}).format(Math.abs(n))+'%';
}
const date=d=>/^\d{4}-\d{2}-\d{2}$/.test(d||'')?d.slice(8)+'.'+d.slice(5,7)+'.'+d.slice(0,4):'—';
const rangeFormat=new Intl.DateTimeFormat('ru-RU',{day:'numeric',month:'short',year:'numeric',timeZone:'UTC'});
function range(a,b){
 const first=new Date(a+'T12:00:00Z'),last=new Date(b+'T12:00:00Z');
 if(!Number.isFinite(first.getTime())||!Number.isFinite(last.getTime())||first>last)return '—';
 if(a===b)return rangeFormat.format(first);
 return rangeFormat.formatRange?rangeFormat.formatRange(first,last):rangeFormat.format(first)+' — '+rangeFormat.format(last);
}
const visibleRange=s=>range(s.displayFrom||s.from,s.displayTo||s.to);
const baseRange=s=>range(s.displayBaseFrom||s.baseFrom,s.displayBaseTo||s.baseTo);
const textError=e=>window.analizUserError?window.analizUserError(e):(e?.message||String(e));
const usable=s=>s&&Number.isSafeInteger(s.revision)&&Array.isArray(s.organizations)&&s.organizations.length>0;
function resultDialog(title,anchor){const body=window.referenceDesktop.dialog(title,anchor);modalHeading=body.firstElementChild;return body;}
function notify(message){if(window.unifiedUI?.notice)window.unifiedUI.notice(message,true);else if(window.referenceDesktop?.notice)window.referenceDesktop.notice(message,true);else $('#s2 .table-empty')?.replaceChildren(el('p',message));}
function setBusy(id,busy){
 const root=$(id);root?.setAttribute('aria-busy',String(busy));
 const selectors=id==='#s1'?'.metrics-grid,.quality-notice,.data-card,.insight-card':'.kpi,.leaders,.table-viewport,.pager-footer,.match-note,.quality-notice,[data-export-changes]';
 for(const node of root?.querySelectorAll(selectors)||[])node.inert=busy;
 for(const node of root?.querySelectorAll('button')||[])if(node.textContent.includes('Печать')){node.inert=busy;node.disabled=busy||!(id==='#s1'?summaryData:changeData);}
}
function captions(s,roots=['#s1','#s2']){for(const root of roots){const n=$(root+' .report-range');if(!n)continue;n.replaceChildren(el('span',s?visibleRange(s):'Выберите периоды и подразделения'));if(s){n.append(el('span','Сравнение с '+baseRange(s)),el('span','Все работы'));const a=el('a','Изменить период');a.href='#reportShell';a.addEventListener('click',event=>{event.preventDefault();window.referenceFilters.openFilters();});n.append(a);}}}
const hasReports=period=>period?.complete||period?.available_reports>0;
const canCompare=summary=>hasReports(summary?.before)&&hasReports(summary?.after);
const partialComparison=summary=>canCompare(summary)&&!(summary.before.complete&&summary.after.complete);
function summaryCards(values,comparison,comparable,totalOrganizations){
 const cards=all('#s1 .metric-card');for(const [i,metric]of ['amount','count','average'].entries()){
  const card=cards[i];if(!card)continue;card.querySelector('.metric-name').textContent=titles[metric];card.querySelector('.metric-value').textContent=fmt(values?.[metric],metric);
  const delta=card.querySelector('.metric-delta');delta.replaceChildren();const m=comparison?.summary?.[metric];
  if(Number.isFinite(m?.delta)){
   const change=displayedChange(m.delta,metric);
   const percentText=change.direction!=='neutral'&&Number.isFinite(m.percent)?' ('+pct(m.percent,true)+')':'';
   delta.append(el('span',change.label+percentText,change.direction));
  }
  else delta.append(el('span',comparison?'Сравнение недоступно':'Загрузка…','mut'));
  if(comparison&&totalOrganizations>0){
   const label=partialComparison(comparison.summary)?'По доступным отчётам':comparable?'по '+comparable+' из '+totalOrganizations:'Почему недоступно?';
   delta.append(button(label,e=>openSample(e.target),'metric-sample sample-link'));
  }
 }
}
function historicalQuality(period){
 if(!period)return '';
 const parts=[];
 if(period.invalid_reports>0)parts.push('ошибочных отчётов по всем подразделениям — '+period.invalid_reports);
 if(period.import_warning_reports>0)parts.push('отчётов с замечаниями в выбранных подразделениях — '+period.import_warning_reports);
 if(period.duplicate_issues>0)parts.push('признаков дублирования в выбранных подразделениях — '+period.duplicate_issues);
 if(period.overlap_issues>0)parts.push('признаков пересечения в выбранных подразделениях — '+period.overlap_issues);
 return parts.length?'За всю историю отчётов этого типа: '+parts.join(', ')+'.':'';
}
function comparisonStatus(comparable,total,summary){
 const noBaseline=summary?.before?.available_reports===0,noCurrent=summary?.after?.available_reports===0;
 if(noBaseline&&noCurrent)return 'За оба интервала нет пригодных данных.';
 if(noBaseline)return 'За базовый интервал нет пригодных данных.';
 if(noCurrent)return 'За анализируемый интервал нет пригодных данных.';
 if(partialComparison(summary))return 'Сравнение по доступным отчётам. Полные отчёты у '+comparable+' из '+total+' подразделений.';
 if(comparable)return 'Полные отчёты у '+comparable+' из '+total+' подразделений.';
 return 'Ни у одного выбранного подразделения нет пригодных отчётов за весь базовый и анализируемый интервалы.';
}
function notice(root,s,comparable,summary){
 const current=summary?.after,baseline=summary?.before;
 const n=$(root+' .quality-notice');if(!n)return;const span=n.querySelector('span');const excluded=s.organizations.filter(org=>!comparable.includes(org));
 const warnings=historicalQuality(current);
 const missingAmount=[current,baseline].some(period=>period?.available_reports>0&&(period.amount_missing_rows===true || (period.amount_missing_rows==null&&period?.available_total?.amount==null)));
 const amountNotice=missingAmount?' В некоторых строках сумма не указана. Количество учтено, показана сумма заполненных ячеек.':'';
 const shareNotice=root==='#s1'&&current?.amount_missing_rows&&observed(current)?.amount>0?' Доли рассчитаны по заполненным суммам.':'';
 const gap=current?.missing===1&&baseline?.missing===0?current.single_missing:baseline?.missing===1&&current?.missing===0?baseline.single_missing:null;
 const detail=excluded.length===1&&gap?.org===excluded[0]&&date(gap.start)!=='—'?' · '+date(gap.start)+': нет пригодного отчёта':'';
 if(excluded.length||current?.complete===false||baseline?.complete===false){
  const missing=(current?.missing??0)+(baseline?.missing??0);
  const coverage=current?.available_reports===0||baseline?.available_reports===0
   ?comparisonStatus(0,s.organizations.length,summary)
   :root==='#s2'?'В таблице показаны изменения по доступным отчётам. Полные отчёты у '+comparable.length+' из '+s.organizations.length+' подразделений.':comparisonStatus(comparable.length,s.organizations.length,summary);
  n.hidden=false;
  span.textContent=(root==='#s1'&&current?.available_reports>0&&current.complete===false?'Итоги частичные. ':'')+coverage+(missing>0?' Пропусков: '+missing+' (подразделение × период).':'')+(detail?' '+excluded[0]+detail+'.':'')+(warnings?' '+warnings:'')+amountNotice+shareNotice+' Пропуски не считаются нулём. Наличие отчётов не подтверждает полноту источника.';
  return;
 }
 n.hidden=!excluded.length&&current?.complete===true&&!warnings&&!missingAmount;
 span.textContent=comparisonStatus(comparable.length,s.organizations.length,summary)+(warnings?' '+warnings:'')+amountNotice+shareNotice+' Пропуски не считаются нулём. Наличие отчётов не подтверждает полноту источника.';
}
function appendTableRow(body,values){
 const tr=el('tr');
 values.forEach((value,i)=>{const td=el('td',typeof value==='object'&&value!==null?null:value,i?'num':'');if(value&&typeof value==='object')td.append(value);tr.append(td);});
 body.append(tr);
}
function writeTable(table,headers,rows){if(!table)return;let head=table.querySelector('thead'),body=table.querySelector('tbody');if(!head){head=el('thead');table.append(head);}if(!body){body=el('tbody');table.append(body);}const tr=el('tr');for(const h of headers){const th=el('th',h);th.setAttribute('scope','col');tr.append(th);}head.replaceChildren(tr);body.replaceChildren();for(const row of rows)appendTableRow(body,row);return body;}
function summaryLeaders(result,metric){const root=$('#s1 .insight-card');if(!root)return;const rows=result?.['top_'+metric]||[],valid=rows.filter(r=>Number.isFinite(r[metric]?.delta)&&r[metric].delta!==0).sort((a,b)=>Math.abs(b[metric].delta)-Math.abs(a[metric].delta)).slice(0,2);const body=root.querySelector('div');if(!body)return;body.replaceChildren(el('p',titles[metric]+(partialComparison(result?.summary)?' · по доступным отчётам':''),'mut'));for(const row of valid){const line=el('div',null,'insight-row'),change=displayedChange(row[metric].delta,metric);line.append(el('span',window.analysisSummary.itemLabel(row)),el('strong',change.label,change.direction));body.append(line);}if(!valid.length)body.append(el('p','Нет сопоставимых ненулевых изменений.','mut'));}
async function refreshSummary(s,ticket){
 const current=()=>ticket===generation&&scope===s;
 try{
  const [result,organizations]=await window.analysisSummary.read(api,s);
  if(!current())return;
  if(!result?.summary?.after||!Array.isArray(result.divisions)||!Array.isArray(organizations))throw Error('Некорректный результат SQL-сводки.');
  const comparable=result.divisions.filter(r=>s.organizations.includes(r.organization)&&Number.isFinite(r.before?.count)&&Number.isFinite(r.after?.count)).map(r=>r.organization);
  const rows=[];
  for(let offset=0;offset<organizations.length;offset+=4){const part=await Promise.all(organizations.slice(offset,offset+4).map(async org=>{
   if(org.available_total||org.complete)return {...org,observed:observed(org)};const partial=await api.nativeAnalysisRange(s.revision,s.type,s.from,s.to,[org.org]);return {...org,observed:partial.available_reports>0?partial.available_total:null};
  }));if(!current())return;rows.push(...part);}
  rows.sort((a,b)=>{const av=a.observed?.amount,bv=b.observed?.amount;if(Number.isFinite(av)!==Number.isFinite(bv))return Number.isFinite(av)?-1:1;return (Number.isFinite(av)&&av!==bv?bv-av:0)||a.org.localeCompare(b.org,'ru');});
  const after=result.summary.after,available=observed(after);
  captions(s,['#s1']);shownSummaryRevision=s.revision;summaryData={scope:s,result,organizations:rows,observed:available,comparable};$('#s1 .data-card button').disabled=false;
  summaryCards(available,result,comparable.length,s.organizations.length);notice('#s1',s,comparable,result.summary);if(screen==='s2'&&changeData)notice('#s2',s,changeData.comparable,result.summary);summaryLeaders(result,s.metric);
  const totalAmount=available?.amount;
  writeTable($('#s1 table'),['Подразделение',titles.amount,titles.count,'Доля суммы',titles.average],rows.map(org=>{
   const value=org.observed;
   const coverage=incomplete(org)?(Number.isFinite(value?.count)?' (частичные данные)':' (нет данных)'):'';
   const label=el('span',org.org+coverage);if(!comparable.includes(org.org)){const warning=button('',e=>openSample(e.target.closest?e.target.closest('button'):e.target),'org-warning');warning.setAttribute('aria-label',org.org+' · неполные отчёты. Показать состав сравнения');warning.title='В базовом или анализируемом интервале есть пропуски отчётов';const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');icon.setAttribute('viewBox','0 0 24 24');icon.setAttribute('width','17');icon.setAttribute('height','17');icon.setAttribute('fill','none');icon.setAttribute('stroke-width','1.7');icon.setAttribute('aria-hidden','true');const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d','M12 3 2 21h20L12 3zM12 10v5M12 18v.5');icon.append(path);warning.append(icon);label.append(warning);}
   const share=Number.isFinite(value?.amount)&&Number.isFinite(totalAmount)&&totalAmount>0?100*value.amount/totalAmount:null,shareCell=el('span',Number.isFinite(share)?new Intl.NumberFormat('ru-RU',{maximumFractionDigits:1,minimumFractionDigits:1}).format(share)+'%':'—');if(Number.isFinite(share)&&share>=0){const bar=el('div',null,'bar'),fill=el('i');fill.style.width=Math.min(100,share)+'%';bar.append(fill);shareCell.append(bar);}return [label,fmt(value?.amount,'amount'),fmt(value?.count,'count'),shareCell,fmt(value?.average,'average')];
  }));
 }catch(error){if(current()){captions(s,['#s1']);shownSummaryRevision=null;summaryData=null;writeTable($('#s1 table'),['Подразделение',titles.amount,titles.count,'Доля суммы',titles.average],[]);summaryLeaders(null,s.metric);summaryCards(null,{summary:{}},0,0);$('#s1 .quality-notice').hidden=false;$('#s1 .quality-notice span').textContent='Не удалось рассчитать сводку: '+textError(error);}}
 finally{if(current())setBusy('#s1',false);}
}
function summaryRequest(s){
 return {revision:s.revision,reportType:s.type,from:s.from,to:s.to,baseFrom:s.baseFrom,baseTo:s.baseTo,organizations:s.organizations.slice(),metric:s.metric,percent:0,minCount:0,minAmount:0,kind:'all',search:'',sort:'name',page:0};
}
function request(s){return {...summaryRequest(s),percent,minCount,minAmount,kind,search:$('#s2 input[placeholder]')?.value.trim()||'',sort:$('#s2 [data-sort]')?.value||'delta',page};}
function changesLoading(message){writeTable($('#s2 .changes-table'),['Работа','Подразделение','Было','Стало','Изменение','Изменение, %'],[]);$('#s2 .table-empty').hidden=false;$('#s2 .table-empty').replaceChildren(el('p',message));$('#s2 .pager-footer').replaceChildren();$('#s2 [data-export-changes]').disabled=true;}
function renderCounts(counts){
 for(const [i,key] of ['up','down','new','gone'].entries()){
  const card=all('#s2 .kpi')[i];if(!card)continue;
  const count=counts?.[key],known=Number.isSafeInteger(count)&&count>=0;
  card.querySelector('[data-count]').textContent=known?(key==='up'?'▲ ':key==='down'?'▼ ':'')+count:'—';
  if(key==='up'||key==='down'){const forms={one:'позиция',few:'позиции',many:'позиций',other:'позиции'};card.children[2].textContent=known?forms[new Intl.PluralRules('ru').select(count)]+' по порогу':'Количество по порогу';}
  card.setAttribute('aria-pressed',String(kind===key));
  card.setAttribute('aria-label',categories[key]+', '+(known?count:'нет данных')+'. Фильтр таблицы');
 }
}
function clearChanges(s,message){
 shownChangesRevision=null;changeData=null;changeRequest=null;
 renderCounts(null);renderChangeLeader(null,s?.metric||'amount','Данные недоступны');changeLabels(s);
 const excluded=$('#s2 .excluded-orgs');if(excluded){excluded.textContent='';excluded.hidden=true;}
 $('#s2 .match-note').hidden=true;$('#s2 .quality-notice').hidden=true;
 window.sqlRowScroll?.reset($('#s2 .table-viewport'));changesLoading(message);
}
function renderChangeLeader(leader,metric,empty='Нет сопоставимых данных'){$('#s2 .leaders').style.gridTemplateColumns='1fr';all('#s2 .leaders .card').forEach((card,i)=>{card.hidden=i!==(metric==='count'?0:1);if(card.hidden)return;card.querySelector('.leader-name').textContent='Наибольшее абсолютное изменение ('+titles[metric]+')';const change=displayedChange(leader?.delta,metric);card.querySelector('.leader-value').textContent=change.label;card.querySelector('.leader-value').className='leader-value num '+change.direction;card.children[2].textContent=leader?leader.name+' · '+leader.org:empty;});}
function pageControls(total,captured,count){
 const root=$('#s2 .table-viewport'),footer=$('#s2 .pager-footer');
 root.dataset.sqlScrollHost='reference-changes';
 window.sqlRowScroll.attach(root,{index:captured.page,size:12,total,count,footer,label:'Страницы анализа изменений',go:i=>{if(changeRequest!==captured)return false;page=i;return refreshChanges();}});
}
async function refreshChanges(debounceSearch=false){
 if(scopeBlocked)return;
 printAbort?.abort();clearTimeout(searchTimer);
 const active=document.activeElement;
 const focusAnchor=active?.closest?.('#s2 .kpi, #s2 .pager-footer button');
 const s=scope,ticket=++changesTicket,mine=generation;changeData=null;changeRequest=null;setBusy('#s2',true);
 if(!usable(s)||shownChangesRevision!==s.revision){
  clearChanges(s,'Загрузка изменений…');
 }
 const current=()=>scope===s&&mine===generation&&ticket===changesTicket;
 if(!usable(s)){clearChanges(null,'Выберите периоды и подразделения.');setBusy('#s2',false);return;}
 const captured=request(s);
 if(debounceSearch){
  searchTimer=setTimeout(()=>loadChanges(s,captured,current,focusAnchor),150);
  return;
 }
 return loadChanges(s,captured,current,focusAnchor);
}
async function loadChanges(s,captured,current,focusAnchor){
 try{
  if(new TextEncoder().encode(captured.search).length>512)throw Error('Сократите поисковый запрос до 512 байт UTF-8.');
  const result=await api.referenceChanges(captured);if(!current())return;
  if(!Array.isArray(result?.rows)||result.rows.length>12||!Number.isSafeInteger(result.total)||result.total<0||!Number.isSafeInteger(result.searched)||!Number.isSafeInteger(result.significant)||result.significant<0||result.searched<result.significant||result.searched<result.total||!Array.isArray(result.comparable)||!Object.keys(categories).every(key=>Number.isSafeInteger(result.counts?.[key])&&result.counts[key]>=0))throw Error('Некорректная страница SQL-изменений.');
  captions(s,['#s2']);shownChangesRevision=s.revision;changeData=result;changeRequest=captured;renderCounts(result.counts);renderChangeLeader(result.leader,s.metric);notice('#s2',s,result.comparable,summaryData?.result.summary);
  changeLabels(s);
  const changes=result.rows.map(row=>displayedChange(row.delta,s.metric));
  const rows=result.rows.map((row,i)=>[row.name,row.org,fmt(row.before,s.metric),fmt(row.after,s.metric),changes[i].value,pct(row.percent)]);writeTable($('#s2 .changes-table'),['Работа','Подразделение','Было','Стало','Изменение','Изменение, %'],rows);for(const [i,tr]of all('#s2 .changes-table tbody tr').entries()){const row=result.rows[i],direction=changes[i].direction;tr.children[4].className='num '+direction;tr.children[5].className='num '+direction;const mobile=el('td',null,'mobile-position'),heading=el('div',null,'position-heading'),metric=el('div',null,'position-metric');mobile.setAttribute('colspan','6');heading.append(el('strong',row.name),el('span',row.org));metric.append(el('span',titles[s.metric]),el('strong',changes[i].value+' · '+pct(row.percent),direction),el('span','Было '+fmt(row.before,s.metric)+' → стало '+fmt(row.after,s.metric),'position-history'));mobile.append(heading,metric);tr.append(mobile);}
  const empty=$('#s2 .table-empty');empty.hidden=Boolean(rows.length);empty.replaceChildren();if(!rows.length){const filtered=captured.search||!['significant','all'].includes(kind),noPositions=!filtered&&!result.comparable.length&&!result.searched;empty.append(el('strong',noPositions?'Нет сопоставимых позиций':filtered?'Совпадений нет':kind==='significant'?'Значимых изменений нет':'Нет выполненных работ'),el('p',noPositions?'Проверьте отчёты за оба периода и выбранные подразделения. Пропуски не считаются нулём.':'Измените запрос, категорию или чувствительность.'));if(filtered)empty.append(button('Сбросить поиск и категорию',()=>{$('#s2 input[placeholder]').value='';kind='significant';page=0;return refreshChanges();}));}
  const note=$('#s2 .match-note');note.hidden=false;
  let caption=kind==='all'?'Все позиции · '+result.searched:kind==='significant'?'Значимые позиции · '+result.significant:'Фильтр · '+categories[kind];
  if(kind==='significant'&&result.partial_fallback){
   const available='Доступные значения · '+(result.total-result.significant);
   caption=result.significant?caption+' · '+available:available+' · Изменение недоступно';
  }
  note.replaceChildren(el('span',caption));if(kind!=='significant')note.append(button('Только значимые',()=>{kind='significant';page=0;return refreshChanges();}));if(kind!=='all'&&result.searched>result.total)note.append(button('Показать все '+result.searched,()=>{kind='all';page=0;return refreshChanges();}));
  pageControls(result.total,captured,result.rows.length);$('#s2 [data-export-changes]').disabled=result.total===0;
 }catch(error){if(current()){captions(s,['#s2']);clearChanges(s,'Не удалось рассчитать изменения: '+textError(error));}}
 finally{if(current()){
  setBusy('#s2',false);
  // Chromium drops focus while an ancestor is inert. Restore it only after unlocking.
  if(focusAnchor&&screen==='s2'&&document.activeElement===document.body){
   const target=focusAnchor.isConnected?focusAnchor:$('#s2 input[placeholder]');target?.focus({preventScroll:true});
  }
 }}

}
function openSample(anchor){const data=summaryData,s=scope;if(!data||data.scope!==s)return;const root=resultDialog('Состав сравнения',anchor);document.getElementById('sheet')?.addEventListener('close',()=>{if(scope===s&&anchor?.isConnected)anchor.focus({preventScroll:true});},{once:true});const summary=data.result.summary;root.append(el('p',comparisonStatus(data.comparable.length,s.organizations.length,summary)),el('p','База: '+baseRange(s)+' · Анализ: '+visibleRange(s)));const list=el('ul');for(const org of s.organizations){const d=data.result.divisions.find(row=>row.organization===org),before=Number.isFinite(d?.before?.count),after=Number.isFinite(d?.after?.count);list.append(el('li',org+': '+(before&&after?'полные отчёты за оба интервала':!before&&!after?'неполные базовый и анализируемый интервалы':!before?'неполный базовый интервал':'неполный анализируемый интервал')));}root.append(list);
 if(hasReports(summary.before)||hasReports(summary.after)){
  const table=el('table');table.className='sample-values';table.setAttribute('aria-label','Итоги по доступным отчётам');
  writeTable(table,['Показатель','Базовый период','Анализируемый период'],['amount','count'].map(metric=>[titles[metric],fmt(observed(summary.before)?.[metric],metric),fmt(observed(summary.after)?.[metric],metric)]));
  root.append(table);
 }
 root.append(el('p','Наличие отчётов не подтверждает полноту источника. Замечания импорта не означают автоматического исключения.','mut'));if(data.comparable.length&&data.comparable.length<s.organizations.length)root.append(button('Сравнить только подразделения с полными отчётами',()=>{document.getElementById('sheet')?.close();window.referenceFilters.setOrganizations(data.comparable.slice());}));root.append(button('Проверить отчёты',()=>openQuality(anchor)));}
function openQuality(anchor){const s=scope,mine=generation,modal=++modalTicket;if(!usable(s))return;const root=resultDialog('Качество отчётов',anchor),nav=el('div',null,'seg'),content=el('div');root.append(button('← К составу сравнения',()=>openSample(anchor)),nav,content);const current=()=>scope===s&&mine===generation&&modal===modalTicket&&document.getElementById('sheet')?.open!==false;for(const [label,from,to]of [['Отчётный период',s.from,s.to],['База сравнения',s.baseFrom,s.baseTo]])nav.append(button(label,()=>draw(from,to)));function draw(from,to){if(!current())return;content.replaceChildren(el('p','Наличие отчёта не подтверждает полноту источника. Замечание импорта само по себе не исключает отчёт.','mut'));if(window.unifiedCoverage?.render)void window.unifiedCoverage.render(content,{...s,from,to},current);else content.append(el('p','Матрица качества данных недоступна.'));}draw(s.from,s.to);}
function sensitivity(anchor){
 const root=resultDialog('Чувствительность анализа',anchor),slider=el('input'),output=el('output',percent+'%');
 const label=el('label','Порог изменения ');slider.type='range';slider.min='0';slider.max='200';slider.step='1';slider.value=String(percent);
 label.append(slider,output);root.append(label);slider.addEventListener('input',()=>{output.textContent=slider.value+'%';});
 const fields=[];
 for(const [text,n]of [['Минимум количества / объёма',minCount],['Минимум суммы',minAmount]]){
  const field=el('input'),caption=el('label',text+' ');field.type='number';field.min='0';field.step='any';field.required=true;field.value=String(n);
  caption.append(field);root.append(caption);fields.push(field);
 }
 root.append(el('p','Сильное изменение должно достигнуть процентного и абсолютного порогов. Рост числа не означает улучшение эффективности.','mut'));
 window.referenceDesktop.setDialogApply(button('Применить',()=>{
  const invalid=fields.find(field=>!field.value.trim()||!Number.isFinite(Number(field.value))||Number(field.value)<0);
  if(invalid){invalid.focus?.();invalid.reportValidity?.();return;}
  percent=Number(slider.value);[minCount,minAmount]=fields.map(field=>Number(field.value));
  document.getElementById('sheet')?.close();sensitivityCaption();page=0;return refreshChanges();
 }));
}
function sensitivityCaption(){const n=$('#s2 .filter-threshold');if(n)n.textContent='Чувствительность: '+percent+'% ▾';
 const minima=$('#s2 .filter-sensitivity span');
 if(minima)minima.textContent='Минимальное изменение: количество / объём — '+String(minCount).replace('.',',')+', сумма — '+String(minAmount).replace('.',',')+'.';}
async function exportChanges(anchor){const captured=changeRequest,s=scope,mine=generation;if(!captured||!changeData?.total)return;anchor.disabled=true;try{const destination=await api.chooseCsvPath('izmeneniya-'+s.from+'-'+s.to+'.csv');if(!destination)return;if(scope!==s||mine!==generation||captured!==changeRequest)throw Error('Параметры изменились. Рассчитайте изменения заново.');await api.referenceChangesCsv(captured,destination);}catch(error){notify(textError(error));}finally{anchor.disabled=!changeData?.total;}}
async function exportSummary(){const data=summaryData,s=scope,mine=generation,anchor=$('#s1 .data-card button');if(!data||data.scope!==s)return;anchor.disabled=true;try{const destination=await api.chooseCsvPath('svodka-'+s.from+'-'+s.to+'.csv');if(!destination)return;if(scope!==s||mine!==generation||summaryData!==data)throw Error('Параметры изменились. Рассчитайте сводку заново.');await api.referenceSummaryCsv(summaryRequest(s),destination);}catch(error){notify(textError(error));}finally{anchor.disabled=!summaryData;}}
function changeTitle(value=kind){return value==='significant'?'Значимые изменения':value==='all'?'Все изменения':categories[value]||'Изменения';}
function changeLabels(s){
 const heading=$('#s2 .changes-heading h2'),metric=$('#s2 [data-current-change-metric]');
 if(heading)heading.textContent=changeTitle();if(metric)metric.textContent=titles[s?.metric]||'Выберите показатель в общих фильтрах';
}
function printTable(root,headers,rows,cls){const table=el('table',null,cls);writeTable(table,headers,rows);root.append(table);}
function printContext(root,s,comparable,overview){
 const summary=overview?.summary;
 root.append(el('p','Анализируемый период: '+visibleRange(s)),el('p','Базовый период: '+baseRange(s)),el('p','Область работ: Все работы'),el('p','Выбраны подразделения: '+s.organizations.join(', ')),el('p','Подразделения с полными отчётами: '+comparable.length+' из '+s.organizations.length+(comparable.length?' · '+comparable.join(', '):'')));
 if(comparable.length<s.organizations.length)root.append(el('p',comparisonStatus(comparable.length,s.organizations.length,summary)));
 const excluded=s.organizations.filter(org=>!comparable.includes(org));
 if(excluded.length){
  for(const org of excluded){const row=overview?.divisions?.find(value=>value.organization===org),before=Number.isFinite(row?.before?.count),after=Number.isFinite(row?.after?.count);root.append(el('p',org+': '+(row?!before&&!after?'неполные базовый и анализируемый интервалы':!before?'неполный базовый интервал':'неполный анализируемый интервал':'нет пригодных отчётов за весь базовый или анализируемый интервал')+'.'));}
 }
 for(const [label,period]of [['База',overview?.summary?.before],['Анализ',overview?.summary?.after]]){
  if(period)root.append(el('p',label+': пропусков отчётов — '+(Number.isSafeInteger(period.missing)?period.missing:'—')+'.'));
 }
 const history=historicalQuality(overview?.summary?.after);
 if(history)root.append(el('p',history));
 if(hasReports(summary?.before)||hasReports(summary?.after)){
  root.append(el('h2',canCompare(summary)?'Сравнение по доступным отчётам':'Доступные итоги по периодам'));
  printTable(root,['Показатель','Было','Стало','Изменение','Изменение, %'],['amount','count','average'].map(metric=>[titles[metric],fmt(observed(summary.before)?.[metric],metric),fmt(observed(summary.after)?.[metric],metric),displayedChange(summary[metric]?.delta,metric).value,pct(summary[metric]?.percent)]),'print-comparison');
 }
 root.append(el('p','Пропуски не считаются нулём. Замечания импорта сами по себе не исключают отчёт. Наличие отчётов не подтверждает полноту источника.'));
}
let printing=false,printAbort=null;
async function printScreen(event){
 const s=scope,mine=generation,tab=screen,data=tab==='s1'?summaryData:changeData,captured=changeRequest,ticket=changesTicket,anchor=event?.currentTarget||event?.target;
 if(printing)return;
 if(!usable(s)||!data||(tab==='s1'?data.scope!==s:!captured)||$('#'+tab)?.getAttribute('aria-busy')==='true'){notify('Дождитесь загрузки данных.');return;}
 const current=()=>scope===s&&generation===mine&&screen===tab&&(tab==='s1'?summaryData===data:changeData===data&&changeRequest===captured&&changesTicket===ticket);
 printing=true;if(tab==='s2')printAbort=new AbortController();if(anchor)anchor.disabled=true;let root;
 try{
  root=el('section');root.id='referencePrintDocument';
  if(tab==='s1'){
   root.append(el('h1','Сводка выполненных работ'));printContext(root,s,data.comparable,data.result);
   root.append(el('h2','Наблюдаемые итоги за анализируемый период'));
   const after=data.result.summary.after,totals=observed(after);
   printTable(root,['Показатель','Наблюдаемое значение'],['amount','count','average'].map(metric=>[titles[metric],fmt(after.available_reports===0?null:totals?.[metric],metric)]),'print-observed');
   printTable(root,['Подразделение',titles.amount,titles.count,'Доля суммы',titles.average],all('#s1 table tbody tr').map(row=>[...row.children].map(cell=>cell.textContent)),'print-organizations');
  }else{
   root.append(el('h1',changeTitle(captured.kind)),el('p','Показатель: '+titles[captured.metric]));
   const comparable=data.comparable;let overview=summaryData?.scope===s?summaryData.result:null;if(!overview){overview=await api.nativeAnalysisCompare(s.revision,s.type,s.baseFrom,s.baseTo,s.from,s.to,s.organizations);if(!current())return;}
   printContext(root,s,comparable,overview);
   if(comparable.length<s.organizations.length&&data.total)root.append(el('p','Изменения в таблице рассчитываются по доступным отчётам, если данные есть за оба интервала.'));
   root.append(el('p','Фильтр: '+changeTitle(captured.kind)+(captured.search?' · Поиск: '+captured.search:'')+' · Сортировка: '+({delta:'по величине изменения',percent:'по изменению, %',name:'по названию'}[captured.sort]||captured.sort)),el('p','Порог: '+captured.percent+'% · Количество / объём: '+captured.minCount+' · Сумма: '+captured.minAmount));
   const totalLine=el('p'),table=el('table',null,'print-changes');
   let rowCount=0;
   const body=writeTable(table,['Работа','Подразделение','Было','Стало','Изменение','Изменение, %'],[]);
   root.append(totalLine,table);
   const printedSample=await api.referenceChangesPrint(captured,batch=>{
    if(!current())return;
    if(!Array.isArray(batch)||batch.length===0||batch.length>128||rowCount+batch.length>data.total)throw Error('Не удалось подтвердить полноту строк печати. Повторите расчёт.');
    for(const row of batch){
     appendTableRow(body,[row.name,row.org,fmt(row.before,s.metric),fmt(row.after,s.metric),displayedChange(row.delta,s.metric).value,pct(row.percent)]);
    }
    rowCount+=batch.length;
   },printAbort.signal);
   if(!current())return;
   if(rowCount!==data.total||JSON.stringify(printedSample)!==JSON.stringify(comparable))throw Error('Не удалось подтвердить полноту строк печати. Повторите расчёт.');
   totalLine.textContent='Строк: '+rowCount+' · Все строки по текущим фильтрам.';
   if(!rowCount)root.append(el('p',data.comparable.length?'Нет изменений по текущим фильтрам.':'Нет сопоставимых позиций.'));

  }
  if(!current())return;
  root.append(el('p','Дата подготовки: '+new Date().toLocaleDateString('ru-RU')));document.body.append(root);document.body.dataset.referencePrint='document';window.print();
 }catch(error){if(current())notify('Не удалось подготовить печать: '+textError(error));}
 finally{root?.remove();delete document.body.dataset.referencePrint;printing=false;printAbort=null;if(anchor){anchor.disabled=!(tab==='s1'?summaryData:changeData);if(current()&&anchor.isConnected)anchor.focus({preventScroll:true});}}
}
function init(){if(initialized)return;initialized=true;
 for(const root of ['#s1','#s2']){const quality=$(root+' .quality-notice button');if(quality){quality.textContent='Проверить отчёты';quality.addEventListener('click',e=>openQuality(e.target));}for(const n of all(root+' button')){if(n.textContent.includes('Печать'))n.addEventListener('click',printScreen);}}
 const summaryExport=$('#s1 .data-card button');summaryExport?.addEventListener('click',exportSummary);if(summaryExport)summaryExport.disabled=true;
 const open=$('#s1 .insight-card button');open?.addEventListener('click',()=>window.referenceDesktop.navigate('s2'));
 for(const [i,key]of ['up','down','new','gone'].entries())all('#s2 .kpi')[i]?.addEventListener('click',()=>{kind=kind===key?'significant':key;page=0;return refreshChanges();});
 $('#s2 input[placeholder]')?.addEventListener('input',()=>{page=0;refreshChanges(true);});$('#s2 [data-sort]')?.addEventListener('change',()=>{page=0;return refreshChanges();});
 $('#s2 .filter-threshold')?.addEventListener('click',e=>sensitivity(e.target));$('#s2 [data-export-changes]')?.addEventListener('click',e=>exportChanges(e.target));
 for(const n of all('#s2 button'))if(n.textContent==='Как читать результаты?')n.addEventListener('click',()=>{const root=resultDialog('Как читать результаты?',n);root.append(el('p','Сравниваются одинаковые подразделения с пригодными отчётами за оба выбранных интервала. Одна позиция — работа в одном подразделении. Пропуски не считаются нулём. Замечание импорта само по себе не исключает отчёт; проверьте его источник.'),el('p','Карточки категорий фильтруют таблицу. Поиск и сортировка действуют на таблицу; карточки и крупнейшее изменение относятся ко всей сопоставимой выборке. Процент при нулевой базе не рассчитывается.'));});
 changeLabels(null);sensitivityCaption();captions(null);summaryCards(null,{summary:{}},0,0);clearChanges(null,'Выберите периоды и подразделения.');
}
function invalidateScope(value){
 printAbort?.abort();
 if(modalHeading?.isConnected&&document.getElementById('sheet')?.open)document.getElementById('sheet').close();
 modalHeading=null;generation++;changesTicket++;modalTicket++;clearTimeout(searchTimer);
 scope=value?{...value,organizations:value.organizations.slice()}:null;summaryData=null;changeData=null;changeRequest=null;page=0;
 $('#s1 .data-card button').disabled=true;$('#s2 [data-export-changes]').disabled=true;
}
function clearSummary(s,loading){
 shownSummaryRevision=null;captions(s,['#s1']);summaryCards(null,loading?null:{summary:{}},0,0);
 $('#s1 .quality-notice').hidden=true;writeTable($('#s1 table'),['Подразделение',titles.amount,titles.count,'Доля суммы',titles.average],[]);summaryLeaders(null,s?.metric||'amount');
}
function beginScopeChange(revision){
 invalidateScope(null);scopeBlocked=true;
 if(shownSummaryRevision===null||shownSummaryRevision!==revision)clearSummary(null,true);
 if(shownChangesRevision===null||shownChangesRevision!==revision){captions(null,['#s2']);clearChanges(null,'Загрузка изменений…');}
 setBusy('#s1',true);setBusy('#s2',true);
}
function failScopeChange(message){
 invalidateScope(null);scopeBlocked=true;
 clearSummary(null,false);captions(null,['#s2']);clearChanges(null,message);
 $('#s1 .quality-notice').hidden=false;$('#s1 .quality-notice span').textContent=message;
 setBusy('#s1',false);setBusy('#s2',false);
}
function refresh(value){
 scopeBlocked=false;invalidateScope(value);
 const available=usable(scope);
 if(!available||shownSummaryRevision!==scope.revision)clearSummary(available?scope:null,available);
 if(!available||shownChangesRevision!==scope.revision){
  captions(available?scope:null,['#s2']);clearChanges(available?scope:null,available?'Загрузка изменений…':'Выберите периоды и подразделения.');
 }
 setBusy('#s1',available);setBusy('#s2',available&&screen==='s2');
 if(!available)return Promise.resolve();
 const tasks=[refreshSummary(scope,generation)];if(screen==='s2')tasks.push(refreshChanges());return Promise.all(tasks);
}
function setScreen(id){if(screen!==id)printAbort?.abort();screen=id==='s2'?'s2':'s1';$('#s1').hidden=screen!=='s1';$('#s2').hidden=screen!=='s2';return screen==='s2'?refreshChanges():Promise.resolve();}
function resetSelection(){kind='significant';page=0;percent=30;minCount=5;minAmount=1000;clearTimeout(searchTimer);$('#s2 input[placeholder]').value='';$('#s2 [data-sort]').value='delta';sensitivityCaption();}
window.referenceResults=Object.freeze({init,refresh,setScreen,resetSelection,beginScopeChange,failScopeChange});
})();
