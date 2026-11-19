(function(){
  'use strict';
  const tauri=window.__TAURI__;
  if(!tauri?.core?.invoke || !tauri?.dialog?.open) {
    throw Error('Tauri API не инициализирован. Откройте приложение через Tauri, а не как файл HTML.');
  }
  const invoke=(command,args)=>tauri.core.invoke(command,args||{});
  const operations=window.tauriOperations={saves:0,reads:0,updating:false};
  function reportOperation(command){
    // Activity observers must never interrupt IPC or hide its original error.
    try{
      if(typeof window.dispatchEvent==='function'&&typeof CustomEvent==='function')
        window.dispatchEvent(new CustomEvent('tauri-operation-change',{
          detail:{reads:operations.reads,saves:operations.saves,command}
        }));
    }catch{}
  }
  async function tracked(command,args,kind){
    if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
    operations[kind]++;
    reportOperation(command);
    try{return await invoke(command,args);}finally{operations[kind]--;reportOperation(command);}
  }
  window.desktopAPI=Object.freeze({
    load:()=>invoke('load_data'),
    nativeRevision:()=>tracked('native_read_revision',{},'reads'),
    nativeSettingsBootstrap:()=>tracked('native_settings_bootstrap',{},'reads'),
    nativeGroupPage:(revision,afterHandle=null,limit=50)=>tracked(
      'native_group_page',{revision,afterHandle,limit},'reads'),
    nativeGroupRename:(revision,handle,name)=>tracked(
      'native_group_rename',{revision,handle,name},'saves'),
    nativeMemberPage:(revision,group,afterHandle=null,limit=50)=>tracked(
      'native_member_page',{revision,group,afterHandle,limit},'reads'),
    nativeCatalogPage:(revision,afterHandle=null,limit=50)=>tracked(
      'native_catalog_page',{revision,afterHandle,limit},'reads'),
    nativeReportQualityPage:(revision,id,scope,offset=0,limit=50)=>tracked(
      'native_report_quality_page',{revision,id,scope,offset,limit},'reads'),
    nativeDeleteReport:(revision,id,backupDestination)=>tracked(
      'native_delete_report',{revision,id,backupDestination},'saves'),
    nativeSettingsEdit:(revision,action)=>tracked(
      'native_settings_edit',{revision,action},'saves'),
    nativeAnalysisIndexPage:(revision,reportType,after=null,limit=20)=>tracked(
      'native_analysis_index_page',{revision,reportType,after,limit},'reads'),
    nativeAnalysisNameOptions:(revision,search='',limit=50)=>tracked(
      'native_analysis_name_options',{revision,search,limit},'reads'),
    nativeAnalysisIndexSettings:revision=>tracked(
      'native_analysis_index_settings',{revision},'reads'),
    nativeAnalysisRange:(revision,reportType,from,to,organizations,selectedName=null,selectedNames=null)=>tracked(
      'native_analysis_range',{revision,reportType,from,to,organizations,selectedName,selectedNames},'reads'),
    nativeAnalysisTimeline:(revision,reportType,from,to,organizations,selectedName=null,grouping=null,selectedNames=null)=>tracked(
      'native_analysis_timeline',{revision,reportType,from,to,organizations,selectedName,grouping,selectedNames},'reads'),
    nativeOrganizationTrends:(revision,reportType,from,to,organizations,selectedName=null,grouping=null,selectedNames=null)=>tracked(
      'native_analysis_organization_trends',{revision,reportType,from,to,organizations,selectedName,grouping,selectedNames},'reads'),
    nativeDailyRollup:(revision,from,to,granularity,organizations,after,limit,selectedName=null,selectedNames=null)=>tracked(
      'native_daily_rollup',{revision,from,to,granularity,organizations,after,limit,selectedName,selectedNames},'reads'),
    nativeChangesPage:(request,after)=>tracked('native_changes_page',{request,after},'reads'),
    nativeChangesCsv:(request,destination)=>tracked('native_changes_csv',{request,destination},'saves'),
    nativeCompositionPage:(request,before,mode,metric,after)=>tracked('native_composition_page',{request,before,mode,metric,after},'reads'),
    nativePatternPage:(request,mode,windowSize,after)=>tracked('native_pattern_page',{request,mode,windowSize,after},'reads'),
    nativeAnalysisCompare:(revision,reportType,beforeFrom,beforeTo,from,to,organizations,selectedName=null,selectedNames=null)=>tracked(
      'native_analysis_compare',{revision,reportType,beforeFrom,beforeTo,from,to,organizations,selectedName,selectedNames},'reads'),
    nativeWorkTimeline:(revision,reportType,from,to,organizations,organization,key,selectedName=null,grouping=null,selectedNames=null)=>tracked(
      'native_analysis_work_timeline',{revision,reportType,from,to,organizations,organization,key,selectedName,grouping,selectedNames},'reads'),
    nativeAnalysisRanking:(revision,reportType,from,to,organizations,organization,metric,category,selectedName=null,selectedNames=null)=>tracked(
      'native_analysis_ranking',{revision,reportType,from,to,organizations,organization,metric,category,selectedName,selectedNames},'reads'),
    nativeAnalysisOrganizations:(revision,reportType,from,to,organizations,selectedName=null,selectedNames=null)=>tracked(
      'native_analysis_organizations',{revision,reportType,from,to,organizations,selectedName,selectedNames},'reads'),
    nativeAnalysisRangePage:(revision,reportType,from,to,organizations,organization=null,after=null,limit=50,selectedName=null,selectedNames=null)=>tracked(
      'native_analysis_range_page',{revision,reportType,from,to,organizations,organization,after,limit,selectedName,selectedNames},'reads'),
    chooseCsvPath:async defaultPath=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      if(typeof tauri.dialog.save!=='function')throw Error('Диалог сохранения CSV недоступен.');
      return tauri.dialog.save({title:'Сохранить результаты анализа',defaultPath,filters:[{name:'CSV',extensions:['csv']}]});
    },
    nativeExportRangeCsv:(revision,reportType,from,to,organizations,organization,destination,selectedName=null,selectedNames=null)=>tracked(
      'native_export_range_csv',{revision,reportType,from,to,organizations,organization,destination,selectedName,selectedNames},'saves'),
    chooseWideXlsx:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return tauri.dialog.open({multiple:false,directory:false,title:'Выберите XLSX для нативного импорта',filters:[{name:'Excel XLSX',extensions:['xlsx']}]});
    },
    nativeFolderImportSession:enabled=>enabled?tracked('native_folder_import_session',{enabled},'saves'):invoke('native_folder_import_session',{enabled}),
    nativeFolderImportPreview:request=>tracked('native_folder_import_preview',{request},'reads'),
    nativeFolderImportApply:(request,reviewToken)=>tracked('native_folder_import_apply',{request,reviewToken},'saves'),
    nativeWideImportPreview:(source,options)=>tracked('native_wide_import_preview',{source,options},'reads'),
    nativeWideImportApply:(source,options,reviewToken,confirmation)=>tracked(
      'native_wide_import_apply',{source,options,reviewToken,confirmation},'saves'),
    nativeLegacyImportSheets:source=>tracked(
      'native_legacy_import_sheets',{source},'reads'),
    nativeLegacyImportPreview:(source,options)=>tracked(
      'native_legacy_import_preview',{source,options},'reads'),
    nativeLegacyImportApply:(source,options,reviewToken,confirmation)=>tracked(
      'native_legacy_import_apply',{source,options,reviewToken,confirmation},'saves'),
    chooseSqliteBackupPath:async defaultPath=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      if(typeof tauri.dialog.save!=='function')throw Error('Диалог резервного копирования недоступен.');
      return tauri.dialog.save({title:'Сохранить резервную копию SQLite',defaultPath,filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    nativeBackupSqlite:destination=>tracked('native_backup_sqlite',{destination},'saves'),
    chooseRestoreBackupPath:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return tauri.dialog.open({multiple:false,directory:false,title:'Проверить резервную копию SQLite',filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    chooseRestoreDestination:async defaultPath=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return tauri.dialog.save({title:'Создать отдельную восстановленную копию',defaultPath,filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    nativeRestorePreview:source=>tracked('native_restore_preview',{source},'reads'),
    nativeRestoreCopy:(source,verification,destination)=>tracked(
      'native_restore_copy',{source,verification,destination},'saves'),
    nativeRestoreSchedule:(source,verification)=>tracked(
      'native_restore_schedule',{source,verification},'saves'),
    nativeRestoreLaunchInfo:()=>tracked('native_restore_launch_info',{},'reads'),
    nativeRestoreCancel:()=>tracked('native_restore_cancel',{},'saves'),
    nativeMetricKinds:()=>tracked('native_metric_kinds',{},'reads'),
    chooseMetricJson:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return tauri.dialog.open({multiple:false,directory:false,title:'Выберите JSON для нового вида отчёта',filters:[{name:'JSON',extensions:['json']}]});
    },
    nativeMetricPreview:source=>tracked('native_metric_preview',{source},'reads'),
    nativeMetricApply:(source,reviewToken)=>tracked('native_metric_apply',{source,reviewToken},'saves'),
    nativeMetricSummary:(kind,start,end)=>tracked('native_metric_summary',{kind,start,end},'reads'),
    nativeMetricXlsxProfiles:()=>tracked('native_metric_xlsx_profiles',{},'reads'),
    chooseMetricXlsx:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return tauri.dialog.open({multiple:false,directory:false,title:'Выберите XLSX для нового вида отчёта',filters:[{name:'Excel XLSX',extensions:['xlsx']}]});
    },
    nativeMetricXlsxPreview:(source,profile,start,end)=>tracked(
      'native_metric_xlsx_preview',{source,profile,start,end},'reads'),
    nativeMetricXlsxApply:(source,profile,start,end,reviewToken)=>tracked(
      'native_metric_xlsx_apply',{source,profile,start,end,reviewToken},'saves'),
    nativeReportPage:(revision,filter,after=null,limit=100)=>tracked(
      'native_report_page',{revision,filter,after,limit},'reads'),
    save:data=>tracked('save_data',{data},'saves'),
    info:()=>invoke('get_info'),
    chooseFolder:async()=>{
      const folder=await tauri.dialog.open({directory:true,multiple:false,title:'Папка с Excel-отчётами'});
      return folder?tracked('set_folder',{folder},'saves'):null;
    },
    list:()=>invoke('list_excel'),
    setRule:(name,rule)=>tracked('set_rule',{name,rule},'saves'),
    nativeSetRuleChecked:(name,rule,revision,folder,signature)=>tracked(
      'native_set_rule_checked',{name,rule,revision,folder,signature},'saves'),
    readExcel:name=>tracked('read_excel',{name},'reads'),
    checkExcel:(name,signature)=>invoke('check_excel',{name,signature}),
    openDataFolder:()=>invoke('open_data_folder'),
    // The webview checks the folder while the program is open. It rescans at startup.
    onFilesChanged:handler=>{
      let previous=null,hadError=false,active=true,busy=false;
      const poll=async()=>{
        if(!active||busy)return;
        busy=true;
        try{
          const result=await invoke('list_excel');
          const fingerprint=JSON.stringify([
            result.folder,
            result.files.map(file=>[file.name,file.signature,file.nativeSignature])
          ]);
          const changed=previous!==null && fingerprint!==previous;
          const recovered=hadError;
          previous=fingerprint;
          hadError=false;
          if(active&&(changed||recovered))handler();
        }catch(err){
          hadError=true;
          // The renderer reports errors during its regular scan.
        }finally{busy=false;}
      };
      poll();
      const timer=setInterval(poll,3000);
      return ()=>{active=false;clearInterval(timer);};
    }
  });
})();

