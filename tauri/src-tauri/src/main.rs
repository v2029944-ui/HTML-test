#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;
mod native_import;
mod native_management;
mod native_name_selection;

use native_management::{native_catalog_page, native_delete_report, native_member_page, native_settings_edit};

use native_import::{
    native_wide_import_preview, native_wide_import_apply,
    native_folder_import_session, native_folder_import_preview, native_folder_import_apply,
    native_legacy_import_sheets, native_legacy_import_preview, native_legacy_import_apply,
};

use storage::{AnalysisScope, ManagementJob, RangeQuery, RangeSummary, ReadExcel, ReadFilter, ReportHeader, Rule, Storage};
use std::sync::Arc;
use tauri::{Manager, State};

// Report kind (family) is independent of day/week/month periodicity.
// Commands only exchange bounded metadata and measured totals with WebView.
#[tauri::command]
fn native_metric_kinds() -> Result<Vec<storage::MetricKind>, String> {
    storage::metric_kinds()
}

#[tauri::command]
async fn native_metric_preview(
    app: tauri::AppHandle,
    source: String,
) -> Result<storage::MetricReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().metric_preview(&source)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_metric_apply(
    app: tauri::AppHandle,
    source: String,
    review_token: String,
) -> Result<storage::MetricReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().metric_apply(&source, &review_token)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn native_metric_xlsx_profiles() -> Result<Vec<storage::MetricXlsxProfile>,String> {
    storage::metric_xlsx_profiles()
}
#[tauri::command]
async fn native_metric_xlsx_preview(
    app: tauri::AppHandle, source: String, profile: String, start: String, end: String,
) -> Result<storage::MetricReview,String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().metric_xlsx_preview(&source,&profile,&start,&end)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
async fn native_metric_xlsx_apply(
    app: tauri::AppHandle, source: String, profile: String, start: String, end: String,
    review_token: String,
) -> Result<storage::MetricReceipt,String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().metric_xlsx_apply(&source,&profile,&start,&end,&review_token)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
async fn native_metric_summary(
    app: tauri::AppHandle,
    kind: String,
    start: String,
    end: String,
) -> Result<storage::MetricSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().metric_summary(&kind, &start, &end)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn load_data(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.load()
}

#[tauri::command]
fn save_data(store: State<'_, Storage>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    store.save(data)
}


#[derive(serde::Serialize, serde::Deserialize)]
struct NativeReportCursor {
    start: String,
    id: String,
}

// SQLite reads are executed away from the WebView thread. The existing D1
// backend guarantees bounded metadata pages and revision-checked keyset order.
#[tauri::command]
async fn native_read_revision(app: tauri::AppHandle) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().revision())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_report_page(
    app: tauri::AppHandle,
    revision: i64,
    filter: ReadFilter,
    after: Option<NativeReportCursor>,
    limit: usize,
) -> Result<Vec<ReportHeader>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().report_page(
            revision,
            &filter,
            after.as_ref().map(|cursor| (cursor.start.as_str(), cursor.id.as_str())),
            limit,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}


#[derive(serde::Serialize)]
struct NativeGroup {
    handle: String,
    id: String,
    name: String,
    members: i64,
}

#[tauri::command]
async fn native_settings_bootstrap(
    app: tauri::AppHandle,
) -> Result<storage::ManagementBootstrap, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        let revision = store.revision()?;
        let job = ManagementJob::new();
        // A bounded, streaming settings index, never the full reports JSON.
        store.management_index_settings(revision, &job)?;
        store.management_bootstrap(revision)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_group_page(
    app: tauri::AppHandle,
    revision: i64,
    after_handle: Option<String>,
    limit: usize,
) -> Result<Vec<NativeGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        native_management::page_limit(limit)?;
        let after = after_handle
            .as_deref()
            .map(|value| value.parse::<i64>().map_err(|_| "Invalid group cursor".to_string()))
            .transpose()?;
        let groups = app.state::<Storage>().management_group_page(revision, "", after, limit)?;
        Ok(groups.into_iter().map(|group| NativeGroup {
            handle: group.handle.to_string(),
            id: group.id,
            name: group.name,
            members: group.members,
        }).collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_group_rename(
    app: tauri::AppHandle,
    revision: i64,
    handle: String,
    name: String,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if name.trim().is_empty() || name.len() > 1024 {
            return Err("Group name must contain 1..1024 UTF-8 bytes".to_string());
        }
        let handle = handle.parse::<i64>()
            .map_err(|_| "Invalid group handle".to_string())?;
        let store = app.state::<Storage>();
        let job = Arc::new(ManagementJob::new());
        let mut editor = store.management_begin_settings(
            revision, 256 * 1024 * 1024, job,
        )?;
        editor.rename_group(handle, &name)?;
        // Settings-only transactional publication. No report write or full load.
        Ok(store.management_commit_settings(editor)?.revision)
    })
    .await
    .map_err(|error| error.to_string())?
}


