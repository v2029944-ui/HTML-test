#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_taskbar;
mod storage;
mod native_shared_runtime;
use native_shared_runtime::{shared_workspace_status,shared_workspace_choose_folder,shared_workspace_connect,shared_workspace_sync,shared_workspace_disconnect,shared_workspace_discard_pending};
mod native_import;
mod native_management;
mod native_advanced_analysis;
use native_advanced_analysis::{native_changes_page,native_changes_csv,native_pattern_page,native_composition_page};
mod native_reference_trend_csv;
use native_reference_trend_csv::reference_trend_csv;
mod native_reference_detail_csv;
use native_reference_detail_csv::reference_detail_csv;
mod native_report_quality;
mod native_coverage;
mod native_pagination;
use native_pagination::{unified_report_window, unified_report_org_window, native_quality_matrix_reports_at_page};
mod native_reference_changes;
use native_reference_changes::{reference_changes,reference_changes_csv,reference_summary_csv};
use native_coverage::{native_quality_matrix_page,native_quality_matrix_reports};
mod native_name_selection;
mod unified_commands;
mod unified_management;
use unified_commands::*;
use unified_management::*;

use native_report_quality::native_report_quality_page;
use native_management::{native_catalog_page, native_delete_report, native_member_page, native_settings_edit};

use native_import::{
    native_wide_import_preview, native_wide_import_apply, native_preview_quality_page, native_preview_quality_csv,
    native_folder_import_session, native_folder_import_preview, native_folder_import_apply,
    native_import_sheets, native_legacy_import_sheets, native_legacy_import_preview, native_legacy_import_apply,
};

use storage::{AnalysisScope, ManagementJob, RangeQuery, RangeSummary, ReadExcel, ReadFilter, ReportHeader, Rule, Storage};
use std::sync::Arc;
use tauri::{Manager, State};

