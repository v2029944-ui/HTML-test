//! Explicit two-phase, disk-backed daily-wide import for the separate SQL UI.
//! Stage objects borrow StageArea and may not cross commands. Apply repeats the
//! bounded parse, verifies the exact reviewed digest, then invokes C3's atomic
//! revision-checked publication. No report JSON enters the WebView.
use crate::storage::{ImportConfirmation, ImportCounts, ImportJob, ImportPreview, ImportReceipt, Storage};
use analiz_import_core::{LegacyOptions, Limits, PreparedImport, QualityCounts, SourceDiagnostics, StageArea, WideOptions};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::{atomic::AtomicBool, Arc}};
use tauri::Manager;

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct NativeWideReview {
    pub review_token: String,
    pub source_name: String,
    pub source_hash: String,
    pub revision: i64,
    pub report_count: u64,
    pub period_range: Option<NativePeriodRange>,
    pub row_count: u64,
    pub processed_cell_count: u64,
    pub catalog_entries: u64,
    pub conflicts: ImportCounts,
    pub quality: QualityCounts,
    pub diagnostics: SourceDiagnostics,
    // Keys combine report type and YYYY-MM. Each bit marks an observed start day.
    pub quality_period_months: std::collections::BTreeMap<String,u32>,
    pub quality_periods_partial: bool,
    pub sample_reports: Vec<NativeReportSample>,
    pub sample_report_issues: Vec<serde_json::Value>,
    pub sample_source_issues: serde_json::Value,
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct NativePeriodRange {
    pub kind: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mixed_week_starts: bool,
}
impl NativePeriodRange {
    fn include(&mut self, kind: &str, start: &str, end: &str) -> Result<(), String> {
        if self.kind != kind { return Err("Mixed reviewed report types".into()); }
        if kind == "week" && !self.mixed_week_starts {
            let weekday = |value: &str| NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map(|day| day.weekday()).map_err(|error| error.to_string());
            self.mixed_week_starts = weekday(&self.from)? != weekday(start)?;
        }
        if start < self.from.as_str() { self.from = start.to_owned(); }
        if end > self.to.as_str() { self.to = end.to_owned(); }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct NativeReportSample {
    pub id: String,
    pub kind: String,
    pub start: String,
    pub end: String,
    pub rows: u64,
    pub issues: u64,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct NativeWideConfirmation {
    replace_exact: bool,
    accept_cross_type: bool,
    accept_quality: bool,
    // The native IPC itself must require the backup destination. The UI is
    // not a security boundary: a direct invoke must not bypass this guard.
    backup_destination: Option<String>,
}
fn with_prepared<T>(
    store: &Storage,
    stage_directory: &str,
    source: &str,
    options: WideOptions,
    action: impl FnOnce(&PreparedImport<'_>, &ImportPreview, &WideOptions) -> Result<T,String>,
) -> Result<T,String> {
    if source.len()>32767 { return Err("Excel path is too long".into()); }
    let path=Path::new(source);
    if !path.is_absolute() || !path.extension().and_then(|e| e.to_str()).is_some_and(|e|e.eq_ignore_ascii_case("xlsx")) {
        return Err("Choose an absolute .xlsx file".into());
    }
    // First initialize / migrate the owner's database through the normal safe
    // path; raw import_connection is intentionally not allowed to migrate.
    store.revision()?;
    let area=StageArea::open(store.root().join(stage_directory))
        .map_err(|e|e.to_string())?;
    let cancelled=Arc::new(AtomicBool::new(false));
    let mut stage=area.ingest(path, Limits::default(), &cancelled, |_|{})
        .map_err(|e|e.to_string())?;
    stage.convert_wide(&options, cancelled.clone(), |_|{})
        .map_err(|e|e.to_string())?;
    let prepared=stage.prepare(&cancelled, |_|{})
        .map_err(|e|e.to_string())?;
    let preview=store.preview_import(&prepared)?;
    action(&prepared,&preview,&options)
}
fn review<T: serde::Serialize>(
    prepared: &PreparedImport<'_>,
    preview: &ImportPreview,
    options: &T,
    domain: &[u8],
    organizations: &[String],
) -> Result<NativeWideReview,String> {
    let catalog_entries=prepared.visit_catalog(organizations,&AtomicBool::new(false),|_,_|Ok(())).map_err(|e|e.to_string())?;
    let mut hash=Sha256::new();
    hash.update(catalog_entries.to_le_bytes());
    let identity=preview.review_identity();
    hash.update(domain);
    hash.update(serde_json::to_vec(&(
        prepared.source_hash(),prepared.source_name(),prepared.source_folder(),
        preview.revision(),preview.counts(),prepared.quality(),
        prepared.source_diagnostics(),options,identity.0,identity.1,
    )).map_err(|e|e.to_string())?);
    let mut samples=Vec::new();
    let mut report_issues=Vec::new();
    // Cap unusual workbooks while covering over ten years of daily imports.
    let mut quality_period_months=std::collections::BTreeMap::new();
    let mut quality_periods_partial=false;
    let mut after=0;
    let mut period_range:Option<NativePeriodRange>=None;
    let mut reports=0u64;
    let mut rows=0u64;
    while let Some(report)=prepared.next_report(after).map_err(|e|e.to_string())? {
        match &mut period_range {
            Some(range) => range.include(&report.kind, &report.start, &report.end)?,
            None => period_range=Some(NativePeriodRange{kind:report.kind.clone(),
                from:report.start.clone(),to:report.end.clone(),mixed_week_starts:false}),
        }
        let month=report.start.get(..7).ok_or("Invalid reviewed period start")?;
        let day=report.start.get(8..).and_then(|s|s.parse::<u32>().ok()).filter(|d|(1..=31).contains(d)).ok_or("Invalid reviewed period day")?;
        let key=format!("{}:{month}",report.kind);
        if quality_period_months.len()<128 || quality_period_months.contains_key(&key) {
            *quality_period_months.entry(key).or_insert(0u32) |= 1u32 << (day-1);
        } else { quality_periods_partial=true; }
        if samples.len()<12 {
            samples.push(NativeReportSample {
                id:report.info.id.clone(),kind:report.kind.clone(),
                start:report.start.clone(),end:report.end.clone(),
                rows:report.rows,issues:report.quality.issues,
            });
        }
        // The C3 prepared report ID and document SHA contain a random stage
        // operation ID; those are deliberately NOT part of the review token.
        // Hash stable converted metadata + source SHA instead, so a second
        // fully validated parse can match the review without replaying stage IDs.
        if report.quality.issues!=0 && report_issues.len()<12 {
            // Read a bounded sample of per-report cell issues in addition to
            // source-wide controls. The UI must identify the actual Excel cells.
            let page=prepared.issues(&report,0,12-report_issues.len())
                .map_err(|e|e.to_string())?;
            for issue in page {
                let mut value=serde_json::to_value(issue).map_err(|e|e.to_string())?;
                if let Some(map)=value.as_object_mut() {
                    map.insert("report_start".into(),serde_json::Value::String(report.start.clone()));
                    map.insert("source_report".into(),serde_json::Value::String(report.info.id.clone()));
                }
                report_issues.push(value);
            }
        }
        hash.update(serde_json::to_vec(&(
            report.ordinal,&report.info,&report.kind,&report.start,&report.end,
            report.rows,&report.quality,&report.provenance,
        )).map_err(|e|e.to_string())?);
        after=report.ordinal;
        reports+=1;
        rows=rows.checked_add(report.rows).ok_or("Import row count overflow")?;
    }
    if reports!=prepared.report_count()||rows!=prepared.row_count() {
        return Err("Incomplete native import review".into());
    }
    let issues=prepared.source_issues(0,12).map_err(|e|e.to_string())?;
    Ok(NativeWideReview {
        review_token:format!("{:x}",hash.finalize()),
        source_name:prepared.source_name().to_string(),
        source_hash:prepared.source_hash().to_string(),
        revision:preview.revision(),
        report_count:reports,period_range,row_count:rows,catalog_entries,
        processed_cell_count: prepared.processed_cell_count(),
        conflicts:preview.counts().clone(),
        quality:prepared.quality().clone(),
        diagnostics:prepared.source_diagnostics().clone(),
        quality_period_months,quality_periods_partial,
        sample_reports:samples,
        sample_report_issues:report_issues,
        sample_source_issues:serde_json::to_value(issues).map_err(|e|e.to_string())?,
    })
}
#[tauri::command]
pub async fn native_wide_import_preview(
    app: tauri::AppHandle,
    source: String,
    options: WideOptions,
    workspace_token: Option<String>,
) -> Result<NativeWideReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let storage = routed_storage;
                with_prepared(
                    &storage,
                    "native-import-stages",
                    &source,
                    options,
                    |prepared, preview, options| {
                        review(
                            prepared,
                            preview,
                            options,
                            b"wide-manual-review-v1\0",
                            &options.organizations,
                        )
                    },
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn native_wide_import_apply(
    app: tauri::AppHandle,
    source: String,
    options: WideOptions,
    review_token: String,
    confirmation: NativeWideConfirmation,
    workspace_token: Option<String>,
) -> Result<ImportReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || { crate::native_shared_runtime::execute(&app, workspace_token.as_deref(), crate::storage::shared_runtime::Access::Write, |routed_storage| {
        if review_token.len()!=64||!review_token.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid import review token".into());
        }
        let storage=routed_storage;
        with_prepared(&storage,"native-import-stages",&source,options,|prepared,preview,options| {
            let current=review(prepared,preview,options,b"wide-manual-review-v1\0",&options.organizations)?;
            if current.review_token!=review_token {
                return Err("Source, report quality, conflicts or database changed. Preview again.".into());
            }
            if preview.counts().replacements>0 && !confirmation.replace_exact {
                return Err("Exact replacement needs explicit confirmation.".into());
            }
            if preview.counts().cross_type_overlaps>0 && !confirmation.accept_cross_type {
                return Err("Cross-type overlaps need explicit confirmation.".into());
            }
            if preview.quality().requires_acknowledgment() && !confirmation.accept_quality {
                return Err("Source quality notices need explicit confirmation.".into());
            }
            if preview.counts().replacements>0 {
                let destination=confirmation.backup_destination.as_deref()
                    .ok_or("A verified SQLite backup destination is required before replacing reports.")?;
                if destination.len()>32767 {
                    return Err("SQLite backup destination is too long".into());
                }
                // The online snapshot is created here in the trusted Rust
                // command, not accepted as a caller-supplied success claim.
                let backed_revision=storage.backup_sqlite_verified(Path::new(destination))?;
                if backed_revision!=preview.revision() {
                    return Err("SQLite revision changed while backing up. Preview again.".into());
                }
            }
            let flags=ImportConfirmation {
                replace_exact:confirmation.replace_exact,
                accept_cross_type:confirmation.accept_cross_type,
                replace_source:false, // manual mode; no folder-lifecycle bypass
                accept_quality:confirmation.accept_quality,
            };
            let job=ImportJob::new();
            storage.apply_import(prepared,preview,flags,&job,|_|{})
        })
     }) }).await.map_err(|e|e.to_string())?
}


/// The ordinary triple-column Excel format uses the same two-stage,
/// disk-backed native import, with a separately domain-bound review token.
fn with_legacy_prepared<T>(
    store: &Storage,
    stage_directory: &str,
    source: &str,
    options: LegacyOptions,
    action: impl FnOnce(&PreparedImport<'_>, &ImportPreview, &LegacyOptions) -> Result<T,String>,
) -> Result<T,String> {
    if source.len()>32767 { return Err("Excel path is too long".into()); }
    let path=Path::new(source);
    if !path.is_absolute() || !path.extension().and_then(|e|e.to_str()).is_some_and(|e|e.eq_ignore_ascii_case("xlsx")) {
        return Err("Choose an absolute .xlsx file".into());
    }
    store.revision()?;
    let area=StageArea::open(store.root().join(stage_directory))
        .map_err(|e|e.to_string())?;
    let cancelled=Arc::new(AtomicBool::new(false));
    let mut stage=area.ingest(path,Limits::default(),&cancelled,|_|{})
        .map_err(|e|e.to_string())?;
    stage.convert_legacy(&options,&cancelled,|_|{})
        .map_err(|e|e.to_string())?;
    let prepared=stage.prepare(&cancelled,|_|{})
        .map_err(|e|e.to_string())?;
    let preview=store.preview_import(&prepared)?;
    action(&prepared,&preview,&options)
}

#[tauri::command]
pub async fn native_import_sheets(
    app: tauri::AppHandle,
    source: String,
    rule: Option<crate::storage::Rule>,
    workspace_token: Option<String>,
) -> Result<Vec<NativeWorksheet>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if source.len() > 32767 {
                    return Err("Excel path too long".into());
                }
                let path = Path::new(&source);
                if !path.is_absolute()
                    || !path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
                {
                    return Err("Choose an absolute .xlsx file".into());
                }
                let storage = routed_storage;
                storage.revision()?;
                let area = StageArea::open(storage.root().join("native-import-stages"))
                    .map_err(|e| e.to_string())?;
                let cancel = AtomicBool::new(false);
                let stage = area
                    .ingest(path, Limits::default(), &cancel, |_| {})
                    .map_err(|e| e.to_string())?;
                let sheets = stage.sheet_names().map_err(|e| e.to_string())?;
                if sheets.len() > 64 {
                    return Err("Too many Excel worksheets for native review".into());
                }
                let auto = rule.as_ref().is_some_and(|r| r.format.as_deref() == Some("auto"));
                let classic_daily = if auto {
                    let orgs = storage.folder_import_organizations(storage.revision()?)?;
                    stage.classic_daily_workbook(&sheets, &orgs, &cancel).map_err(|e| e.to_string())?
                } else { false };
                let mut result = Vec::new();
                for (name, component) in sheets {
                    if !stage
                        .page(&component, 0, 1)
                        .map_err(|e| e.to_string())?
                        .is_empty()
                    {
                        let layout = stage.worksheet_layout(&component).map_err(|e| e.to_string())?;
                        let daily_wide = if auto { layout.map(|shape| shape == "daily-wide") } else { None };
                        result.push(NativeWorksheet { name, component, daily_wide, layout, classic_daily });
                    }
                }
                Ok(result)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct NativeWorksheet {
    pub name: String,
    pub component: String,
    #[serde(rename="dailyWide",skip_serializing_if="Option::is_none")]
    pub daily_wide: Option<bool>,
    pub layout: Option<&'static str>,
    #[serde(rename="classicDaily")]
    pub classic_daily: bool,
}

#[tauri::command]
pub async fn native_legacy_import_sheets(
    app: tauri::AppHandle,
    source: String,
    workspace_token: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if source.len() > 32767 {
                    return Err("Excel path too long".into());
                }
                let path = Path::new(&source);
                if !path.is_absolute()
                    || !path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
                {
                    return Err("Choose an absolute .xlsx file".into());
                }
                let storage = routed_storage;
                storage.revision()?;
                let area = StageArea::open(storage.root().join("native-import-stages"))
                    .map_err(|e| e.to_string())?;
                let cancel = AtomicBool::new(false);
                let stage = area
                    .ingest(path, Limits::default(), &cancel, |_| {})
                    .map_err(|e| e.to_string())?;
                stage.worksheet_components().map_err(|e| e.to_string())
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn native_legacy_import_preview(
    app: tauri::AppHandle,
    source: String,
    options: LegacyOptions,
    workspace_token: Option<String>,
) -> Result<NativeWideReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let storage = routed_storage;
                with_legacy_prepared(
                    &storage,
                    "native-import-stages",
                    &source,
                    options,
                    |prepared, preview, options| {
                        review(
                            prepared,
                            preview,
                            options,
                            b"legacy-manual-review-v1\0",
                            &options.organizations,
                        )
                    },
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn native_legacy_import_apply(
    app: tauri::AppHandle,
    source: String,
    options: LegacyOptions,
    review_token: String,
    confirmation: NativeWideConfirmation,
    workspace_token: Option<String>,
) -> Result<ImportReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || { crate::native_shared_runtime::execute(&app, workspace_token.as_deref(), crate::storage::shared_runtime::Access::Write, |routed_storage| {
        if review_token.len()!=64 || !review_token.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid import review token".into());
        }
        let storage=routed_storage;
        with_legacy_prepared(&storage,"native-import-stages",&source,options,|prepared,preview,options| {
            let current=review(prepared,preview,options,b"legacy-manual-review-v1\0",&options.organizations)?;
            if current.review_token!=review_token {
                return Err("Excel, report quality, conflicts or SQLite changed. Preview again.".into());
            }
            if preview.counts().replacements>0 && !confirmation.replace_exact {
                return Err("Replacing existing reports requires confirmation.".into());
            }
            if preview.counts().cross_type_overlaps>0 && !confirmation.accept_cross_type {
                return Err("Overlapping report types require confirmation.".into());
            }
            if preview.quality().requires_acknowledgment() && !confirmation.accept_quality {
                return Err("Excel quality warnings require confirmation.".into());
            }
            if preview.counts().replacements>0 {
                let destination=confirmation.backup_destination.as_deref()
                    .ok_or("Verified SQLite backup required before replacement.")?;
                if destination.len()>32767 {
                    return Err("SQLite backup path too long".into());
                }
                let backed_revision=storage.backup_sqlite_verified(Path::new(destination))?;
                if backed_revision!=preview.revision() {
                    return Err("SQLite changed during backup. Preview again.".into());
                }
            }
            let flags=ImportConfirmation {
                replace_exact:confirmation.replace_exact,
                accept_cross_type:confirmation.accept_cross_type,
                replace_source:false, // manual import cannot bypass source ownership
                accept_quality:confirmation.accept_quality,
            };
            let job=ImportJob::new();
            storage.apply_import(prepared,preview,flags,&job,|_|{})
        })
     }) }).await.map_err(|e|e.to_string())?
}

/// A lease belongs to the window that opted in and expires on stop or process exit.
#[tauri::command]
pub async fn native_folder_import_session(
    window: tauri::WebviewWindow,
    enabled: bool,
    workspace_token: Option<String>,
) -> Result<Option<String>, String> {
    if !enabled {
        window
            .state::<crate::storage::shared_runtime::Runtime>()
            .close_window(window.label());
        return Ok(None);
    }

    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &window,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Local,
            |routed_storage| {
                let store = routed_storage;
                let lease = store.set_folder_import_session(window.label(), enabled)?;
                if enabled
                    && window
                        .app_handle()
                        .get_webview_window(window.label())
                        .is_none()
                {
                    store.set_folder_import_session(window.label(), false)?;
                    return Err("Окно автоимпорта закрыто.".into());
                }
                Ok(lease)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn with_folder_prepared<T>(
    store: &Storage,
    window: &str,
    request: &crate::storage::FolderImportRequest,
    action: impl FnOnce(&PreparedImport<'_>, &ImportPreview, &ImportJob, &crate::storage::Rule) -> Result<T,String>,
) -> Result<T,String> {
    let operation = store.begin_folder_import(window, request)?;
    let cancel = operation.job.cancellation();
    let (rule, source, file) = store.checked_folder_import(request)?;
    if !matches!(rule.format.as_deref(), None | Some("legacy" | "daily-wide" | "period-pairs" | "auto")) {
        return Err("Выберите формат импорта файла.".into());
    }
    let organizations = store.folder_import_organizations(request.revision)?;
    let area = StageArea::open(store.root().join("native-import-stages")).map_err(|e|e.to_string())?;
    let mut stage = area.ingest_opened(source, file, Limits::default(), cancel, |_|{})
        .map_err(|e|e.to_string())?;
    let mut wide_options=rule.native_wide_options(organizations.clone())?;
    let book=stage.sheet_names().map_err(|e|e.to_string())?;
    let wide = if rule.format.as_deref() == Some("auto") {
        if stage.classic_daily_workbook(&book, &organizations, cancel).map_err(|e| e.to_string())? {
            wide_options.kind = "day".into();
            wide_options.start = None;
            true
        } else {
            let sheet = rule.native_legacy_sheet(&book)?;
            let layout = stage.worksheet_layout(&sheet).map_err(|e| e.to_string())?
                .ok_or("Не удалось однозначно определить структуру Excel. Укажите её в правиле файла.")?;
            if layout == "daily-wide" {
                wide_options.sheets = book.iter().filter(|(_, component)| *component == sheet).map(|(name, _)| name.clone()).collect();
            }
            layout == "daily-wide"
        }
    } else { matches!(rule.format.as_deref(), Some("daily-wide" | "period-pairs")) };
    if wide {
        stage.convert_wide(&wide_options,operation.job.cancellation_handle(),|_|{}).map_err(|e|e.to_string())?;
    } else {
        let sheet=rule.native_legacy_sheet(&book)?;
        let options=LegacyOptions{period_dates:None,sheet,kind:rule.kind.clone(),start:rule.start.clone(),organizations,mappings:None};
        stage.convert_legacy(&options,cancel,|_|{}).map_err(|e|e.to_string())?;
    }
    let prepared = stage.prepare_with_mode(analiz_import_core::ImportMode::Folder, cancel, |_|{})
        .map_err(|e|e.to_string())?;
    let preview = store.preview_import_with_cancel(&prepared, cancel)?;
    if preview.revision() != request.revision {
        return Err("Stale storage revision; refresh the selection.".into());
    }
    action(&prepared, &preview, &operation.job, &rule)
}

fn is_clean_new_folder_review(review: &NativeWideReview) -> bool {
    let (c, q) = (&review.conflicts, &review.quality);
    review.report_count > 0 && c.new_reports == review.report_count
        && c.replacements == 0 && c.cross_type_overlaps == 0
        && [q.issues, q.invalid, q.skipped, q.mismatches, q.other_notices, q.mismatch_reports,
            q.source_issues, q.relevant_source_issues, q.deferred_source_issues]
            .into_iter().all(|count| count == 0)
}

#[tauri::command]
pub async fn native_folder_import_preview(
    window: tauri::WebviewWindow,
    request: crate::storage::FolderImportRequest,
    workspace_token: Option<String>,
) -> Result<NativeWideReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &window,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                with_folder_prepared(
                    &store,
                    window.label(),
                    &request,
                    |prepared, preview, _, rule| {
                        review(
                            prepared,
                            preview,
                            &(&request, rule),
                            b"folder-auto-review-v1\0",
                            &store.folder_import_organizations(request.revision)?,
                        )
                    },
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn native_folder_import_apply(
    window: tauri::WebviewWindow,
    request: crate::storage::FolderImportRequest,
    review_token: String,
    workspace_token: Option<String>,
) -> Result<ImportReceipt, String> {
    if review_token.len() != 64 || !review_token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid import review token.".into());
    }
    tauri::async_runtime::spawn_blocking(move || { crate::native_shared_runtime::execute(&window, workspace_token.as_deref(), crate::storage::shared_runtime::Access::Write, |routed_storage| {
        let store = routed_storage;
        with_folder_prepared(&store, window.label(), &request, |prepared, preview, job, rule| {
            let current = review(prepared, preview, &(&request, rule), b"folder-auto-review-v1\0", &store.folder_import_organizations(request.revision)?)?;
            if current.review_token != review_token || !is_clean_new_folder_review(&current) {
                return Err("Изменения, пересечения, замены и замечания требуют ручной проверки.".into());
            }
            store.apply_import(prepared, preview, ImportConfirmation::default(), job, |_|{})
        })
     }) }).await.map_err(|e|e.to_string())?
}

/// Every paged read or export reopens the source and verifies the original
/// preview token, which includes Excel contents, conversion options and revision.
#[derive(Deserialize)]
#[serde(tag="format", content="options")]
pub enum QualityImportOptions {
    #[serde(rename="daily-wide")]
    Wide(WideOptions),
    #[serde(rename="legacy")]
    Legacy(LegacyOptions),
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ReviewedQualityQuery {
    source:String,
    selection:QualityImportOptions,
    review_token:String,
    filter:crate::storage::UnifiedQualityFilter,
}
impl ReviewedQualityQuery {
    fn read<T>(self,store:&Storage,action:impl FnOnce(&PreparedImport<'_>,i64,&crate::storage::UnifiedQualityFilter)->Result<T,String>)->Result<T,String>{
        if self.review_token.len()!=64||!self.review_token.bytes().all(|b|b.is_ascii_hexdigit()){return Err("Invalid import review token".into());}
        let accept=|prepared:&PreparedImport<'_>,preview:&ImportPreview,current:NativeWideReview|{
            if current.review_token!=self.review_token{return Err("Excel, report quality, conflicts or SQLite changed. Preview again.".into());}
            action(prepared,preview.revision(),&self.filter)
        };
        // A quality read can outlive the WebView filter which started it. Keep
        // that scratch area separate from preview/apply and serialize readers.
        static READING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = READING.lock().map_err(|_| "Quality staging is unavailable")?;
        match self.selection{
            QualityImportOptions::Wide(options)=>with_prepared(store,"native-quality-stages",&self.source,options,|p,v,o|accept(p,v,review(p,v,o,b"wide-manual-review-v1\0",&o.organizations)?)),
            QualityImportOptions::Legacy(options)=>with_legacy_prepared(store,"native-quality-stages",&self.source,options,|p,v,o|accept(p,v,review(p,v,o,b"legacy-manual-review-v1\0",&o.organizations)?)),
        }
    }
}
#[tauri::command]
pub async fn native_preview_quality_page(
    app: tauri::AppHandle,
    query: ReviewedQualityQuery,
    offset: u32,
    workspace_token: Option<String>,
) -> Result<crate::storage::UnifiedQualityPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                query.read(&store, |prepared, revision, filter| {
                    store.prepared_quality_page(revision, prepared, filter, offset)
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn native_preview_quality_csv(
    app: tauri::AppHandle,
    query: ReviewedQualityQuery,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                query.read(&store, |prepared, revision, filter| {
                    store.prepared_quality_csv(revision, prepared, filter, Path::new(&destination))
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod reviewed_quality_tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn workbook(path: &Path, name: &str) {
        let mut writer = ZipWriter::new(std::fs::File::create(path).unwrap());
        writer.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default()).unwrap();
        write!(writer,r#"<worksheet><sheetData><row r="2"><c r="A2" t="inlineStr"><is><t>СМУ16</t></is></c><c r="B2"><v>1</v></c><c r="C2"><v>2</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>{name}</t></is></c><c r="B3"><v>1</v></c><c r="C3"><v>2</v></c></row></sheetData></worksheet>"#).unwrap();
        writer.start_file("xl/worksheets/sheet2.xml", SimpleFileOptions::default()).unwrap();
        write!(writer, r#"<worksheet><sheetData><row r="20"><c r="C20"/></row></sheetData></worksheet>"#).unwrap();
        writer.finish().unwrap();
    }

    #[test]
    fn reviewed_week_range_records_an_off_schedule_week_between_matching_endpoints() {
        let mut range = NativePeriodRange { kind: "week".into(), from: "2026-11-04".into(),
            to: "2026-11-10".into(), mixed_week_starts: false };
        range.include("week", "2026-11-25", "2026-12-01").unwrap();
        assert!(serde_json::to_value(&range).unwrap().get("mixedWeekStarts").is_none());
        range.include("week", "2026-11-12", "2026-11-18").unwrap();
        assert!(range.mixed_week_starts);
        assert_eq!((range.from.as_str(), range.to.as_str()), ("2026-11-04", "2026-12-01"));
    }

    #[test]
    fn reviewed_quality_rejects_same_path_different_bytes_before_creating_csv() {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("quality.xlsx");
        workbook(&path,"Исходная работа");
        let source=path.to_str().unwrap();
        let store=Storage::new(dir.path().join("db")).unwrap();
        let options=||LegacyOptions{
            period_dates:None,sheet:"xl/worksheets/sheet1.xml".into(),kind:"day".into(),start:"2024-01-01".into(),
            organizations:["СМУ16".into(),"АВТО".into(),"ТРАК".into(),"ЦЕНТР".into()],
            mappings:Some(vec![analiz_import_core::TripleMapping{header_row:1,work_col:1,count_col:2,amount_col:3}]),
        };
        let reviewed=with_legacy_prepared(&store,"native-import-stages",source,options(),|p,v,o|review(p,v,o,b"legacy-manual-review-v1\0",&o.organizations)).unwrap();
        // Six cells on the selected sheet plus the explicit empty cell on the unused sheet.
        assert_eq!(reviewed.processed_cell_count, 7);
        assert_eq!(reviewed.quality_period_months.get("day:2024-01"),Some(&1));
        assert!(!reviewed.quality_periods_partial);
        let range=reviewed.period_range.as_ref().unwrap();
        assert_eq!((&*range.kind,&*range.from,&*range.to),("day","2024-01-01","2024-01-01"));
        let make_query=||ReviewedQualityQuery{
            source:source.into(),selection:QualityImportOptions::Legacy(options()),review_token:reviewed.review_token.clone(),filter:Default::default(),
        };
        make_query().read(&store,|p,r,f|store.prepared_quality_page(r,p,f,0)).unwrap();
        workbook(&path,"Изменённая работа");
        let output=dir.path().join("stale.csv");
        let result=make_query().read(&store,|p,r,f|store.prepared_quality_csv(r,p,f,&output));
        assert!(result.unwrap_err().contains("Preview again"));
        assert!(!output.exists());assert_eq!(store.revision().unwrap(),reviewed.revision);
    }
}