#[derive(serde::Serialize)]
struct NativeAnalysisProgress {
    processed: usize,
    next: Option<NativeReportCursor>,
    done: bool,
}

#[tauri::command]
async fn native_analysis_index_page(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    after: Option<NativeReportCursor>,
    limit: usize,
) -> Result<NativeAnalysisProgress, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<NativeAnalysisProgress, String> {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if !(1..=20).contains(&limit) {
            return Err("Analysis indexing batch must contain 1..=20 reports".into());
        }
        let store = app.state::<Storage>();
        let filter = ReadFilter { report_type: Some(report_type), ..Default::default() };
        let headers = store.report_page(
            revision,
            &filter,
            after.as_ref().map(|cursor| (cursor.start.as_str(), cursor.id.as_str())),
            limit,
        )?;
        for item in &headers {
            store.analysis_index_report(&item.id, revision)?;
        }
        let done = headers.len() < limit;
        let next = if done { None } else {
            headers.last().map(|item| NativeReportCursor {
                start: item.period_start.clone(),
                id: item.id.clone(),
            })
        };
        Ok(NativeAnalysisProgress { processed: headers.len(), next, done })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_analysis_index_settings(
    app: tauri::AppHandle,
    revision: i64,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().analysis_index_settings(revision)
    })
    .await
    .map_err(|error| error.to_string())?
}

// The existing SQL analysis API already supports exact group/work display
// name filters. Native callers create a short-lived sealed SQLite selection,
// never a WebView-side scan of historic report JSON. Cleanup also runs after
// an error, so stale names cannot leak into subsequent queries.
fn with_selected_work_names<T>(
    store: &Storage,
    revision: i64,
    name: Option<String>,
    names: Option<Vec<String>>,
    mut scope: AnalysisScope,
    action: impl FnOnce(&AnalysisScope) -> Result<T, String>,
) -> Result<T, String> {
    let names = native_name_selection::normalize(name, names)?;
    if names.is_empty() {
        return action(&scope);
    }
    let id=store.analysis_filter_create(revision,"names")?;
    let result=(|| {
        store.analysis_filter_append(revision,&id,&names)?;
        store.analysis_filter_seal(revision,&id)?;
        scope.names=Some(id.clone());
        action(&scope)
    })();
    let cleanup=store.analysis_filter_delete(&id);
    match (result,cleanup) {
        (Err(error),_)=>Err(error),
        (Ok(value),Ok(()))=>Ok(value),
        (Ok(_),Err(error))=>Err(error),
    }
}


#[tauri::command]
async fn native_analysis_name_options(
    app: tauri::AppHandle,
    revision: i64,
    search: String,
    limit: usize,
) -> Result<storage::AnalysisNameOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().analysis_name_options(revision,&search,limit)
    })
    .await
    .map_err(|error|error.to_string())?
}


