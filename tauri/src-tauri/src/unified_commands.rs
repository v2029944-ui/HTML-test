//! Bounded IPC adapters for the canonical interface. Domain methods retain
//! revision checks, streaming, transaction and source-integrity ownership.
use crate::{native_advanced_analysis::AdvancedRequest, storage::*};
use std::path::Path;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn unified_period_options(
    app: AppHandle,
    revision: i64,
    report_type: String,
    grouping: Option<String>,
    after: Option<String>,
    limit: usize,
) -> Result<UnifiedPeriodOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_period_options(
            revision,
            &report_type,
            grouping.as_deref(),
            after.as_deref(),
            limit,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_name_options(
    app: AppHandle,
    revision: i64,
    search: String,
    after: Option<String>,
    limit: usize,
    page_index: Option<usize>,
) -> Result<UnifiedNameOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store=app.state::<Storage>();
        if let Some(index)=page_index {
            if after.is_some(){return Err("Do not combine a cursor and a page index".into());}
            store.unified_name_options_at_page(revision,&search,index,limit)
        }else{store.unified_name_options(revision,&search,after.as_deref(),limit)}
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_changes_page(
    app: AppHandle,
    request: AdvancedRequest,
    options: UnifiedChangeOptions,
    after: Option<String>,
    page_index: Option<usize>,
) -> Result<UnifiedChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if page_index.is_some()&&after.is_some(){return Err("Do not combine a cursor and a page index".into());}
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, grouping| {
            if let Some(index)=page_index{return store.unified_changes_page_at_page(request.revision,scope,&request.query,grouping,&options,index);}
            store.unified_changes_page(
                request.revision,
                scope,
                &request.query,
                grouping,
                &options,
                after.as_deref(),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_changes_csv(
    app: AppHandle,
    request: AdvancedRequest,
    options: UnifiedChangeOptions,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&destination);
        if destination.len() > 32767
            || !path.is_absolute()
            || !path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV file path".into());
        }
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, grouping| {
            store.unified_changes_csv(
                request.revision,
                scope,
                &request.query,
                grouping,
                &options,
                path,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_pattern_page(
    app: AppHandle,
    request: AdvancedRequest,
    mode: String,
    window_size: i32,
    options: UnifiedPatternOptions,
    after: Option<String>,
    page_index: Option<usize>,
) -> Result<PatternPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if page_index.is_some()&&after.is_some(){return Err("Do not combine a cursor and a page index".into());}
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, grouping| {
            if let Some(index)=page_index{return store.unified_pattern_page_at_page(request.revision,scope,&request.query,grouping,&mode,window_size,&options,index);}
            store.unified_pattern_page(
                request.revision,
                scope,
                &request.query,
                grouping,
                &mode,
                window_size,
                &options,
                after.as_deref(),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_comparison_page(
    app: AppHandle,
    request: AdvancedRequest,
    before: RangeQuery,
    organization: Option<String>,
    after: Option<String>,
    limit: usize,
) -> Result<Vec<ComparisonValue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !(1..=50).contains(&limit) {
            return Err("Comparison page must contain 1..=50 rows".into());
        }
        if organization
            .as_ref()
            .is_some_and(|org| !request.organizations.contains(org))
        {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let query = ComparisonQuery {
            before,
            after: RangeQuery {
                from: request.query.from.clone(),
                to: request.query.to.clone(),
            },
            percent: request.query.percent,
            min_count: request.query.min_count,
            min_amount: request.query.min_amount,
        };
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, _| {
            store.analysis_comparison_page(
                request.revision,
                scope,
                &query,
                organization.as_deref(),
                after.as_deref(),
                limit,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}


#[tauri::command]
pub async fn unified_comparison_window(
    app: AppHandle,
    request: AdvancedRequest,
    before: RangeQuery,
    organization: Option<String>,
    page_index: usize,
    limit: usize,
) -> Result<SqlPage<ComparisonValue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !(1..=50).contains(&limit) {
            return Err("Comparison page must contain 1..=50 rows".into());
        }
        if organization
            .as_ref()
            .is_some_and(|org| !request.organizations.contains(org))
        {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let query = ComparisonQuery {
            before,
            after: RangeQuery {
                from: request.query.from.clone(),
                to: request.query.to.clone(),
            },
            percent: request.query.percent,
            min_count: request.query.min_count,
            min_amount: request.query.min_amount,
        };
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, _| {
            store.analysis_comparison_window(
                request.revision,
                scope,
                &query,
                organization.as_deref(),
                page_index,
                limit,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_group_audit_page(
    app: AppHandle,
    request: AdvancedRequest,
    mode: String,
    search: String,
    offset: i64,
    limit: usize,
) -> Result<UnifiedGroupAuditPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let query = RangeQuery {
            from: request.query.from.clone(),
            to: request.query.to.clone(),
        };
        let store = app.state::<Storage>();
        request.with_scope(&store, |scope, _| {
            store.unified_group_audit_page(
                request.revision,
                scope,
                &query,
                &mode,
                &search,
                offset,
                limit,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_rename_organizations(
    app: AppHandle,
    revision: i64,
    names: Vec<String>,
    destination: String,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_rename_organizations(
            revision,
            &names,
            Path::new(&destination),
            &ManagementJob::new(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_clear_reports(
    app: AppHandle,
    revision: i64,
    destination: String,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_clear_reports(
            revision,
            Path::new(&destination),
            &ManagementJob::new(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_page(
    app: AppHandle,
    revision: i64,
    filter: ManagementReportFilter,
    after: Option<ManagementReportCursor>,
    limit: usize,
) -> Result<Vec<ManagementReport>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !(1..=50).contains(&limit) {
            return Err("Report page must contain 1..=50 rows".into());
        }
        app.state::<Storage>()
            .management_report_page(revision, &filter, after.as_ref(), limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_summary(
    app: AppHandle,
    revision: i64,
    filter: ManagementReportFilter,
) -> Result<ManagementReportSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .management_report_summary(revision, &filter)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_org_page(
    app: AppHandle,
    revision: i64,
    id: String,
    after: Option<i64>,
    limit: usize,
) -> Result<Vec<(i64, String)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !(1..=50).contains(&limit) {
            return Err("Report organization page must contain 1..=50 rows".into());
        }
        app.state::<Storage>()
            .management_report_org_page(revision, &id, after, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_quality_page(
    app: AppHandle,
    revision: i64,
    filter: UnifiedQualityFilter,
    offset: u32,
    limit: usize,
) -> Result<UnifiedQualityPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>()
            .unified_quality_page(revision, &filter, offset, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_calendar(
    app: AppHandle,
    revision: i64,
    year: i32,
) -> Result<UnifiedCalendar, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().unified_calendar(revision, year)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_quality_csv(app:AppHandle,revision:i64,filter:UnifiedQualityFilter,destination:String)->Result<(),String>{
    tauri::async_runtime::spawn_blocking(move||app.state::<Storage>().unified_quality_csv(revision,&filter,Path::new(&destination)))
        .await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn unified_group_audit_csv(app:AppHandle,request:AdvancedRequest,mode:String,search:String,destination:String)->Result<(),String>{
    tauri::async_runtime::spawn_blocking(move||{
        let store=app.state::<Storage>();
        let q=RangeQuery{from:request.query.from.clone(),to:request.query.to.clone()};
        request.with_scope(&store,|scope,_|store.unified_group_audit_csv(request.revision,scope,&q,&mode,&search,Path::new(&destination)))
    }).await.map_err(|e|e.to_string())?
}
