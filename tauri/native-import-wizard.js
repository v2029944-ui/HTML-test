// Single file entry point. Each native importer still owns review and commit.
(function(){
'use strict';
const $=id=>document.getElementById(id);
let busy=false;
$('importWizardChoose').addEventListener('click',async()=>{
 if(busy)return;
 const importer=$('importWizardFormat').value==='wide'?window.nativeWideImporter:window.nativeLegacyImporter;
 if(!importer?.selectSource){$('importWizardStatus').textContent='Импорт недоступен. Перезапустите приложение.';return;}
 if(window.nativeLegacyImporter?.busy()||window.nativeWideImporter?.busy())return;
 busy=true;$('importWizardChoose').disabled=true;$('importWizardFormat').disabled=true;
 try{
  const source=await window.desktopAPI.chooseWideXlsx();
  if(!source){$('importWizardStatus').textContent='Выбор отменён.';return;}
  if(window.nativeLegacyImporter?.busy()||window.nativeWideImporter?.busy()){ $('importWizardStatus').textContent='Дождитесь завершения текущего импорта и повторите выбор.';return;}
  window.nativeLegacyImporter?.invalidate?.();window.nativeWideImporter?.invalidate?.();
  const opened=await importer.selectSource(source);
  if(opened===false){$('importWizardStatus').textContent='Файл не открыт. Проверьте сообщение формы импорта ниже.';return;}
  $('importWizardStatus').textContent='Файл передан в форму ниже. Уточните лист, периоды и столбцы, выполните проверку, затем подтвердите сохранение.';
 }catch(error){$('importWizardStatus').textContent='Не удалось выбрать файл. Проверьте доступ и повторите действие.';}
 finally{busy=false;$('importWizardChoose').disabled=false;$('importWizardFormat').disabled=false;}
});
})();
