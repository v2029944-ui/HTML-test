'use strict';
const fs=require('node:fs');
const path=require('node:path');
const {classicAssets}=require('./classic-ui.cjs');

function prepareHtml(source){
  if(!source.includes('window.appReady=main();'))
    throw Error('Недостаточно совместимая версия index.html: точка запуска не обнаружена.');
  if(source.includes('tauri-bridge.js')||source.includes('tauri-folder-importer.js')||source.includes('tauri-desktop.js')||source.includes('tauri-updates.js')||source.includes('tauri-native-reader.js')||source.includes('tauri-native-errors.js'))
    throw Error('Tauri-скрипты уже присутствуют в исходном HTML.');
  if((source.match(/<\/head>/g)||[]).length!==1||(source.match(/<\/body>/g)||[]).length!==1)
    throw Error('Не найдены однозначные границы HTML.');
  // Keep the original HTML unchanged; inject Tauri access only into the generated copy.
  return source
    .replace('</head>','<link rel="stylesheet" href="tauri-app-status.css">\n<script src="tauri-bridge.js"></script>\n</head>')
    .replace('</body>','<script src="tauri-folder-importer.js"></script>\n<script src="tauri-native-reader.js"></script>\n<script src="tauri-app-status.js"></script>\n<script src="tauri-updates.js"></script>\n</body>');
}

function prepare(options={}){
  const base=options.base||path.resolve(__dirname,'..');
  const output=options.output||path.join(__dirname,'web');
  const source=fs.readFileSync(path.join(base,'index.html'),'utf8');
  fs.mkdirSync(output,{recursive:true});
  const appearance=classicAssets(source);
  fs.writeFileSync(path.join(output,'tauri-classic.css'),appearance.css);
  fs.writeFileSync(path.join(output,'tauri-appearance.js'),appearance.script);
  fs.copyFileSync(path.join(__dirname,'native-skin.css'),path.join(output,'tauri-native-skin.css'));
  fs.copyFileSync(path.join(__dirname,'app-status.js'),path.join(output,'tauri-app-status.js'));
  fs.copyFileSync(path.join(__dirname,'app-status.css'),path.join(output,'tauri-app-status.css'));
  fs.writeFileSync(path.join(output,'index.html'),prepareHtml(source));
  fs.copyFileSync(path.join(__dirname,'bridge.js'),path.join(output,'tauri-bridge.js'));
  fs.copyFileSync(path.join(__dirname,'updates.js'),path.join(output,'tauri-updates.js'));
  fs.copyFileSync(path.join(__dirname,'native-updates.js'),path.join(output,'tauri-native-updates.js'));
  fs.copyFileSync(path.join(__dirname,'native-reader.js'),path.join(output,'tauri-native-reader.js'));
  fs.copyFileSync(path.join(__dirname,'native-errors.js'),path.join(output,'tauri-native-errors.js'));
  fs.copyFileSync(path.join(__dirname,'native-workspace.js'),path.join(output,'tauri-native-workspace.js'))
  fs.copyFileSync(path.join(__dirname,'native-management.js'),path.join(output,'tauri-native-management.js'));
  fs.copyFileSync(path.join(__dirname,'native-report-quality.js'),path.join(output,'tauri-native-report-quality.js'));
  fs.copyFileSync(path.join(__dirname,'native-period-visibility.js'),path.join(output,'tauri-native-period-visibility.js'));
  fs.copyFileSync(path.join(__dirname,'native-timeline.js'),path.join(output,'tauri-native-timeline.js'));
  fs.copyFileSync(path.join(__dirname,'native-ranking.js'),path.join(output,'tauri-native-ranking.js'));
  fs.copyFileSync(path.join(__dirname,'native-organization-trends.js'),path.join(output,'tauri-native-organization-trends.js'));
  fs.copyFileSync(path.join(__dirname,'native-period-selection.js'),path.join(output,'tauri-native-period-selection.js'));
  fs.copyFileSync(path.join(__dirname,'native-daily-overview.js'),path.join(output,'tauri-native-daily-overview.js'));
  fs.copyFileSync(path.join(__dirname,'native-name-filter.js'),path.join(output,'tauri-native-name-filter.js'));
  fs.copyFileSync(path.join(__dirname,'native-organization-filter.js'),path.join(output,'tauri-native-organization-filter.js'));
  fs.copyFileSync(path.join(__dirname,'native-daily-calendar.js'),path.join(output,'tauri-native-daily-calendar.js'));
  fs.copyFileSync(path.join(__dirname,'native-work-trends.js'),path.join(output,'tauri-native-work-trends.js'));
  fs.copyFileSync(path.join(__dirname,'native-advanced-analysis.js'),path.join(output,'tauri-native-advanced-analysis.js'));
  fs.copyFileSync(path.join(__dirname,'native-comparison.js'),path.join(output,'tauri-native-comparison.js'));
  fs.copyFileSync(path.join(__dirname,'native-navigation.js'),path.join(output,'tauri-native-navigation.js'));
  fs.copyFileSync(path.join(__dirname,'native-report-calendar.js'),path.join(output,'tauri-native-report-calendar.js'));
  fs.copyFileSync(path.join(__dirname,'native-folder-auto-import.js'),path.join(output,'tauri-native-folder-auto-import.js'));
  fs.copyFileSync(path.join(__dirname,'native-folder-status.js'),path.join(output,'tauri-native-folder-status.js'));
  fs.copyFileSync(path.join(__dirname,'native-legacy-import.js'),path.join(output,'tauri-native-legacy-import.js'));
  fs.copyFileSync(path.join(__dirname,'native-import-mappings.js'),path.join(output,'tauri-native-import-mappings.js'));
  fs.copyFileSync(path.join(__dirname,'native-import-wizard.js'),path.join(output,'tauri-native-import-wizard.js'));
  fs.copyFileSync(path.join(__dirname,'native-wide-import.js'),path.join(output,'tauri-native-wide-import.js'));
  fs.copyFileSync(path.join(__dirname,'native-metric-kinds.js'),path.join(output,'tauri-native-metric-kinds.js'));
  const workspace=fs.readFileSync(path.join(__dirname,'native-workspace.html'),'utf8');
  fs.writeFileSync(path.join(output,'native-workspace.html'),workspace.replace('<!-- CLASSIC_APPEARANCE -->',appearance.markup));
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

