'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
function mount(){
 const e=Object.fromEntries(['importWizardChoose','importWizardFormat','importWizardStatus'].map(id=>[id,{value:'legacy',events:{},addEventListener(k,v){this.events[k]=v;}}]));
 const calls=[];
 const window={desktopAPI:{chooseWideXlsx:async()=>{calls.push('choose');return 'C:/files/report.xlsx';}},nativeLegacyImporter:{busy:()=>false,selectSource:async path=>calls.push(['legacy',path])},nativeWideImporter:{busy:()=>false,selectSource:async path=>calls.push(['wide',path])}};
 vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../native-import-wizard.js'),'utf8'),{window,document:{getElementById:id=>e[id]}});
 return {e,calls,window};
}
test('one file choice hands source to selected native review without committing reports',async()=>{
 const {e,calls}=mount();await e.importWizardChoose.events.click();
 assert.deepEqual(calls,['choose',['legacy','C:/files/report.xlsx']]);
 e.importWizardFormat.value='wide';await e.importWizardChoose.events.click();
 assert.deepEqual(calls.at(-1),['wide','C:/files/report.xlsx']);
 assert.match(e.importWizardStatus.textContent,/провер/);
});

test('wizard reports failed handoff instead of claiming a file is ready',async()=>{
 const {e,window}=mount();window.nativeLegacyImporter.selectSource=async()=>false;
 await e.importWizardChoose.events.click();assert.match(e.importWizardStatus.textContent,/не открыт/);
});