#[tauri::command]
async fn native_analysis_range(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<RangeSummary, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<RangeSummary, String> {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        // A request spanning thousands of years would create millions of SQL
        // period rows. Explicitly bound native UI work; D2 still validates
        // period alignment and the underlying report completeness.
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        // A temporary on-disk exact-match selection; no saved filter presets
        // or unbounded string arrays in report JSON.
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_range_summary(
                revision, scope, &RangeQuery { from, to },
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(summary), Ok(())) => Ok(summary),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_analysis_timeline(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
    grouping: Option<String>,
) -> Result<storage::TimelineSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_timeline_grouped(revision, scope, &RangeQuery { from, to },grouping.as_deref())
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(summary), Ok(())) => Ok(summary),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_analysis_organization_trends(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
    grouping: Option<String>,
) -> Result<storage::OrganizationTrends, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_organization_trends_grouped(
                revision, scope, &RangeQuery { from, to },grouping.as_deref(),
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(summary), Ok(())) => Ok(summary),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_daily_rollup(
    app: tauri::AppHandle,
    revision: i64,
    from: String,
    to: String,
    granularity: String,
    organizations: Vec<String>,
    after: Option<String>,
    limit: usize,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<storage::DailyRollupPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if organizations.is_empty() || organizations.len()>100 {
            return Err("Select 1..=100 organizations for daily rollups".into());
        }
        if !matches!(granularity.as_str(),"day"|"week"|"month"|"year") {
            return Err("Unknown calendar period type".into());
        }
        if !(1..=60).contains(&limit) {
            return Err("Calendar page size must be 1..=60".into());
        }
        let first=chrono::NaiveDate::parse_from_str(&from,"%Y-%m-%d")
            .map_err(|_| "Invalid calendar start date".to_string())?;
        let last=chrono::NaiveDate::parse_from_str(&to,"%Y-%m-%d")
            .map_err(|_| "Invalid calendar end date".to_string())?;
        if last<first || (last-first).num_days()>3660 {
            return Err("Calendar date range must not exceed ten years".into());
        }
        let store=app.state::<Storage>();
        let filter=store.analysis_filter_create(revision,"organizations")?;
        let result=(|| {
            store.analysis_filter_append(revision,&filter,&organizations)?;
            store.analysis_filter_seal(revision,&filter)?;
            let scope=AnalysisScope{
                report_type:"day".into(),
                organizations:filter.clone(),
                names:None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_daily_rollup(
                revision, scope,
                &RangeQuery{from,to},&granularity,after.as_deref(),limit,
            )
            })
        })();
        let cleanup=store.analysis_filter_delete(&filter);
        match (result,cleanup) {
            (Err(error),_)=>Err(error),
            (Ok(page),Ok(()))=>Ok(page),
            (Ok(_),Err(error))=>Err(error),
        }
    })
    .await
    .map_err(|error|error.to_string())?
}


#[tauri::command]
async fn native_analysis_compare(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    before_from: String,
    before_to: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<storage::ComparisonDashboard, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len()>100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        for (start,end) in [(&before_from,&before_to),(&from,&to)] {
            let first=chrono::NaiveDate::parse_from_str(start,"%Y-%m-%d")
                .map_err(|_| "Invalid comparison start date".to_string())?;
            let last=chrono::NaiveDate::parse_from_str(end,"%Y-%m-%d")
                .map_err(|_| "Invalid comparison end date".to_string())?;
            if last<first || (last-first).num_days()>3660 {
                return Err("Comparison range must not exceed ten years".into());
            }
        }
        let store=app.state::<Storage>();
        let filter=store.analysis_filter_create(revision,"organizations")?;
        let result=(|| {
            store.analysis_filter_append(revision,&filter,&organizations)?;
            store.analysis_filter_seal(revision,&filter)?;
            let scope=AnalysisScope{report_type,organizations:filter.clone(),names:None};
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_comparison_dashboard(
                revision, scope,
                &storage::ComparisonQuery{
                    before:RangeQuery{from:before_from,to:before_to},
                    after:RangeQuery{from,to},
                    percent:0.,min_count:0.,min_amount:0.,
                },
            )
            })
        })();
        let cleanup=store.analysis_filter_delete(&filter);
        match (result,cleanup) {
            (Err(error),_)=>Err(error),
            (Ok(response),Ok(()))=>Ok(response),
            (Ok(_),Err(error))=>Err(error),
        }
    })
    .await
    .map_err(|error|error.to_string())?
}