#[tauri::command]
async fn native_report_calendar(
    app: tauri::AppHandle,
    revision: i64,
    year: i32,
    workspace_token: Option<String>,
) -> Result<storage::ReportCalendar, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.report_calendar(revision, year),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

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
    workspace_token: Option<String>,
) -> Result<storage::MetricReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.metric_preview(&source),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_metric_apply(
    app: tauri::AppHandle,
    source: String,
    review_token: String,
    workspace_token: Option<String>,
) -> Result<storage::MetricReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| routed_storage.metric_apply(&source, &review_token),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn native_metric_xlsx_profiles() -> Result<Vec<storage::MetricXlsxProfile>,String> {
    storage::metric_xlsx_profiles()
}
#[tauri::command]
async fn native_metric_xlsx_preview(
    app: tauri::AppHandle,
    source: String,
    profile: String,
    start: String,
    end: String,
    workspace_token: Option<String>,
) -> Result<storage::MetricReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.metric_xlsx_preview(&source, &profile, &start, &end),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn native_metric_xlsx_apply(
    app: tauri::AppHandle,
    source: String,
    profile: String,
    start: String,
    end: String,
    review_token: String,
    workspace_token: Option<String>,
) -> Result<storage::MetricReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.metric_xlsx_apply(&source, &profile, &start, &end, &review_token)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_metric_summary(
    app: tauri::AppHandle,
    kind: String,
    start: String,
    end: String,
    workspace_token: Option<String>,
) -> Result<storage::MetricSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.metric_summary(&kind, &start, &end),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn load_data(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| store.load(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_data(
    app: tauri::AppHandle,
    data: serde_json::Value,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Write,
            |store| store.save(data),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}


#[derive(serde::Serialize, serde::Deserialize)]
struct NativeReportCursor {
    start: String,
    id: String,
}

// SQLite reads are executed away from the WebView thread. The existing D1
// backend guarantees bounded metadata pages and revision-checked keyset order.
#[tauri::command]
async fn native_read_revision(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.revision(),
        )
    })
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
    workspace_token: Option<String>,
) -> Result<Vec<ReportHeader>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.report_page(
                    revision,
                    &filter,
                    after
                        .as_ref()
                        .map(|cursor| (cursor.start.as_str(), cursor.id.as_str())),
                    limit,
                )
            },
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
    workspace_token: Option<String>,
) -> Result<storage::ManagementBootstrap, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                let revision = store.revision()?;
                let job = ManagementJob::new();
                // A bounded, streaming settings index, never the full reports JSON.
                store.management_index_settings(revision, &job)?;
                store.management_bootstrap(revision)
            },
        )
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
    workspace_token: Option<String>,
) -> Result<Vec<NativeGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                native_management::page_limit(limit)?;
                let after = after_handle
                    .as_deref()
                    .map(|value| {
                        value
                            .parse::<i64>()
                            .map_err(|_| "Invalid group cursor".to_string())
                    })
                    .transpose()?;
                let groups = routed_storage.management_group_page(revision, "", after, limit)?;
                Ok(groups
                    .into_iter()
                    .map(|group| NativeGroup {
                        handle: group.handle.to_string(),
                        id: group.id,
                        name: group.name,
                        members: group.members,
                    })
                    .collect())
            },
        )
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
    workspace_token: Option<String>,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                if name.trim().is_empty() || name.len() > 1024 {
                    return Err("Group name must contain 1..1024 UTF-8 bytes".to_string());
                }
                let handle = handle
                    .parse::<i64>()
                    .map_err(|_| "Invalid group handle".to_string())?;
                let store = routed_storage;
                let job = Arc::new(ManagementJob::new());
                let mut editor =
                    store.management_begin_settings(revision, 256 * 1024 * 1024, job)?;
                editor.rename_group(handle, &name)?;
                // Settings-only transactional publication. No report write or full load.
                Ok(store.management_commit_settings(editor)?.revision)
            },
        )
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
    workspace_token: Option<String>,
) -> Result<NativeAnalysisProgress, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if !(1..=20).contains(&limit) {
                    return Err("Analysis indexing batch must contain 1..=20 reports".into());
                }
                let store = routed_storage;
                let filter = ReadFilter {
                    report_type: Some(report_type),
                    ..Default::default()
                };
                let headers = store.report_page(
                    revision,
                    &filter,
                    after
                        .as_ref()
                        .map(|cursor| (cursor.start.as_str(), cursor.id.as_str())),
                    limit,
                )?;
                for item in &headers {
                    store.index_unified_startup_report(item, revision, &ManagementJob::new())?;
                }
                let done = headers.len() < limit;
                let next = if done {
                    None
                } else {
                    headers.last().map(|item| NativeReportCursor {
                        start: item.period_start.clone(),
                        id: item.id.clone(),
                    })
                };
                Ok(NativeAnalysisProgress {
                    processed: headers.len(),
                    next,
                    done,
                })
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_analysis_index_settings(
    app: tauri::AppHandle,
    revision: i64,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.analysis_index_settings(revision),
        )
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
    workspace_token: Option<String>,
) -> Result<storage::AnalysisNameOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.analysis_name_options(revision, &search, limit),
        )
    })
    .await
    .map_err(|error| error.to_string())?
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
    workspace_token: Option<String>,
) -> Result<RangeSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
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
                let store = routed_storage;
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
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_range_summary(revision, scope, &RangeQuery { from, to })
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(summary), Ok(())) => Ok(summary),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<storage::TimelineSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
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
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_timeline_grouped(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                grouping.as_deref(),
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(summary), Ok(())) => Ok(summary),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<storage::OrganizationTrends, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
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
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_organization_trends_grouped(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                grouping.as_deref(),
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(summary), Ok(())) => Ok(summary),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<storage::DailyRollupPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for daily rollups".into());
                }
                if !matches!(granularity.as_str(), "day" | "week" | "month" | "year") {
                    return Err("Unknown calendar period type".into());
                }
                if !(1..=60).contains(&limit) {
                    return Err("Calendar page size must be 1..=60".into());
                }
                let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
                    .map_err(|_| "Invalid calendar start date".to_string())?;
                let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
                    .map_err(|_| "Invalid calendar end date".to_string())?;
                if last < first || (last - first).num_days() > 3660 {
                    return Err("Calendar date range must not exceed ten years".into());
                }
                let store = routed_storage;
                let filter = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &filter, &organizations)?;
                    store.analysis_filter_seal(revision, &filter)?;
                    let scope = AnalysisScope {
                        report_type: "day".into(),
                        organizations: filter.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_daily_rollup(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                &granularity,
                                after.as_deref(),
                                limit,
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&filter);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(page), Ok(())) => Ok(page),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
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
    workspace_token: Option<String>,
) -> Result<storage::ComparisonDashboard, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for analysis".into());
                }
                for (start, end) in [(&before_from, &before_to), (&from, &to)] {
                    let first = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d")
                        .map_err(|_| "Invalid comparison start date".to_string())?;
                    let last = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d")
                        .map_err(|_| "Invalid comparison end date".to_string())?;
                    if last < first || (last - first).num_days() > 3660 {
                        return Err("Comparison range must not exceed ten years".into());
                    }
                }
                let store = routed_storage;
                let filter = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &filter, &organizations)?;
                    store.analysis_filter_seal(revision, &filter)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: filter.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_comparison_dashboard(
                                revision,
                                scope,
                                &storage::ComparisonQuery {
                                    before: RangeQuery {
                                        from: before_from,
                                        to: before_to,
                                    },
                                    after: RangeQuery { from, to },
                                    percent: 0.,
                                    min_count: 0.,
                                    min_amount: 0.,
                                },
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&filter);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(response), Ok(())) => Ok(response),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
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
    workspace_token: Option<String>,
) -> Result<storage::WorkTrendSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for analysis".into());
                }
                if organization
                    .as_deref()
                    .is_some_and(|org| !organizations.iter().any(|o| o == org))
                {
                    return Err("Selected organization is outside the analysis scope".into());
                }
                let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis start date".to_string())?;
                let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis end date".to_string())?;
                if last < first || (last - first).num_days() > 3660 {
                    return Err("Analysis range must not exceed ten years".into());
                }
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_work_timeline_grouped(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                organization.as_deref(),
                                &key,
                                grouping.as_deref(),
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(rows), Ok(())) => Ok(rows),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<storage::RankingSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for analysis".into());
                }
                if organization
                    .as_deref()
                    .is_some_and(|o| !organizations.iter().any(|name| name == o))
                {
                    return Err("Selected organization is outside the analysis scope".into());
                }
                let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis start date".to_string())?;
                let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis end date".to_string())?;
                if last < first || (last - first).num_days() > 3660 {
                    return Err("Analysis range must not exceed ten years".into());
                }
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_ranking(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                organization.as_deref(),
                                &metric,
                                &category,
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(rows), Ok(())) => Ok(rows),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<Vec<storage::OrganizationSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
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
                let store = routed_storage;
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
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_organization_page(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                None,
                                100,
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&filter_id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(rows), Ok(())) => Ok(rows),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<Vec<storage::RangeValue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for analysis".into());
                }
                if !(1..=50).contains(&limit) {
                    return Err("Detailed analysis page must contain 1..=50 rows".into());
                }
                if organization
                    .as_deref()
                    .is_some_and(|org| !organizations.iter().any(|x| x == org))
                {
                    return Err("Selected organization is outside the analysis scope".into());
                }
                let first = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis start date".to_string())?;
                let last = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
                    .map_err(|_| "Invalid analysis end date".to_string())?;
                if last < first || (last - first).num_days() > 3660 {
                    return Err("Analysis range must not exceed ten years".into());
                }
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_range_page(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                organization.as_deref(),
                                after.as_deref(),
                                limit,
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(rows), Ok(())) => Ok(rows),
                    (Ok(_), Err(error)) => Err(error),
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !matches!(report_type.as_str(), "day" | "week" | "month") {
                    return Err("Unknown analysis report type".into());
                }
                if organizations.is_empty() || organizations.len() > 100 {
                    return Err("Select 1..=100 organizations for export".into());
                }
                if organization
                    .as_deref()
                    .is_some_and(|org| !organizations.iter().any(|x| x == org))
                {
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
                if !path.is_absolute()
                    || !path
                        .extension()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.eq_ignore_ascii_case("csv"))
                {
                    return Err("Choose an absolute path ending in .csv".into());
                }
                let store = routed_storage;
                let id = store.analysis_filter_create(revision, "organizations")?;
                let result = (|| {
                    store.analysis_filter_append(revision, &id, &organizations)?;
                    store.analysis_filter_seal(revision, &id)?;
                    let scope = AnalysisScope {
                        report_type,
                        organizations: id.clone(),
                        names: None,
                    };
                    with_selected_work_names(
                        &*store,
                        revision,
                        selected_name,
                        selected_names,
                        scope,
                        |scope| {
                            store.analysis_export_range(
                                revision,
                                scope,
                                &RangeQuery { from, to },
                                organization.as_deref(),
                                path,
                            )
                        },
                    )
                })();
                let cleanup = store.analysis_filter_delete(&id);
                match (result, cleanup) {
                    (Err(error), _) => Err(error),
                    (Ok(()), Ok(())) => Ok(()),
                    (Ok(()), Err(error)) => Err(error),
                }
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_backup_sqlite(
    app: tauri::AppHandle,
    destination: String,
    workspace_token: Option<String>,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if destination.len() > 32767 {
                    return Err("Backup destination is too long".to_string());
                }
                routed_storage.backup_sqlite_verified(std::path::Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_restore_preview(
    app: tauri::AppHandle,
    source: String,
    workspace_token: Option<String>,
) -> Result<storage::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if source.len() > 32767 {
                    return Err("Слишком длинный путь к копии.".into());
                }
                routed_storage.restore_preview(std::path::Path::new(&source))
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_restore_copy(
    app: tauri::AppHandle,
    source: String,
    verification: String,
    destination: String,
    workspace_token: Option<String>,
) -> Result<storage::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if source.len() > 32767 || destination.len() > 32767 {
                    return Err("Слишком длинный путь к файлу.".into());
                }
                routed_storage.restore_verified_copy(
                    std::path::Path::new(&source),
                    &verification,
                    std::path::Path::new(&destination),
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn native_restore_schedule(
    app: tauri::AppHandle,
    source: String,
    verification: String,
    workspace_token: Option<String>,
) -> Result<storage::ScheduledRestore, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::PersonalOnly,
            |routed_storage| {
                if source.len() > 32767 {
                    return Err("Слишком длинный путь к резервной копии.".into());
                }
                routed_storage.schedule_restore(std::path::Path::new(&source), &verification)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn native_restore_launch_info(
    state: State<'_, storage::RestoreLaunchInfo>
) -> Option<storage::RestoreLaunchResult> {
    state.0.clone()
}

#[tauri::command]
async fn native_restore_cancel(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::PersonalOnly,
            |routed_storage| routed_storage.cancel_scheduled_restore(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_info(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| store.info(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_folder(
    app: tauri::AppHandle,
    folder: String,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Local,
            |store| store.set_folder(folder),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_rule(
    app: tauri::AppHandle,
    name: String,
    rule: Rule,
    workspace_token: Option<String>,
) -> Result<Rule, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Local,
            |store| store.set_rule(name, rule),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_set_rule_checked(
    app: tauri::AppHandle,
    name: String,
    rule: Rule,
    revision: i64,
    folder: String,
    signature: String,
    workspace_token: Option<String>,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Local,
            |routed_storage| {
                routed_storage.set_rule_checked(name, rule, revision, folder, signature)
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn native_import_folder_context(
    app: tauri::AppHandle,
    source: String,
    workspace_token: Option<String>,
) -> Result<Option<serde_json::Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(&app, workspace_token.as_deref(),
            storage::shared_runtime::Access::Read, |store| store.import_folder_context(std::path::Path::new(&source)))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_excel(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| store.list_excel(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn read_excel(
    app: tauri::AppHandle,
    name: String,
    workspace_token: Option<String>,
) -> Result<ReadExcel, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| store.read_excel(name),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn check_excel(
    app: tauri::AppHandle,
    name: String,
    signature: String,
    workspace_token: Option<String>,
) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| Ok(store.check_excel(name, signature)),
        )
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or(false)
}

#[tauri::command]
async fn open_data_folder(
    app: tauri::AppHandle,
    workspace_token: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            storage::shared_runtime::Access::Read,
            |store| {
                tauri_plugin_opener::open_path(store.root(), None::<&str>)
                    .map_err(|e| e.to_string())?;
                Ok(String::new())
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn updater_configured() -> bool {
    !option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or("").trim().is_empty()
}

#[cfg(feature="ci-webview2")]
fn smoke_configuration() -> Result<Option<(std::path::PathBuf, u16)>, String> {
    let Some(root) = std::env::var_os("ANALIZ_SMOKE_DATA_ROOT") else {
        return Ok(None);
    };
    let root = std::path::PathBuf::from(root);
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Smoke data root must be an absolute local fixture directory".into());
    }
    #[cfg(windows)]
    if !matches!(root.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("Smoke data root must use a local disk path, not UNC".into());
    }
    let port = std::env::var("ANALIZ_SMOKE_CDP_PORT")
        .unwrap_or_else(|_| "9227".into())
        .parse::<u16>()
        .map_err(|_| "Invalid smoke CDP port")?;
    if port == 0 {
        return Err("Invalid smoke CDP port".into());
    }
    Ok(Some((root, port)))
}

fn main() {
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(windows)]
    if let Err(error) = windows_taskbar::set_process_identity(&context.config().identifier) {
        eprintln!("Unable to set Windows application identity: {error}");
    }
    // Build the window hidden and set its shell properties before Explorer groups it.
    #[cfg(windows)]
    let startup_windows: Vec<_> = context.config_mut().app.windows.iter_mut()
        .filter(|window| window.create)
        .map(|window| {
            let config = window.clone();
            window.create = false;
            config
        })
        .collect();
    #[cfg(feature = "ci-webview2")]
    let smoke = smoke_configuration().expect("Invalid CI-only smoke configuration");
    #[cfg(all(feature = "ci-webview2", not(windows)))]
    if smoke.is_some() {
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
    }
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.on_page_load(|webview, page| {
        // Apply ICON_BIG to the window that actually finished loading, including cold WebView2 starts.
        if matches!(page.event(), tauri::webview::PageLoadEvent::Finished) {
            let icon = webview.window().hwnd()
                .map_err(|error| std::io::Error::other(error.to_string()))
                .and_then(|hwnd| windows_taskbar::set_large_icon(hwnd).map_err(std::io::Error::other));
            if let Err(error) = icon {
                eprintln!("Unable to set the Windows taskbar icon: {error}");
            }
        }
    });
    builder
        .on_window_event(|window, event| {
            #[cfg(windows)]
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                if let Ok(hwnd) = window.hwnd() {
                    if let Err(error) = windows_taskbar::clear_relaunch(hwnd) {
                        eprintln!("Unable to clear Windows relaunch metadata: {error}");
                    }
                }
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(runtime) = window.try_state::<storage::shared_runtime::Runtime>() {
                    runtime.close_window(window.label());
                }
            }
        })
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or(""))
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let root = app.path().document_dir()?.join("Анализ работ");
            #[cfg(feature = "ci-webview2")]
            let root = smoke.as_ref().map(|(root, _)| root.clone()).unwrap_or(root);
            #[cfg(feature = "ci-webview2")]
            if smoke.is_some() {
                std::fs::create_dir_all(&root)?;
                storage::shared_runtime::check_local_cache(&root).map_err(std::io::Error::other)?;
            }
            // Hold one exclusive lock for the entire application lifetime.
            // Offline recovery runs before ANY native SQLite connection opens.
            let lease = storage::AppRestoreLock::acquire(&root).map_err(std::io::Error::other)?;
            let restored = storage::apply_pending_restore_or_open_original(&root)
                .map_err(std::io::Error::other)?;
            let private_root = app.path().app_local_data_dir()?.join("shared-runtime");
            #[cfg(feature = "ci-webview2")]
            let private_root = smoke
                .as_ref()
                .map(|(root, _)| root.join("shared-runtime"))
                .unwrap_or(private_root);
            let runtime = storage::shared_runtime::Runtime::with_cache(root.clone(), private_root)
                .map_err(std::io::Error::other)?;
            app.manage(lease);
            app.manage(runtime);
            app.manage(storage::RestoreLaunchInfo(restored));
            #[cfg(any(windows, feature = "ci-webview2"))]
            {
                #[cfg(not(windows))]
                let startup_windows = if smoke.is_some() {
                    app.config().app.windows.clone()
                } else {
                    Vec::new()
                };
                for config in &startup_windows {
                    let builder = tauri::WebviewWindowBuilder::from_config(app, config)?;
                    #[cfg(windows)]
                    let builder = builder.visible(false);
                    #[cfg(feature = "ci-webview2")]
                    let builder = if let Some((_, port)) = &smoke {
                        builder.data_directory(root.join(format!("webview-profile-{port}")))
                            .additional_browser_args(&format!(
                                "--remote-debugging-address=127.0.0.1 --remote-debugging-port={port}"
                            ))
                    } else {
                        builder
                    };
                    let _window = builder.build()?;
                    #[cfg(windows)]
                    {
                        match _window.hwnd() {
                            Ok(hwnd) => {
                                if let Err(error) = windows_taskbar::set_large_icon(hwnd) {
                                    eprintln!("Unable to set the Windows taskbar icon: {error}");
                                }
                                if let Err(error) = windows_taskbar::configure_relaunch(hwnd, &app.config().identifier) {
                                    eprintln!("Unable to set Windows relaunch metadata: {error}");
                                }
                            }
                            Err(error) => eprintln!("Unable to obtain the Windows window handle: {error}"),
                        }
                        if config.visible {
                            _window.show()?;
                            if config.focus {
                                if let Err(error) = _window.set_focus() {
                                    eprintln!("Unable to focus the startup window: {error}");
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shared_workspace_status,
            shared_workspace_choose_folder,
            shared_workspace_connect,
            shared_workspace_sync,
            shared_workspace_disconnect,
            shared_workspace_discard_pending,
            reference_changes,
            reference_changes_csv,
            reference_summary_csv,
            unified_period_options,
            unified_name_options,
            unified_changes_page,
            unified_changes_csv,
            reference_trend_csv,
            reference_detail_csv,
            unified_pattern_page,
            unified_comparison_page,
            unified_comparison_window,
            unified_group_audit_page,
            unified_rename_organizations,
            unified_clear_reports,
            unified_report_page,
            unified_report_summary,
            unified_report_org_page,
            unified_report_window,
            unified_report_org_window,
            native_quality_matrix_reports_at_page,
            unified_quality_page,
            unified_calendar,
            unified_group_audit_csv,
            unified_quality_csv,
            unified_management_page,
            unified_group_save,
            unified_work_assign,
            unified_groups_preview,
            unified_groups_import,
            unified_groups_export,
            unified_catalog_preview,
            unified_catalog_apply,
            updater_configured,
            load_data,
            save_data,
            native_read_revision,
            native_report_page,
            native_report_calendar,
            native_report_quality_page,
            native_quality_matrix_page,
            native_quality_matrix_reports,
            native_settings_bootstrap,
            native_group_page,
            native_group_rename,
            native_catalog_page,
            native_delete_report,
            native_member_page,
            native_settings_edit,
            native_analysis_index_page,
            native_analysis_index_settings,
            native_analysis_name_options,
            native_analysis_range,
            native_analysis_timeline,
            native_analysis_ranking,
            native_analysis_organization_trends,
            native_analysis_work_timeline,
            native_analysis_compare,
            native_daily_rollup,
            native_changes_page,
            native_changes_csv,
            native_pattern_page,
            native_composition_page,
            native_analysis_organizations,
            native_analysis_range_page,
            native_export_range_csv,
            native_backup_sqlite,
            native_restore_preview,
            native_restore_copy,
            native_restore_schedule,
            native_restore_launch_info,
            native_restore_cancel,
            native_folder_import_session,
            native_folder_import_preview,
            native_folder_import_apply,
            native_wide_import_preview,
            native_wide_import_apply,
            native_preview_quality_page,
            native_preview_quality_csv,
            native_import_folder_context,
            native_import_sheets,
            native_legacy_import_sheets,
            native_legacy_import_preview,
            native_legacy_import_apply,
            native_metric_kinds,
            native_metric_preview,
            native_metric_apply,
            native_metric_summary,
            native_metric_xlsx_profiles,
            native_metric_xlsx_preview,
            native_metric_xlsx_apply,
            get_info,
            set_folder,
            set_rule,
            native_set_rule_checked,
            list_excel,
            read_excel,
            check_excel,
            open_data_folder
        ])
        .run(context)
        .expect("Failed to start Tauri application");
}

