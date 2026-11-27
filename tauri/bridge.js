(function(){
  'use strict';
  const tauri=window.__TAURI__;
  if(!tauri?.core?.invoke || !tauri?.dialog?.open) {
    throw Error('Tauri API не инициализирован. Откройте приложение через Tauri, а не как файл HTML.');
  }
  const invoke=(command,args)=>window.sharedWorkspace
    ?window.sharedWorkspace.invoke(command,args||{}):tauri.core.invoke(command,args||{});
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
  // Native pickers disable their owner HWND; remember the actual visible opener
  // before an async controller disables its hidden file input or button.
  const document=window.document;
  let activation=null,pickerOpen=false,interaction=0,pendingFocus=null;
  const restoredLabels=new WeakSet();
  function visibleOpener(node){
    return node?.closest?.('label')||node?.closest?.('button,a,input,select,textarea,[tabindex]')||null;
  }
  function newInteraction(){
    if(pickerOpen)return;
    interaction++;
    pendingFocus?.();
  }
  document?.addEventListener('pointerdown',newInteraction,true);
  document?.addEventListener('click',event=>{
    const opener=visibleOpener(event.target);
    if(!opener)return;
    if(pickerOpen){event.preventDefault();event.stopImmediatePropagation();return;}
    newInteraction();
    activation=opener;
  },true);
  document?.addEventListener('keydown',event=>{
    newInteraction();
    const opener=document.activeElement;
    if(!restoredLabels.has(opener)||!['Enter',' '].includes(event.key)||opener.control?.disabled)return;
    event.preventDefault();
    if(!pickerOpen)opener.click();
  },true);
  function restoreOpener(opener,expectedInteraction){
    if(!opener||expectedInteraction!==interaction)return;
    pendingFocus?.();
    const initialFocus=document?.activeElement;
    let observer=null,timer=null,expiry=null,finished=false;
    const stop=()=>{
      if(finished)return;
      finished=true;observer?.disconnect();
      clearTimeout(timer);clearTimeout(expiry);
      if(pendingFocus===stop)pendingFocus=null;
    };
    const attempt=()=>{
      if(finished)return;
      const active=document?.activeElement;
      if(expectedInteraction!==interaction||!opener.isConnected||
         (active!==initialFocus&&active!==opener&&active!==opener.control&&active!==document?.body)){
        stop();return;
      }
      // Folder cancellation reloads metadata asynchronously and disables its
      // button again. Wait for the real controller unlock, not one timer tick.
      if(opener.disabled||opener.control?.disabled||opener.getAttribute('aria-busy')==='true'||
         opener.getAttribute('aria-disabled')==='true'||
         opener.control?.getAttribute?.('aria-busy')==='true')return;
      if(!opener.getClientRects().length){stop();return;}
      stop();
      if(opener.tagName==='LABEL'){
        restoredLabels.add(opener);
        if(!opener.hasAttribute('tabindex')){
          opener.setAttribute('tabindex','0');
          opener.addEventListener('blur',()=>opener.removeAttribute('tabindex'),{once:true});
        }
      }
      opener.focus({preventScroll:true});
    };
    pendingFocus=stop;
    if(typeof MutationObserver==='function'){
      observer=new MutationObserver(attempt);
      for(const node of new Set([opener,opener.control].filter(Boolean)))
        observer.observe(node,{attributes:true,attributeFilter:['disabled','aria-busy','aria-disabled']});
    }
    timer=setTimeout(attempt,0);
    expiry=setTimeout(stop,90_000);
  }
  // The dialog plugin supplies an asynchronous confirmation. Controllers may
  // temporarily disable their opener while the native owner window is blocked.
  const originalConfirm=window.confirm;
  if(typeof originalConfirm==='function')window.confirm=function(...args){
    const opener=visibleOpener(document?.activeElement)||(activation?.isConnected?activation:null);
    const expectedInteraction=interaction;
    const token=window.sharedWorkspace?.state?.workspace_token;
    const answer=originalConfirm.apply(this,args);
    const finish=accepted=>{if(accepted===false)restoreOpener(opener,expectedInteraction);return accepted&&token!==window.sharedWorkspace?.state?.workspace_token?false:accepted;};
    return answer&&typeof answer.then==='function'?answer.then(finish):finish(answer);
  };
  async function nativePicker(kind,options){
    if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
    if(pickerOpen)throw Error('Закройте открытый диалог выбора файла.');
    const token=window.sharedWorkspace?.state?.workspace_token;
    const active=document?.activeElement;
    const opener=(activation?.isConnected?activation:visibleOpener(active));
    const expectedInteraction=interaction;
    const oldBusy=opener?.getAttribute('aria-busy'),oldDisabled=opener?.getAttribute('aria-disabled');
    pickerOpen=true;
    opener?.setAttribute('aria-busy','true');opener?.setAttribute('aria-disabled','true');
    let cancelled=true;
    try{
      const result=kind==='sharedFolder' ? await tauri.core.invoke('shared_workspace_choose_folder') : await tauri.dialog[kind](options);
      if(token!==window.sharedWorkspace?.state?.workspace_token)throw Error('Активная база изменилась. Повторите выбор.');
      cancelled=result===null||result===undefined||result===false||result==='';
      return result;
    }finally{
      pickerOpen=false;
      if(opener){
        if(oldBusy===null)opener.removeAttribute('aria-busy');else opener.setAttribute('aria-busy',oldBusy);
        if(oldDisabled===null)opener.removeAttribute('aria-disabled');else opener.setAttribute('aria-disabled',oldDisabled);
      }
      if(cancelled)restoreOpener(opener,expectedInteraction);
    }
  }
  async function referenceChangesPrint(request,onRows,signal){
    const batches=new tauri.core.Channel();
    let control=null,offset=0,abort;
    // The producer allows one outstanding batch. Cancellation also interrupts SQLite preparation.
    const delivered=new Promise((resolve,reject)=>{
      const reply=value=>tauri.event.emit(control,value).catch(reject);
      abort=()=>{if(control)void reply(null);};
      batches.onmessage=event=>{
        try{
          if(event.kind==='ready'){control=event.control;if(signal?.aborted)abort();return;}
          if(event.kind==='done'){resolve();return;}
          if(signal?.aborted)return;
          if(event.kind!=='rows'||!control||event.offset!==offset||!Array.isArray(event.rows)||
             !event.rows.length||event.rows.length>128)throw Error('Нарушен порядок строк печати.');
          onRows(event.rows);
          offset+=event.rows.length;void reply(offset);
        }catch(error){abort();reject(error);}
      };
    });
    signal?.addEventListener('abort',abort,{once:true});
    try{
      const [comparable]=await Promise.all([tracked('reference_changes_print',{request,batches},'reads'),delivered]);
      return comparable;
    }finally{signal?.removeEventListener('abort',abort);}
  }
  window.desktopAPI=Object.freeze({
    load:()=>invoke('load_data'),
    referenceChangesPrint,
    referenceChanges:request=>tracked('reference_changes',{request},'reads'),
    referenceChangesCsv:(request,destination)=>tracked('reference_changes_csv',{request,destination},'saves'),
    referenceSummaryCsv:(request,destination)=>tracked('reference_summary_csv',{request,destination},'saves'),
    unifiedQualityCsv:(revision,filter,destination)=>tracked('unified_quality_csv',{revision,filter,destination},'saves'),
    unifiedGroupAuditCsv:(request,mode,search,destination)=>tracked('unified_group_audit_csv',{request,mode,search,destination},'saves'),
    nativePreviewQualityPage:(query,offset)=>tracked('native_preview_quality_page',{query,offset},'reads'),
    nativePreviewQualityCsv:(query,destination)=>tracked('native_preview_quality_csv',{query,destination},'saves'),
    unifiedPeriodOptions:(revision,reportType,grouping=null,after=null,limit=100,weekCalendarMonth=null)=>tracked('unified_period_options',{revision,reportType,grouping,after,limit,weekCalendarMonth},'reads'),
    unifiedNameOptions:(revision,search='',after=null,limit=50)=>tracked('unified_name_options',{revision,search,after,limit},'reads'),
    unifiedSelectedNameOptions:(revision,selectedKeys)=>tracked('unified_name_options',{revision,search:'',after:null,limit:50,selectedKeys},'reads'),
    unifiedGroupChildrenAtPage:(revision,parentKey,pageIndex)=>tracked('unified_name_options',{revision,search:'',limit:50,pageIndex,parentKey},'reads'),
    unifiedNameOptionsAtPage:(revision,search='',pageIndex=0,limit=50,allSeries=false)=>tracked('unified_name_options',{revision,search,after:null,limit,pageIndex,...(allSeries?{allSeries:true}:{})},'reads'),
    unifiedChangesPage:(request,options,after=null)=>tracked('unified_changes_page',{request,options,after},'reads'),
    unifiedChangesPageAtPage:(request,options,pageIndex=0)=>tracked('unified_changes_page',{request,options,after:null,pageIndex},'reads'),
    unifiedChangesCsv:(request,options,destination)=>tracked('unified_changes_csv',{request,options,destination},'saves'),
    referenceTrendCsv:(request,options,destination)=>tracked('reference_trend_csv',{request,options,destination},'saves'),
    referenceDetailCsv:(payload,destination)=>tracked('reference_detail_csv',{payload,destination},'saves'),
    unifiedPatternPage:(request,mode,windowSize,options,after=null)=>tracked('unified_pattern_page',{request,mode,windowSize,options,after},'reads'),
    unifiedPatternPageAtPage:(request,mode,windowSize,options,pageIndex=0)=>tracked('unified_pattern_page',{request,mode,windowSize,options,after:null,pageIndex},'reads'),
    unifiedComparisonPage:(request,before,organization=null,after=null,limit=50)=>tracked('unified_comparison_page',{request,before,organization,after,limit},'reads'),
    unifiedOrganizationWorkWindow:(request,pageIndex=0,limit=50)=>tracked('unified_organization_work_window',{request,pageIndex,limit},'reads'),
    unifiedComparisonWindow:(request,before,organization=null,pageIndex=0,limit=50)=>tracked('unified_comparison_window',{request,before,organization,pageIndex,limit},'reads'),
    unifiedGroupAuditPage:(request,mode,search='',offset=0,limit=50)=>tracked('unified_group_audit_page',{request,mode,search,offset,limit},'reads'),
    unifiedManagementPage:(revision,kind,query='',group=null,offset=0,limit=50)=>tracked('unified_management_page',{revision,kind,query,group,offset,limit},'reads'),
    unifiedGroupSave:(revision,handle,id,name,works)=>tracked('unified_group_save',{revision,handle,id,name,works},'saves'),
    unifiedGroupTreeEdit:(revision,action)=>tracked('unified_group_tree_edit',{revision,action},'saves'),
    unifiedWorkAssign:(revision,work,group)=>tracked('unified_work_assign',{revision,work,group},'saves'),
    unifiedGroupsPreview:(revision,source)=>tracked('unified_groups_preview',{revision,source},'reads'),
    unifiedGroupsImport:(revision,source,hash,confirmed,backup)=>tracked('unified_groups_import',{revision,source,hash,confirmed,...(backup?{backup}:{})},'saves'),
    unifiedGroupsDelete:(revision,handles,confirmedCount,backup)=>tracked('unified_groups_delete',{revision,handles,confirmedCount,backup,confirmed:true},'saves'),
    unifiedGroupsExport:(revision,destination)=>tracked('unified_groups_export',{revision,destination},'saves'),
    unifiedCatalogPreview:(revision,source)=>tracked('unified_catalog_preview',{revision,source},'reads'),
    unifiedCatalogApply:(revision,source,hash,confirmed)=>tracked('unified_catalog_apply',{revision,source,hash,confirmed},'saves'),
    unifiedRenameOrganizations:(revision,names,destination)=>tracked('unified_rename_organizations',{revision,names,destination},'saves'),
    unifiedClearReports:revision=>tracked('unified_clear_reports',{revision},'saves'),
    unifiedReportDeletionCount:(revision,scope)=>tracked('unified_report_deletion_count',{revision,scope},'reads'),
    unifiedDeleteReports:(revision,scope,confirmedCount)=>tracked('unified_delete_reports',{revision,scope,confirmedCount},'saves'),
    managementReportPage:(revision,filter,after=null,limit=50)=>tracked('unified_report_page',{revision,filter,after,limit},'reads'),
    managementReportWindow:(revision,filter,pageIndex=0,limit=50)=>tracked('unified_report_window',{revision,filter,pageIndex,limit},'reads'),
    managementReportSummary:(revision,filter)=>tracked('unified_report_summary',{revision,filter},'reads'),
    managementReportOrgPage:(revision,id,after=null,limit=50)=>tracked('unified_report_org_page',{revision,id,after,limit},'reads'),
    managementReportOrgWindow:(revision,id,pageIndex=0,limit=50)=>tracked('unified_report_org_window',{revision,id,pageIndex,limit},'reads'),
    unifiedQualityPage:(revision,filter,offset=0,limit=50)=>tracked('unified_quality_page',{revision,filter,offset,limit},'reads'),
    nativeQualityMatrixPage:(revision,reportType,from,to,organizations)=>tracked('native_quality_matrix_page',{revision,reportType,from,to,organizations},'reads'),
    nativeQualityMatrixReports:(revision,reportType,start,organization=null,after=null,limit=20)=>tracked('native_quality_matrix_reports',{revision,reportType,start,organization,after,limit},'reads'),
    nativeQualityMatrixReportsAtPage:(revision,reportType,start,organization=null,pageIndex=0,limit=20)=>tracked('native_quality_matrix_reports_at_page',{revision,reportType,start,organization,pageIndex,limit},'reads'),
    unifiedCalendar:(revision,year)=>tracked('unified_calendar',{revision,year},'reads'),
    chooseXlsx:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Выберите справочник XLSX',filters:[{name:'Excel XLSX',extensions:['xlsx']}]});
    },
    chooseGroupsImport:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Импорт групп',filters:[{name:'JSON',extensions:['json']}]});
    },
    chooseGroupsExport:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      if(typeof tauri.dialog.save!=='function')throw Error('Диалог сохранения недоступен.');
      return nativePicker('save',{title:'Сохранить группы',defaultPath:'groups.json',filters:[{name:'JSON',extensions:['json']}]});
    },
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
    nativeDeleteReport:(revision,id)=>tracked(
      'native_delete_report',{revision,id},'saves'),
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
    nativeCompositionPage:(request,before,mode,metric,after,structureLimit=null)=>tracked('native_composition_page',{request,before,mode,metric,after,structureLimit},'reads'),
    nativeCompositionPageAtPage:(request,before,mode,metric,pageIndex=0,structureLimit=null)=>tracked('native_composition_page',{request,before,mode,metric,after:null,structureLimit,pageIndex},'reads'),
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
      return nativePicker('save',{title:'Сохранить результаты анализа',defaultPath,filters:[{name:'CSV',extensions:['csv']}]});
    },
    nativeExportRangeCsv:(revision,reportType,from,to,organizations,organization,destination,selectedName=null,selectedNames=null)=>tracked(
      'native_export_range_csv',{revision,reportType,from,to,organizations,organization,destination,selectedName,selectedNames},'saves'),
    chooseWideXlsx:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Выберите XLSX для нативного импорта',filters:[{name:'Excel XLSX',extensions:['xlsx']}]});
    },
    nativeFolderImportSession:enabled=>enabled?tracked('native_folder_import_session',{enabled},'saves'):invoke('native_folder_import_session',{enabled}),
    nativeFolderImportPreview:request=>tracked('native_folder_import_preview',{request},'reads'),
    nativeFolderImportApply:(request,reviewToken)=>tracked('native_folder_import_apply',{request,reviewToken},'saves'),
    nativeWideImportPreview:(source,options)=>tracked('native_wide_import_preview',{source,options},'reads'),
    nativeWideImportApply:(source,options,reviewToken,confirmation)=>tracked(
      'native_wide_import_apply',{source,options,reviewToken,confirmation},'saves'),
    nativeLegacyImportSheets:source=>tracked(
      'native_legacy_import_sheets',{source},'reads'),
    nativeImportFolderContext:source=>tracked('native_import_folder_context',{source},'reads'),
    nativeImportSheets:(source,rule=null)=>tracked('native_import_sheets',{source,...(rule?{rule}:{})},'reads'),
    nativeLegacyImportPreview:(source,options)=>tracked(
      'native_legacy_import_preview',{source,options},'reads'),
    nativeLegacyImportApply:(source,options,reviewToken,confirmation)=>tracked(
      'native_legacy_import_apply',{source,options,reviewToken,confirmation},'saves'),
    chooseSqliteBackupPath:async defaultPath=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      if(typeof tauri.dialog.save!=='function')throw Error('Диалог резервного копирования недоступен.');
      return nativePicker('save',{title:'Сохранить резервную копию SQLite',defaultPath,filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    nativeBackupSqlite:destination=>tracked('native_backup_sqlite',{destination},'saves'),
    nativeScheduledBackup:enabled=>tracked('native_scheduled_backup',{enabled},'saves'),
    nativeOpenBackupFolder:destination=>tracked('native_open_backup_folder',{destination},'reads'),
    nativeRestorePending:()=>tracked('native_restore_pending',{},'reads'),
    chooseRestoreBackupPath:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Проверить резервную копию SQLite',filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    chooseRestoreDestination:async defaultPath=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('save',{title:'Создать отдельную восстановленную копию',defaultPath,filters:[{name:'База SQLite',extensions:['sqlite3']}]});
    },
    nativeRestorePreview:source=>tracked('native_restore_preview',{source},'reads'),
    nativeRestoreCopy:(source,verification,destination)=>tracked(
      'native_restore_copy',{source,verification,destination},'saves'),
    nativeRestoreSchedule:(source,verification,revision)=>tracked(
      'native_restore_schedule',revision===undefined?{source,verification}:{source,verification,revision},'saves'),
    nativeRestoreLaunchInfo:()=>tracked('native_restore_launch_info',{},'reads'),
    nativeRestoreCancel:()=>tracked('native_restore_cancel',{},'saves'),
    nativeMetricKinds:()=>tracked('native_metric_kinds',{},'reads'),
    chooseMetricJson:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Выберите JSON для нового вида отчёта',filters:[{name:'JSON',extensions:['json']}]});
    },
    nativeMetricPreview:source=>tracked('native_metric_preview',{source},'reads'),
    nativeMetricApply:(source,reviewToken)=>tracked('native_metric_apply',{source,reviewToken},'saves'),
    nativeMetricSummary:(kind,start,end)=>tracked('native_metric_summary',{kind,start,end},'reads'),
    nativeMetricXlsxProfiles:()=>tracked('native_metric_xlsx_profiles',{},'reads'),
    chooseMetricXlsx:async()=>{
      if(operations.updating)throw Error('Дождитесь завершения обновления программы.');
      return nativePicker('open',{multiple:false,directory:false,title:'Выберите XLSX для нового вида отчёта',filters:[{name:'Excel XLSX',extensions:['xlsx']}]});
    },
    nativeMetricXlsxPreview:(source,profile,start,end)=>tracked(
      'native_metric_xlsx_preview',{source,profile,start,end},'reads'),
    nativeMetricXlsxApply:(source,profile,start,end,reviewToken)=>tracked(
      'native_metric_xlsx_apply',{source,profile,start,end,reviewToken},'saves'),
    nativeReportCalendar:(revision,year)=>tracked(
      'native_report_calendar',{revision,year},'reads'),
    nativeReportPage:(revision,filter,after=null,limit=100)=>tracked(
      'native_report_page',{revision,filter,after,limit},'reads'),
    save:data=>tracked('save_data',{data},'saves'),
    info:()=>invoke('get_info'),
    chooseSharedFolder:()=>nativePicker('sharedFolder'),
    chooseFolder:async()=>{
      const folder=await nativePicker('open',{directory:true,multiple:false,title:'Папка с Excel-отчётами'});
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