#[tauri::command]
async fn native_analysis_work_timeline(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    organization: Option<String>,
    key: String,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
    grouping: Option<String>,
) -> Result<storage::WorkTrendSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        if organization.as_deref().is_some_and(|org| !organizations.iter().any(|o| o==org)) {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last-first).num_days()>3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result=(|| {
            store.analysis_filter_append(revision,&id,&organizations)?;
            store.analysis_filter_seal(revision,&id)?;
            let scope=AnalysisScope {
                report_type,
                organizations:id.clone(),
                names:None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_work_timeline_grouped(
                revision,scope,&RangeQuery{from,to},organization.as_deref(),&key,grouping.as_deref(),
            )
            })
        })();
        let cleanup=store.analysis_filter_delete(&id);
        match (result,cleanup) {
            (Err(error),_)=>Err(error),
            (Ok(rows),Ok(()))=>Ok(rows),
            (Ok(_),Err(error))=>Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_analysis_ranking(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    organization: Option<String>,
    metric: String,
    category: String,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<storage::RankingSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        if organization.as_deref().is_some_and(|o| !organizations.iter().any(|name| name == o)) {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_ranking(
                revision, scope, &RangeQuery { from, to },
                organization.as_deref(), &metric, &category,
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(rows), Ok(())) => Ok(rows),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_analysis_organizations(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<Vec<storage::OrganizationSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<storage::OrganizationSummary>, String> {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let filter_id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &filter_id, &organizations)?;
            store.analysis_filter_seal(revision, &filter_id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: filter_id.clone(),
                names: None,
            };
            // The exact selection contains at most 100 organizations.
            // SQL aggregation never materializes historical report JSON in JS.
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_organization_page(
                revision, scope, &RangeQuery { from, to }, None, 100,
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&filter_id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(rows), Ok(())) => Ok(rows),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_analysis_range_page(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    organization: Option<String>,
    after: Option<String>,
    limit: usize,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<Vec<storage::RangeValue>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<storage::RangeValue>, String> {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for analysis".into());
        }
        if !(1..=50).contains(&limit) {
            return Err("Detailed analysis page must contain 1..=50 rows".into());
        }
        if organization.as_deref().is_some_and(|org| !organizations.iter().any(|x| x == org)) {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid analysis end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Analysis range must not exceed ten years".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_range_page(
                revision, scope, &RangeQuery { from, to },
                organization.as_deref(), after.as_deref(), limit,
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(rows), Ok(())) => Ok(rows),
            (Ok(_), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}


#[tauri::command]
async fn native_export_range_csv(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
    organization: Option<String>,
    destination: String,
    selected_name: Option<String>,
    selected_names: Option<Vec<String>>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        if !matches!(report_type.as_str(), "day" | "week" | "month") {
            return Err("Unknown analysis report type".into());
        }
        if organizations.is_empty() || organizations.len() > 100 {
            return Err("Select 1..=100 organizations for export".into());
        }
        if organization.as_deref().is_some_and(|org| !organizations.iter().any(|x| x == org)) {
            return Err("Selected organization is outside the export scope".into());
        }
        let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
            .map_err(|_| "Invalid export start date".to_string())?;
        let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
            .map_err(|_| "Invalid export end date".to_string())?;
        if last < first || (last - first).num_days() > 3660 {
            return Err("Export range must not exceed ten years".into());
        }
        // The file path is supplied by the explicit user-controlled Save dialog.
        // The D2 writer refuses existing files and uses a temporary file plus
        // an atomic hard link, so neither overwrite nor half-written CSV is possible.
        if destination.len() > 32767 {
            return Err("Export destination is too long".into());
        }
        let path = std::path::Path::new(&destination);
        if !path.is_absolute() || !path.extension().and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("csv")) {
            return Err("Choose an absolute path ending in .csv".into());
        }
        let store = app.state::<Storage>();
        let id = store.analysis_filter_create(revision, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(revision, &id, &organizations)?;
            store.analysis_filter_seal(revision, &id)?;
            let scope = AnalysisScope {
                report_type,
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(&*store,revision,selected_name,selected_names,scope,|scope| {
                store.analysis_export_range(
                revision, scope, &RangeQuery { from, to }, organization.as_deref(), path,
            )
            })
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => Err(error),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_backup_sqlite(
    app: tauri::AppHandle,
    destination: String,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if destination.len()>32767 {
            return Err("Backup destination is too long".to_string());
        }
        app.state::<Storage>().backup_sqlite_verified(std::path::Path::new(&destination))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_restore_preview(
    app: tauri::AppHandle, source: String
) -> Result<storage::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source.len() > 32767 { return Err("Слишком длинный путь к копии.".into()); }
        app.state::<Storage>().restore_preview(std::path::Path::new(&source))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_restore_copy(
    app: tauri::AppHandle, source: String, verification: String, destination: String
) -> Result<storage::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source.len() > 32767 || destination.len() > 32767 {
            return Err("Слишком длинный путь к файлу.".into());
        }
        app.state::<Storage>().restore_verified_copy(
            std::path::Path::new(&source), &verification, std::path::Path::new(&destination)
        )
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_restore_schedule(
    app: tauri::AppHandle, source: String, verification: String
) -> Result<storage::ScheduledRestore, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source.len() > 32767 {
            return Err("Слишком длинный путь к резервной копии.".into());
        }
        app.state::<Storage>().schedule_restore(
            std::path::Path::new(&source), &verification
        )
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn native_restore_launch_info(
    state: State<'_, storage::RestoreLaunchInfo>
) -> Option<storage::RestoreLaunchResult> {
    state.0.clone()
}

#[tauri::command]
async fn native_restore_cancel(app: tauri::AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().cancel_scheduled_restore()
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_info(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.info()
}

#[tauri::command]
fn set_folder(store: State<'_, Storage>, folder: String) -> Result<serde_json::Value, String> {
    store.set_folder(folder)
}

#[tauri::command]
fn set_rule(store: State<'_, Storage>, name: String, rule: Rule) -> Result<Rule, String> {
    store.set_rule(name, rule)
}

#[tauri::command]
async fn native_set_rule_checked(
    app: tauri::AppHandle,
    name: String,
    rule: Rule,
    revision: i64,
    folder: String,
    signature: String,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Storage>().set_rule_checked(name, rule, revision, folder, signature)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn list_excel(store: State<'_, Storage>) -> Result<serde_json::Value, String> {
    store.list_excel()
}

#[tauri::command]
fn read_excel(store: State<'_, Storage>, name: String) -> Result<ReadExcel, String> {
    store.read_excel(name)
}

#[tauri::command]
fn check_excel(store: State<'_, Storage>, name: String, signature: String) -> bool {
    store.check_excel(name, signature)
}

#[tauri::command]
fn open_data_folder(store: State<'_, Storage>) -> Result<String, String> {
    tauri_plugin_opener::open_path(store.root(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(String::new())
}

#[tauri::command]
fn updater_configured() -> bool {
    !option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or("").trim().is_empty()
}

fn main() {
    tauri::Builder::default()
        .on_window_event(|window,event| {
            if matches!(event,tauri::WindowEvent::Destroyed) {
                if let Some(store)=window.try_state::<Storage>() {
                    let _=store.set_folder_import_session(window.label(),false);
                }
            }
        })
        .plugin(tauri_plugin_updater::Builder::new()
            .pubkey(option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or(""))
            .build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app.path().document_dir()?.join("Анализ работ");
            // Hold one exclusive lock for the entire application lifetime.
            // Offline recovery runs before ANY native SQLite connection opens.
            let lease = storage::AppRestoreLock::acquire(&root)
                .map_err(std::io::Error::other)?;
            let restored = storage::apply_pending_restore_or_open_original(&root)
                .map_err(std::io::Error::other)?;
            let storage = Storage::new(root)
                .map_err(std::io::Error::other)?;
            app.manage(lease);
            app.manage(storage);
            app.manage(storage::RestoreLaunchInfo(restored));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updater_configured, load_data, save_data, native_read_revision, native_report_page,
            native_settings_bootstrap, native_group_page, native_group_rename,
            native_catalog_page, native_delete_report, native_member_page, native_settings_edit,
            native_analysis_index_page, native_analysis_index_settings, native_analysis_name_options, native_analysis_range,
            native_analysis_timeline, native_analysis_ranking, native_analysis_organization_trends, native_analysis_work_timeline, native_analysis_compare, native_daily_rollup,
            native_analysis_organizations, native_analysis_range_page,
            native_export_range_csv, native_backup_sqlite, native_restore_preview, native_restore_copy,
            native_restore_schedule, native_restore_launch_info, native_restore_cancel,
            native_folder_import_session,
            native_folder_import_preview,
            native_folder_import_apply,
            native_wide_import_preview, native_wide_import_apply,
            native_legacy_import_sheets, native_legacy_import_preview, native_legacy_import_apply,
            native_metric_kinds, native_metric_preview, native_metric_apply, native_metric_summary,
            native_metric_xlsx_profiles, native_metric_xlsx_preview, native_metric_xlsx_apply,
            get_info, set_folder, set_rule, native_set_rule_checked,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

