// User-entered Excel coordinates; never executable expressions or formulas.
(function(){
'use strict';
function parseMappings(text){
 if(typeof text!=='string'||text.length>8192)throw Error('Сопоставления слишком длинные.');
 if(!text.trim())return null;
 const lines=text.trim().split(/\r?\n/);
 if(lines.length>64)throw Error('Не более 64 блоков за импорт.');
 const seen=new Set();
 return lines.map((line,i)=>{
  const fields=line.split(';').map(s=>s.trim().toUpperCase());
  const fail=()=>{throw Error('Строка '+(i+1)+': укажите номер заголовка и три разных столбца Excel через точку с запятой.');};
  if(fields.length!==4||!/^\d{1,6}$/.test(fields[0]))return fail();
  const header=Number(fields[0]);if(header<1||header>100000)return fail();
  const columns=fields.slice(1).map(s=>{
   if(!/^[A-Z]{1,3}$/.test(s))return fail();
   let n=0;for(const ch of s)n=n*26+ch.charCodeAt(0)-64;
   if(n>2048)return fail();return n;
  });
  if(new Set(columns).size!==3)return fail();
  const key=[header,...columns].join(':');if(seen.has(key))return fail();seen.add(key);
  return {header_row:header,work_col:columns[0],count_col:columns[1],amount_col:columns[2]};
 });
}
const api={parseMappings};
if(typeof module!=='undefined')module.exports=api;
if(typeof window!=='undefined')window.nativeImportMappings=Object.freeze(api);
})();
