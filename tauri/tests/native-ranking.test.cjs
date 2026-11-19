'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),os=require('node:os');
const {prepare}=require('../prepare.cjs');
const script=fs.readFileSync(path.join(__dirname,'../native-ranking.js'),'utf8');
class Element{
 constructor(tag){this.tag=tag;this.children=[];this.textContent='';this.value='';this.className='';this.style={};this.attributes={};}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=[...nodes];}
 setAttribute(k,v){this.attributes[k]=String(v);}
}
function mount(){
 const elements=Object.fromEntries(['rankingStatus','rankingBars','rankingMetric','rankingCategory'].map(id=>[id,new Element(id)]));
 elements.rankingMetric.value='amount';elements.rankingCategory.value='all';
 const window={},document={getElementById:id=>elements[id],createElement:tag=>new Element(tag)};
 vm.runInNewContext(script,{window,document});
 return {window,e:elements};
}
const item=(name,kind,count,amount)=>({name,key:kind+':'+name,kind,count,amount,active:true});
// Distribution coverage moved to unified-entry.test.cjs (sole canonical entry).
test('ranking preserves signed and zero amounts and renders labels as text',()=>{
 const {window,e}=mount();
 window.nativeRanking.show({complete:true,rows:[
   item('<img src=x>', 'group',3,100),
   item('Отрицательная работа','work',-2,-50),
   item('Ноль','work',0,0)
 ]});
 assert.equal(e.rankingBars.children.length,3);
 const a=e.rankingBars.children[0],b=e.rankingBars.children[1],c=e.rankingBars.children[2];
 assert.match(a.children[0].children[0].textContent,/<img src=x>/);
 assert.equal(a.children[1].children[0].style.width,'100%');
 assert.equal(b.children[1].children[0].style.width,'50%');
 assert.equal(c.children[1].children[0].style.width,'0%');
 assert.match(b.children[1].children[0].className,/negative/);
 assert.equal(a.children[1].attributes['aria-hidden'],'true');
 assert.match(e.rankingStatus.textContent,/по модулю суммы/);
});
test('incomplete and invalid ranking never leaves stale bars',()=>{
 const {window,e}=mount();
 window.nativeRanking.show({complete:true,rows:[item('Начало','work',3,10)]});
 assert.equal(e.rankingBars.children.length,1);
 window.nativeRanking.show({complete:false,rows:[]});
 assert.equal(e.rankingBars.children.length,0);
 assert.match(e.rankingStatus.textContent,/есть пропуски/);
 window.nativeRanking.show({complete:true,rows:Array.from({length:13},(_,i)=>item('x'+i,'work',i,i))});
 assert.equal(e.rankingBars.children.length,0);
 assert.match(e.rankingStatus.textContent,/Некорректная выборка/);
 e.rankingCategory.value='group';
 window.nativeRanking.show({complete:true,rows:[item('Wrong','work',2,4)]});
 assert.equal(e.rankingBars.children.length,0);
 window.nativeRanking.clear('Фильтр изменился');
 assert.equal(e.rankingStatus.textContent,'Фильтр изменился');
});
