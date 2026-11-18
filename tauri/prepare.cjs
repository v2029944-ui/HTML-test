'use strict';
const fs=require('node:fs');
const path=require('node:path');

function prepareHtml(source){
  if(!source.includes('window.appReady=main();'))
    throw Error('Недостаточно совместимая версия index.html: точка запуска не обнаружена.');
  if(source.includes('tauri-bridge.js')||source.includes('tauri-folder-importer.js')||source.includes('tauri-desktop.js')||source.includes('tauri-updates.js')||source.includes('tauri-native-reader.js')||source.includes('tauri-native-errors.js'))
    throw Error('Tauri-скрипты уже присутствуют в исходном HTML.');
  if((source.match(/<\/head>/g)||[]).length!==1||(source.match(/<\/body>/g)||[]).length!==1)
    throw Error('Не найдены однозначные границы HTML.');
  // Keep the original HTML unchanged; inject Tauri access only into the generated copy.
  return source
    .replace('</head>','<script src="tauri-bridge.js"></script>\n</head>')
    .replace('</body>','<script src="tauri-folder-importer.js"></script>\n<script src="tauri-native-reader.js"></script>\n<script src="tauri-updates.js"></script>\n</body>');
}

function prepare(options={}){
  const base=options.base||path.resolve(__dirname,'..');
  const output=options.output||path.join(__dirname,'web');
  const source=fs.readFileSync(path.join(base,'index.html'),'utf8');
  fs.mkdirSync(output,{recursive:true});
  fs.writeFileSync(path.join(output,'index.html'),prepareHtml(source));
  fs.copyFileSync(path.join(__dirname,'bridge.js'),path.join(output,'tauri-bridge.js'));
  fs.copyFileSync(path.join(__dirname,'updates.js'),path.join(output,'tauri-updates.js'));
  fs.copyFileSync(path.join(__dirname,'native-updates.js'),path.join(output,'tauri-native-updates.js'));
  fs.copyFileSync(path.join(__dirname,'native-reader.js'),path.join(output,'tauri-native-reader.js'));
  fs.copyFileSync(path.join(__dirname,'native-errors.js'),path.join(output,'tauri-native-errors.js'));
  fs.copyFileSync(path.join(__dirname,'native-workspace.js'),path.join(output,'tauri-native-workspace.js'))
  fs.copyFileSync(path.join(__dirname,'native-folder-status.js'),path.join(output,'tauri-native-folder-status.js'));
  fs.copyFileSync(path.join(__dirname,'native-wide-import.js'),path.join(output,'tauri-native-wide-import.js'));
  fs.copyFileSync(path.join(__dirname,'native-metric-kinds.js'),path.join(output,'tauri-native-metric-kinds.js'));
  fs.copyFileSync(path.join(__dirname,'native-workspace.html'),path.join(output,'native-workspace.html'));
  fs.copyFileSync(path.join(__dirname,'startup.html'),path.join(output,'startup.html'));
  fs.copyFileSync(path.join(__dirname,'startup.js'),path.join(output,'tauri-startup.js'));
  fs.copyFileSync(path.join(__dirname,'startup-preference.js'),path.join(output,'tauri-startup-preference.js'));
  const renderer=fs.readFileSync(path.join(__dirname,'folder-importer.js'),'utf8');
  // The folder importer uses Tauri's compatible API; adjust only storage hints.
  const adapted=renderer.replace(
    'Резервная копия: reports.json.bak. Данные не находятся в браузере.',
    'Используется SQLite. Исходный reports.json сохраняется после переноса; делайте отдельные резервные копии.'
  );
  fs.writeFileSync(path.join(output,'tauri-folder-importer.js'),adapted);
  return {sourceBytes:Buffer.byteLength(source,'utf8'),target:output};
}

if(require.main===module){
  const result=prepare();
  process.stdout.write('Prepared Tauri HTML from original ('+result.sourceBytes+' bytes)\n');
}
module.exports={prepareHtml,prepare};

