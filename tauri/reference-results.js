/* Reference summary and changes presentation over bounded native SQL queries. */
(function(){
'use strict';
const api=window.desktopAPI,$=selector=>document.querySelector(selector),all=selector=>[...document.querySelectorAll(selector)];
const titles={amount:'Сумма',count:'Количество / объём',average:'Сумма / количество (объём)'},categories={up:'Рост',down:'Снижение',new:'Новые работы',gone:'Без выполнений'};
let initialized=false,scope=null,screen='s1',generation=0,changesTicket=0,modalTicket=0,searchTimer=null,summaryData=null,changeData=null,changeRequest=null;
let modalHeading=null;
let kind='significant',page=0,percent=30,minCount=5,minAmount=1000;
const el=(tag,text,cls)=>{const n=document.createElement(tag);if(text!==undefined&&text!==null)n.textContent=String(text);if(cls)n.className=cls;return n;};
const button=(text,run,cls='btn')=>{const n=el('button',text,cls);n.type='button';n.addEventListener('click',run);return n;};
const fmt=(n,metric='amount')=>Number.isFinite(n)?new Intl.NumberFormat('ru-RU',{minimumFractionDigits:metric==='count'?3:2,maximumFractionDigits:metric==='count'?3:2}).format(n):'—';
function displayedChange(delta,metric){
 if(!Number.isFinite(delta))return {value:'—',label:'—',direction:'neutral'};
 const value=fmt(Math.abs(delta),metric);
 if(Math.abs(delta)<1e-9)return {value,label:'Без изменений',direction:'neutral'};
 if(!/[1-9]/.test(value))return {value,label:'≈ без изменений',direction:'neutral'};
 const signed=(delta>0?'+':'−')+value;
 return {value:signed,label:(delta>0?'▲ ':'▼ ')+signed,direction:delta>0?'up':'down'};
}
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
function setBusy(id,busy){$(id)?.setAttribute('aria-busy',String(busy));}
function captions(s){for(const root of ['#s1','#s2']){const n=$(root+' .report-range');if(!n)continue;n.replaceChildren(el('span',s?visibleRange(s):'Выберите периоды и подразделения'));if(s){n.append(el('span','Сравнение с '+baseRange(s)));const a=el('a','Изменить выше');a.href='#reportShell';a.addEventListener('click',()=>{const fold=$('#reportShell .filter-fold');if(fold)fold.open=true;});n.append(a);}}}
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
  if(comparison&&totalOrganizations>0)delta.append(button('по '+comparable+' из '+totalOrganizations,e=>openSample(e.target),'metric-sample sample-link'));
 }
}
function notice(root,s,comparable,summary){
 const current=summary?.after,baseline=summary?.before;
 const n=$(root+' .quality-notice');if(!n)return;const span=n.querySelector('span');const excluded=s.organizations.filter(org=>!comparable.includes(org));
 const warnings=(current?.invalid_reports||0)+(current?.duplicate_issues||0)+(current?.overlap_issues||0)+(current?.import_warning_reports||0);
 const gap=current?.missing===1&&baseline?.missing===0?current.single_missing:baseline?.missing===1&&current?.missing===0?baseline.single_missing:null;
 const detail=excluded.length===1&&gap?.org===excluded[0]&&date(gap.start)!=='—'?' · '+date(gap.start)+': нет пригодного отчёта':'';
 n.hidden=!excluded.length&&current?.complete===true&&!warnings;
 span.textContent=(current?.available_reports===0?'Нет пригодных данных за анализируемый интервал. ':current?.complete===false?'Итоги частичные. ':'')+'Сравнение по '+comparable.length+' из '+s.organizations.length+' подразделений.'+(excluded.length?' Исключены из сравнения: '+excluded.join(', ')+detail+'.':'')+(warnings?' В исходных отчётах есть замечания: '+warnings+'.':'')+' Пропуски не считаются нулём. Наличие отчётов не подтверждает полноту источника.';
}
function writeTable(table,headers,rows){if(!table)return;let head=table.querySelector('thead'),body=table.querySelector('tbody');if(!head){head=el('thead');table.append(head);}if(!body){body=el('tbody');table.append(body);}const tr=el('tr');for(const h of headers){const th=el('th',h);th.setAttribute('scope','col');tr.append(th);}head.replaceChildren(tr);body.replaceChildren();for(const row of rows){const tr=el('tr');row.forEach((value,i)=>{const td=el('td',typeof value==='object'&&value!==null?null:value,i?'num':'');if(value&&typeof value==='object')td.append(value);tr.append(td);});body.append(tr);}}
function summaryLeaders(result,metric){const root=$('#s1 .insight-card');if(!root)return;const rows=result?.['top_'+metric]||[],valid=rows.filter(r=>Number.isFinite(r[metric]?.delta)&&r[metric].delta!==0).sort((a,b)=>Math.abs(b[metric].delta)-Math.abs(a[metric].delta)).slice(0,2);const body=root.querySelector('div');if(!body)return;body.replaceChildren(el('p',titles[metric]+' · сопоставимые подразделения','mut'));for(const row of valid){const line=el('div',null,'insight-row'),change=displayedChange(row[metric].delta,metric);line.append(el('span',row.name),el('strong',change.label,change.direction));body.append(line);}if(!valid.length)body.append(el('p','Нет сопоставимых ненулевых изменений.','mut'));}
async function refreshSummary(s,ticket){
 const current=()=>ticket===generation&&scope===s;
 try{
  const [result,organizations]=await Promise.all([api.nativeAnalysisCompare(s.revision,s.type,s.baseFrom,s.baseTo,s.from,s.to,s.organizations),api.nativeAnalysisOrganizations(s.revision,s.type,s.from,s.to,s.organizations)]);
  if(!current())return;
  if(!result?.summary?.after||!Array.isArray(result.divisions)||!Array.isArray(organizations))throw Error('Некорректный результат SQL-сводки.');
  const comparable=result.divisions.filter(r=>s.organizations.includes(r.organization)&&Number.isFinite(r.before?.amount)&&Number.isFinite(r.before?.count)&&Number.isFinite(r.after?.amount)&&Number.isFinite(r.after?.count)).map(r=>r.organization);
  let comparison=result;
  if(comparable.length&&comparable.length<s.organizations.length)comparison=await api.nativeAnalysisCompare(s.revision,s.type,s.baseFrom,s.baseTo,s.from,s.to,comparable);
  if(!current())return;
  const rows=[];
  for(let offset=0;offset<organizations.length;offset+=4){const part=await Promise.all(organizations.slice(offset,offset+4).map(async org=>{
   if(org.complete)return {...org,observed:org.total};const partial=await api.nativeAnalysisRange(s.revision,s.type,s.from,s.to,[org.org]);return {...org,observed:partial.available_reports>0?partial.available_total:null};
  }));if(!current())return;rows.push(...part);}
  rows.sort((a,b)=>{const av=a.observed?.amount,bv=b.observed?.amount;if(Number.isFinite(av)!==Number.isFinite(bv))return Number.isFinite(av)?-1:1;return (Number.isFinite(av)&&av!==bv?bv-av:0)||a.org.localeCompare(b.org,'ru');});
  const after=result.summary.after,observed=after.complete?after.total:after.available_reports>0?after.available_total:null;
  summaryData={scope:s,result,comparison:comparable.length?comparison:null,organizations:rows,observed,comparable};$('#s1 .data-card button').disabled=false;
  summaryCards(observed,comparable.length?comparison:{summary:{}},comparable.length,s.organizations.length);notice('#s1',s,comparable,result.summary);if(screen==='s2'&&changeData)notice('#s2',s,changeData.comparable,result.summary);summaryLeaders(comparable.length?comparison:null,s.metric);
  const totalAmount=observed?.amount;
  writeTable($('#s1 table'),['Подразделение',titles.amount,titles.count,'Доля суммы'],rows.map(org=>{
   const label=el('span',org.org+(org.complete?'':' (частичные данные)'));if(!comparable.includes(org.org)){const warning=button('',e=>openSample(e.target.closest?e.target.closest('button'):e.target),'org-warning');warning.setAttribute('aria-label',org.org+' · неполные отчёты. Показать состав сравнения');warning.title='Недостаточно пригодных отчётов за базовый или анализируемый интервал';const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');icon.setAttribute('viewBox','0 0 24 24');icon.setAttribute('width','17');icon.setAttribute('height','17');icon.setAttribute('fill','none');icon.setAttribute('stroke-width','1.7');icon.setAttribute('aria-hidden','true');const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d','M12 3 2 21h20L12 3zM12 10v5M12 18v.5');icon.append(path);warning.append(icon);label.append(warning);}const value=org.observed;
   const share=Number.isFinite(value?.amount)&&Number.isFinite(totalAmount)&&totalAmount>0?100*value.amount/totalAmount:null,shareCell=el('span',Number.isFinite(share)?new Intl.NumberFormat('ru-RU',{maximumFractionDigits:1,minimumFractionDigits:1}).format(share)+'%':'—');if(Number.isFinite(share)&&share>=0){const bar=el('div',null,'bar'),fill=el('i');fill.style.width=Math.min(100,share)+'%';bar.append(fill);shareCell.append(bar);}return [label,fmt(value?.amount,'amount'),fmt(value?.count,'count'),shareCell];
  }));
 }catch(error){if(current()){summaryData=null;summaryCards(null,{summary:{}},0,s.organizations.length);$('#s1 .quality-notice').hidden=false;$('#s1 .quality-notice span').textContent='Не удалось рассчитать сводку: '+textError(error);}}
 finally{if(current())setBusy('#s1',false);}
}
function request(s){return {revision:s.revision,reportType:s.type,from:s.from,to:s.to,baseFrom:s.baseFrom,baseTo:s.baseTo,organizations:s.organizations.slice(),metric:s.metric,percent,minCount,minAmount,kind,search:$('#s2 input[placeholder]')?.value.trim()||'',sort:$('#s2 [data-sort]')?.value||'delta',page};}
function changesLoading(message){writeTable($('#s2 .changes-table'),['Работа','Подразделение','Было','Стало','Изменение','Изменение, %'],[]);$('#s2 .table-empty').hidden=false;$('#s2 .table-empty').replaceChildren(el('p',message));$('#s2 .pager-footer').replaceChildren();$('#s2 [data-export-changes]').disabled=true;}
function renderCounts(counts){for(const [i,key]of ['up','down','new','gone'].entries()){const card=all('#s2 .kpi')[i];if(!card)continue;card.querySelector('[data-count]').textContent=(key==='up'?'▲ ':key==='down'?'▼ ':'')+counts[key];if(key==='up'||key==='down'){const forms={one:'позиция',few:'позиции',many:'позиций',other:'позиции'};card.children[2].textContent=forms[new Intl.PluralRules('ru').select(counts[key])]+' по порогу';}card.setAttribute('aria-pressed',String(kind===key));card.setAttribute('aria-label',categories[key]+', '+counts[key]+'. Фильтр таблицы');}}
function renderChangeLeader(leader,metric){$('#s2 .leaders').style.gridTemplateColumns='1fr';all('#s2 .leaders .card').forEach((card,i)=>{card.hidden=i!==(metric==='count'?0:1);if(card.hidden)return;card.querySelector('.leader-name').textContent='Наибольшее абсолютное изменение ('+titles[metric]+')';const change=displayedChange(leader?.delta,metric);card.querySelector('.leader-value').textContent=change.label;card.querySelector('.leader-value').className='leader-value num '+change.direction;card.children[2].textContent=leader?leader.name+' · '+leader.org:'Нет сопоставимых данных';});}
function pageControls(total,captured,count){
 const root=$('#s2 .table-viewport'),footer=$('#s2 .pager-footer');
 root.dataset.sqlScrollHost='reference-changes';
 window.sqlRowScroll.attach(root,{index:captured.page,size:12,total,count,footer,label:'Страницы анализа изменений',go:i=>{page=i;return refreshChanges(true);}});
}
async function refreshChanges(scrolling=false){
 const s=scope,ticket=++changesTicket,mine=generation;changeData=null;changeRequest=null;setBusy('#s2',true);if(scrolling!==true){window.sqlRowScroll?.reset($('#s2 .table-viewport'));changesLoading('Загрузка изменений…');}
 const current=()=>scope===s&&mine===generation&&ticket===changesTicket;
 if(!usable(s)){window.sqlRowScroll?.reset($('#s2 .table-viewport'));changesLoading('Выберите периоды и подразделения.');setBusy('#s2',false);return;}
 const captured=request(s);
 try{const result=await api.referenceChanges(captured);if(!current())return;
  if(!Array.isArray(result?.rows)||result.rows.length>12||!Number.isSafeInteger(result.total)||result.total<0||!Number.isSafeInteger(result.searched)||!Number.isSafeInteger(result.significant)||result.significant<0||result.searched<result.significant||result.searched<result.total||!Array.isArray(result.comparable)||!result.counts)throw Error('Некорректная страница SQL-изменений.');
  changeData=result;changeRequest=captured;renderCounts(result.counts);renderChangeLeader(result.leader,s.metric);notice('#s2',s,result.comparable,summaryData?.result.summary);
  const excluded=s.organizations.filter(x=>!result.comparable.includes(x)),chip=$('#s2 .excluded-orgs');if(chip){chip.hidden=!excluded.length;chip.textContent='без '+excluded.join(', ');}
  for(const control of all('#s2 [data-change-metric]'))control.setAttribute('aria-pressed',String(control.dataset.changeMetric===s.metric));
  const changes=result.rows.map(row=>displayedChange(row.delta,s.metric));
  const rows=result.rows.map((row,i)=>[row.name,row.org,fmt(row.before,s.metric),fmt(row.after,s.metric),changes[i].value,pct(row.percent)]);writeTable($('#s2 .changes-table'),['Работа','Подразделение','Было','Стало','Изменение','Изменение, %'],rows);for(const [i,tr]of all('#s2 .changes-table tbody tr').entries()){const row=result.rows[i],direction=changes[i].direction;tr.children[4].className='num '+direction;tr.children[5].className='num '+direction;const mobile=el('td',null,'mobile-position'),heading=el('div',null,'position-heading'),metric=el('div',null,'position-metric');mobile.setAttribute('colspan','6');heading.append(el('strong',row.name),el('span',row.org));metric.append(el('span',titles[s.metric]),el('strong',changes[i].value+' · '+pct(row.percent),direction),el('span','Было '+fmt(row.before,s.metric)+' → стало '+fmt(row.after,s.metric),'position-history'));mobile.append(heading,metric);tr.append(mobile);}
  const empty=$('#s2 .table-empty');empty.hidden=Boolean(rows.length);empty.replaceChildren();if(!rows.length){const filtered=captured.search||!['significant','all'].includes(kind);empty.append(el('strong',!result.comparable.length?'Нет сопоставимых позиций':filtered?'Совпадений нет':kind==='significant'?'Значимых изменений нет':'Нет выполненных работ'),el('p',!result.comparable.length?'Проверьте отчёты за оба периода и выбранные подразделения. Пропуски не считаются нулём.':'Измените запрос, категорию или чувствительность.'));if(filtered)empty.append(button('Сбросить поиск и категорию',()=>{$('#s2 input[placeholder]').value='';kind='significant';page=0;return refreshChanges();}));}
  const note=$('#s2 .match-note');note.hidden=false;note.replaceChildren(el('span',kind==='all'?'Все позиции · '+result.searched:kind==='significant'?'Значимые позиции · '+result.significant:'Фильтр · '+categories[kind]));if(kind!=='significant')note.append(button('Только значимые',()=>{kind='significant';page=0;return refreshChanges();}));if(kind!=='all'&&result.searched>result.significant)note.append(button('Показать все '+result.searched,()=>{kind='all';page=0;return refreshChanges();}));
  pageControls(result.total,captured,result.rows.length);$('#s2 [data-export-changes]').disabled=result.total===0;
 }catch(error){if(current()){window.sqlRowScroll?.reset($('#s2 .table-viewport'));changesLoading('Не удалось рассчитать изменения: '+textError(error));}}finally{if(current())setBusy('#s2',false);}
}
function openSample(anchor){const data=summaryData,s=scope;if(!data||data.scope!==s)return;const root=resultDialog('Состав сравнения',anchor);document.getElementById('sheet')?.addEventListener('close',()=>{if(scope===s&&anchor?.isConnected)anchor.focus({preventScroll:true});},{once:true});root.append(el('p','Сравнение по '+data.comparable.length+' из '+s.organizations.length+' подразделений. В обоих интервалах используется один и тот же состав.'),el('p','База: '+baseRange(s)+' · Анализ: '+visibleRange(s)));const list=el('ul');for(const org of s.organizations){const d=data.result.divisions.find(row=>row.organization===org),before=Number.isFinite(d?.before?.count)&&Number.isFinite(d?.before?.amount),after=Number.isFinite(d?.after?.count)&&Number.isFinite(d?.after?.amount);list.append(el('li',org+': '+(before&&after?'включено':!before&&!after?'нет пригодных отчётов за оба интервала':!before?'неполный базовый интервал':'неполный анализируемый интервал')));}root.append(list);
 const totals=data.comparison?.summary;
 if(data.comparable.length&&totals){
  const table=el('table');table.className='sample-values';table.setAttribute('aria-label','Показатели сопоставимых подразделений');
  writeTable(table,['Показатель','Базовый период','Анализируемый период'],['amount','count'].map(metric=>[titles[metric],fmt(totals.before?.total?.[metric],metric),fmt(totals.after?.total?.[metric],metric)]));
  root.append(table);
 }
 root.append(el('p','Наличие отчётов не подтверждает полноту источника. Замечания импорта не означают автоматического исключения.','mut'));if(data.comparable.length&&data.comparable.length<s.organizations.length)root.append(button('Сравнить только сопоставимые подразделения',()=>{document.getElementById('sheet')?.close();window.referenceFilters.setOrganizations(data.comparable.slice());}));root.append(button('Проверить отчёты',()=>openQuality(anchor)));}
