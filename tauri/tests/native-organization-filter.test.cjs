'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');

function mount(value='*'){
 class Element{
  constructor(){this.value='';this.children=[];this.events={};}
  append(...children){this.children.push(...children);}
  replaceChildren(...children){this.children=children;}
  addEventListener(event,callback){this.events[event]=callback;}
 }
 const nodes={analysisOrganization:new Element(),analysisOrganizationChoices:new Element()};
 nodes.analysisOrganization.value=value;
 const document={getElementById:id=>nodes[id],createElement:()=>new Element()};
 let calls=0;
 const window={nativeWorkspaceFilterChanged(){calls++;}};
 const script=fs.readFileSync(path.join(__dirname,'../native-organization-filter.js'),'utf8');
 vm.runInNewContext(script,{window,document});
 const filter=window.nativeOrganizationFilter,select=nodes.analysisOrganization;
 return {filter,select,selected:()=>Array.from(filter.selected()),
  changes:()=>calls,
  check(index,checked){
   const input=nodes.analysisOrganizationChoices.children[index+1].children[0];
   input.checked=checked;input.events.change();
  }};
}

test('an arbitrary subset synchronizes with the existing all/single dropdown and survives refresh',()=>{
 const {filter,select,selected,check,changes}=mount('B');
 filter.setOrganizations(['A','B','C','D']);
 filter.setLocked(false);
 assert.deepEqual(selected(),['B']);
 check(3,true);
 assert.equal(select.value,'__custom__');
 assert.deepEqual(selected(),['B','D']);
 filter.setOrganizations(['A','B','D','E']);
 assert.deepEqual(selected(),['B','D']);
 assert.equal(changes(),1);
 select.value='*';select.events.change();
 assert.deepEqual(selected(),['A','B','D','E']);
 filter.setOrganizations(['A','B','D','E','F']);
 assert.deepEqual(selected(),['A','B','D','E','F']);
});

test('explicit empty and reserved organization names are never confused with dropdown sentinels',()=>{
 const {filter,select,selected,check,changes}=mount();
 filter.setOrganizations(['*','__custom__','C']);
 filter.setLocked(false);
 check(0,false);check(1,false);check(2,false);
 assert.equal(select.value,'__custom__');
 assert.deepEqual(selected(),[]);
 check(1,true);
 assert.notEqual(select.value,'__custom__');
 assert.deepEqual(selected(),['__custom__']);
 filter.setLocked(true);
 check(0,true);
 assert.deepEqual(selected(),['__custom__']);
 assert.equal(changes(),4);
 filter.setOrganizations([]);
 assert.deepEqual(selected(),[]);
});

test('explicit single choice remains restricted when settings add an organization',()=>{
 const {filter,select,selected}=mount();
 filter.setOrganizations(['A']);
 filter.setLocked(false);
 select.value='A';select.events.change();
 assert.equal(select.value,'A');
 filter.setOrganizations(['A','B']);
 assert.deepEqual(selected(),['A']);
 assert.equal(select.value,'A');
});
