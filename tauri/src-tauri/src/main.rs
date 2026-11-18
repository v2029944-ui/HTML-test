#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;
mod native_import;

use native_import::{native_wide_import_preview, native_wide_import_apply};

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

#[tauri::command]
async fn native_analysis_range(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
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
            store.analysis_range_summary(
                revision, &scope, &RangeQuery { from, to },
            )
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
async fn native_analysis_organizations(
    app: tauri::AppHandle,
    revision: i64,
    report_type: String,
    from: String,
    to: String,
    organizations: Vec<String>,
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
            store.analysis_organization_page(
                revision, &scope, &RangeQuery { from, to }, None, 100,
            )
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
            store.analysis_range_page(
                revision, &scope, &RangeQuery { from, to },
                organization.as_deref(), after.as_deref(), limit,
            )
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
            store.analysis_export_range(
                revision, &scope, &RangeQuery { from, to }, organization.as_deref(), path,
            )
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
        .plugin(tauri_plugin_updater::Builder::new()
            .pubkey(option_env!("ANALIZ_UPDATER_PUBLIC_KEY").unwrap_or(""))
            .build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app.path().document_dir()?.join("Анализ работ");
            let storage = Storage::new(root)
                .map_err(std::io::Error::other)?;
            app.manage(storage);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updater_configured, load_data, save_data, native_read_revision, native_report_page,
            native_settings_bootstrap, native_group_page, native_group_rename,
            native_analysis_index_page, native_analysis_index_settings, native_analysis_range,
            native_analysis_organizations, native_analysis_range_page,
            native_export_range_csv, native_backup_sqlite, native_restore_preview, native_restore_copy,
            native_wide_import_preview, native_wide_import_apply,
            native_metric_kinds, native_metric_preview, native_metric_apply, native_metric_summary,
            native_metric_xlsx_profiles, native_metric_xlsx_preview, native_metric_xlsx_apply,
            get_info, set_folder, set_rule,
            list_excel, read_excel, check_excel, open_data_folder
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tauri application");
}