function openQuality(anchor){const s=scope,mine=generation,modal=++modalTicket;if(!usable(s))return;const root=resultDialog('Качество отчётов',anchor),nav=el('div',null,'seg'),content=el('div');root.append(button('← К составу сравнения',()=>openSample(anchor)),nav,content);const current=()=>scope===s&&mine===generation&&modal===modalTicket&&document.getElementById('sheet')?.open!==false;for(const [label,from,to]of [['Отчётный период',s.from,s.to],['База сравнения',s.baseFrom,s.baseTo]])nav.append(button(label,()=>draw(from,to)));function draw(from,to){if(!current())return;content.replaceChildren(el('p','Наличие отчёта не подтверждает полноту источника. Замечание импорта само по себе не исключает отчёт.','mut'));if(window.unifiedCoverage?.render)void window.unifiedCoverage.render(content,{...s,from,to},current);else content.append(el('p','Матрица качества данных недоступна.'));}draw(s.from,s.to);}
function sensitivity(anchor){const root=resultDialog('Чувствительность анализа',anchor),range=el('input'),output=el('output',percent+'%'),label=el('label','Порог изменения ');range.type='range';range.min='0';range.max='200';range.step='1';range.value=String(percent);label.append(range,output);root.append(label);range.addEventListener('input',()=>{output.textContent=range.value+'%';});const fields=[];for(const [text,n]of [['Минимум количества / объёма',minCount],['Минимум суммы',minAmount]]){const field=el('input'),label=el('label',text+' ');field.type='number';field.min='0';field.step='any';field.value=String(n);label.append(field);root.append(label);fields.push(field);}root.append(el('p','Сильное изменение должно достигнуть процентного и абсолютного порогов. Рост числа не означает улучшение эффективности.','mut'),button('Применить',()=>{const values=[Number(range.value),...fields.map(n=>Number(n.value))];if(values.some(n=>!Number.isFinite(n)||n<0)||values[0]>200){notify('Укажите неотрицательные конечные пороги.');return;}[percent,minCount,minAmount]=values;document.getElementById('sheet')?.close();sensitivityCaption();page=0;return refreshChanges();}));}
function sensitivityCaption(){const n=$('#s2 .filter-threshold');if(n)n.textContent='Чувствительность: '+percent+'% ▾';
 const minima=$('#s2 .filter-sensitivity span');
 if(minima)minima.textContent='Минимальное изменение: количество / объём — '+String(minCount).replace('.',',')+', сумма — '+String(minAmount).replace('.',',')+'.';}
async function exportChanges(anchor){const captured=changeRequest,s=scope,mine=generation;if(!captured||!changeData?.total)return;anchor.disabled=true;try{const destination=await api.chooseCsvPath('izmeneniya-'+s.from+'-'+s.to+'.csv');if(!destination)return;if(scope!==s||mine!==generation||captured!==changeRequest)throw Error('Параметры изменились. Рассчитайте изменения заново.');await api.referenceChangesCsv(captured,destination);}catch(error){notify(textError(error));}finally{anchor.disabled=!changeData?.total;}}
async function exportSummary(){const data=summaryData,s=scope,mine=generation,anchor=$('#s1 .data-card button');if(!data||data.scope!==s)return;anchor.disabled=true;try{const destination=await api.chooseCsvPath('svodka-'+s.from+'-'+s.to+'.csv');if(!destination)return;if(scope!==s||mine!==generation||summaryData!==data)throw Error('Параметры изменились. Рассчитайте сводку заново.');await api.referenceSummaryCsv(request(s),destination);}catch(error){notify(textError(error));}finally{anchor.disabled=!summaryData;}}
function printScreen(){if(!usable(scope)||(screen==='s1'?!summaryData:!changeData)){notify('Дождитесь загрузки данных.');return;}document.body.dataset.referencePrint=screen;try{window.print();}finally{delete document.body.dataset.referencePrint;}}
function init(){if(initialized)return;initialized=true;
 for(const root of ['#s1','#s2']){const quality=$(root+' .quality-notice button');if(quality){quality.textContent='Проверить отчёты';quality.addEventListener('click',e=>openQuality(e.target));}for(const n of all(root+' button')){if(n.textContent.includes('Печать'))n.addEventListener('click',printScreen);}}
 const summaryExport=$('#s1 .data-card button');summaryExport?.addEventListener('click',exportSummary);if(summaryExport)summaryExport.disabled=true;
 const open=$('#s1 .insight-card button');open?.addEventListener('click',()=>window.referenceDesktop.navigate('s2'));
 for(const [i,key]of ['up','down','new','gone'].entries())all('#s2 .kpi')[i]?.addEventListener('click',()=>{kind=kind===key?'significant':key;page=0;return refreshChanges();});
 for(const n of all('#s2 [data-change-metric]')){n.textContent=titles[n.dataset.changeMetric];n.addEventListener('click',()=>window.referenceFilters.setMetric(n.dataset.changeMetric));}
 $('#s2 input[placeholder]')?.addEventListener('input',()=>{clearTimeout(searchTimer);searchTimer=setTimeout(()=>{page=0;void refreshChanges();},150);});$('#s2 [data-sort]')?.addEventListener('change',()=>{page=0;return refreshChanges();});
 $('#s2 .filter-threshold')?.addEventListener('click',e=>sensitivity(e.target));$('#s2 [data-export-changes]')?.addEventListener('click',e=>exportChanges(e.target));
 for(const n of all('#s2 button'))if(n.textContent==='Как читать результаты?')n.addEventListener('click',()=>{const root=resultDialog('Как читать результаты?',n);root.append(el('p','Сравниваются одинаковые подразделения с пригодными отчётами за оба выбранных интервала. Одна позиция — работа в одном подразделении. Пропуски не считаются нулём. Замечание импорта само по себе не исключает отчёт; проверьте его источник.'),el('p','Карточки категорий фильтруют таблицу. Поиск и сортировка действуют на таблицу; карточки и крупнейшее изменение относятся ко всей сопоставимой выборке. Процент при нулевой базе не рассчитывается.'));});
 sensitivityCaption();captions(null);summaryCards(null,{summary:{}},0,0);renderCounts({up:0,down:0,new:0,gone:0});changesLoading('Выберите периоды и подразделения.');
}
function refresh(value){window.sqlRowScroll?.reset($('#s2 .table-viewport'));if(modalHeading?.isConnected&&document.getElementById('sheet')?.open)document.getElementById('sheet').close();modalHeading=null;generation++;changesTicket++;modalTicket++;clearTimeout(searchTimer);scope=value?{...value,organizations:value.organizations.slice()}:null;summaryData=null;changeData=null;changeRequest=null;page=0;captions(usable(scope)?scope:null);summaryCards(null,usable(scope)?null:{summary:{}},0,scope?.organizations.length||0);$('#s1 .data-card button').disabled=true;for(const root of ['#s1','#s2']){const note=$(root+' .quality-notice');if(note){note.hidden=true;note.querySelector('span').textContent='';}}writeTable($('#s1 table'),['Подразделение',titles.amount,titles.count,'Доля суммы'],[]);summaryLeaders(null,scope?.metric||'amount');changesLoading(usable(scope)?'Загрузка изменений…':'Выберите периоды и подразделения.');renderCounts({up:0,down:0,new:0,gone:0});renderChangeLeader(null,scope?.metric||'amount');
 if(!usable(scope)){for(const root of ['#s1','#s2']){setBusy(root,false);const note=$(root+' .quality-notice');if(note){note.hidden=true;note.querySelector('span').textContent='';}}return Promise.resolve();}
 setBusy('#s1',true);const tasks=[refreshSummary(scope,generation)];if(screen==='s2')tasks.push(refreshChanges());return Promise.all(tasks);
}
function setScreen(id){screen=id==='s2'?'s2':'s1';$('#s1').hidden=screen!=='s1';$('#s2').hidden=screen!=='s2';return screen==='s2'?refreshChanges():Promise.resolve();}
function resetSelection(){kind='significant';page=0;percent=30;minCount=5;minAmount=1000;clearTimeout(searchTimer);$('#s2 input[placeholder]').value='';$('#s2 [data-sort]').value='delta';sensitivityCaption();}
window.referenceResults=Object.freeze({init,refresh,setScreen,resetSelection});
})();
