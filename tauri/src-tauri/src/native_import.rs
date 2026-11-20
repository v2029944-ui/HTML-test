//! Explicit two-phase, disk-backed daily-wide import for the separate SQL UI.
//! Stage objects borrow StageArea and may not cross commands. Apply repeats the
//! bounded parse, verifies the exact reviewed digest, then invokes C3's atomic
//! revision-checked publication. No report JSON enters the WebView.
use crate::storage::{ImportConfirmation, ImportCounts, ImportJob, ImportPreview, ImportReceipt, Storage};
use analiz_import_core::{LegacyOptions, Limits, PreparedImport, QualityCounts, SourceDiagnostics, StageArea, WideOptions};
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
    pub row_count: u64,
    pub catalog_entries: u64,
    pub conflicts: ImportCounts,
    pub quality: QualityCounts,
    pub diagnostics: SourceDiagnostics,
    pub sample_reports: Vec<NativeReportSample>,
    pub sample_report_issues: Vec<serde_json::Value>,
    pub sample_source_issues: serde_json::Value,
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
    let area=StageArea::open(store.root().join("native-import-stages"))
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
    let mut after=0;
    let mut reports=0u64;
    let mut rows=0u64;
    while let Some(report)=prepared.next_report(after).map_err(|e|e.to_string())? {
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
        report_count:reports,row_count:rows,catalog_entries,
        conflicts:preview.counts().clone(),
        quality:prepared.quality().clone(),
        diagnostics:prepared.source_diagnostics().clone(),
        sample_reports:samples,
        sample_report_issues:report_issues,
        sample_source_issues:serde_json::to_value(issues).map_err(|e|e.to_string())?,
    })
}
#[tauri::command]
pub async fn native_wide_import_preview(
    app: tauri::AppHandle,
    source:String,
    options:WideOptions,
) -> Result<NativeWideReview,String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage=app.state::<Storage>();
        with_prepared(&storage,&source,options,|prepared,preview,options| {
            review(prepared,preview,options,b"wide-manual-review-v1\0",&options.organizations)
        })
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn native_wide_import_apply(
    app: tauri::AppHandle,
    source:String,
    options:WideOptions,
    review_token:String,
    confirmation:NativeWideConfirmation,
) -> Result<ImportReceipt,String> {
    tauri::async_runtime::spawn_blocking(move || {
        if review_token.len()!=64||!review_token.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid import review token".into());
        }
        let storage=app.state::<Storage>();
        with_prepared(&storage,&source,options,|prepared,preview,options| {
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
    }).await.map_err(|e|e.to_string())?
}


/// The ordinary triple-column Excel format uses the same two-stage,
/// disk-backed native import, with a separately domain-bound review token.
fn with_legacy_prepared<T>(
    store: &Storage,
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
    let area=StageArea::open(store.root().join("native-import-stages"))
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
pub async fn native_legacy_import_sheets(
    app: tauri::AppHandle,
    source: String,
) -> Result<Vec<String>,String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source.len()>32767 {
            return Err("Excel path too long".into());
        }
        let path=Path::new(&source);
        if !path.is_absolute() || !path.extension().and_then(|e|e.to_str())
            .is_some_and(|e|e.eq_ignore_ascii_case("xlsx"))
        {
            return Err("Choose an absolute .xlsx file".into());
        }
        let storage=app.state::<Storage>();
        storage.revision()?;
        let area=StageArea::open(storage.root().join("native-import-stages"))
            .map_err(|e|e.to_string())?;
        let cancel=AtomicBool::new(false);
        let stage=area.ingest(path,Limits::default(),&cancel,|_|{})
            .map_err(|e|e.to_string())?;
        stage.worksheet_components().map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn native_legacy_import_preview(
    app: tauri::AppHandle,
    source: String,
    options: LegacyOptions,
) -> Result<NativeWideReview,String> {
    tauri::async_runtime::spawn_blocking(move || {
        let storage=app.state::<Storage>();
        with_legacy_prepared(&storage,&source,options,|prepared,preview,options| {
            review(prepared,preview,options,b"legacy-manual-review-v1\0",&options.organizations)
        })
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn native_legacy_import_apply(
    app: tauri::AppHandle,
    source: String,
    options: LegacyOptions,
    review_token: String,
    confirmation: NativeWideConfirmation,
) -> Result<ImportReceipt,String> {
    tauri::async_runtime::spawn_blocking(move || {
        if review_token.len()!=64 || !review_token.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid import review token".into());
        }
        let storage=app.state::<Storage>();
        with_legacy_prepared(&storage,&source,options,|prepared,preview,options| {
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
    }).await.map_err(|e|e.to_string())?
}

/// A lease belongs to the window that opted in and expires on stop or process exit.
#[tauri::command]
pub async fn native_folder_import_session(
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = window.state::<Storage>();
        let lease = store.set_folder_import_session(window.label(), enabled)?;
        if enabled && window.app_handle().get_webview_window(window.label()).is_none() {
            store.set_folder_import_session(window.label(), false)?;
            return Err("Окно автоимпорта закрыто.".into());
        }
        Ok(lease)
    }).await.map_err(|e|e.to_string())?
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
    if !matches!(rule.format.as_deref(), None | Some("legacy" | "daily-wide" | "auto")) {
        return Err("Выберите формат импорта файла.".into());
    }
    let organizations = store.folder_import_organizations(request.revision)?;
    let area = StageArea::open(store.root().join("native-import-stages")).map_err(|e|e.to_string())?;
    let mut stage = area.ingest_opened(source, file, Limits::default(), cancel, |_|{})
        .map_err(|e|e.to_string())?;
    let wide_options=rule.native_wide_options(organizations.clone())?;
    let wide=rule.format.as_deref()==Some("daily-wide") ||
        rule.format.as_deref()==Some("auto") && !stage.detect_wide(&wide_options,operation.job.cancellation_handle()).map_err(|e|e.to_string())?.is_empty();
    if wide {
        stage.convert_wide(&wide_options,operation.job.cancellation_handle(),|_|{}).map_err(|e|e.to_string())?;
    } else {
        let book=stage.sheet_names().map_err(|e|e.to_string())?;
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
) -> Result<NativeWideReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = window.state::<Storage>();
        with_folder_prepared(&store, window.label(), &request, |prepared, preview, _, rule| {
            review(prepared, preview, &(&request, rule), b"folder-auto-review-v1\0", &store.folder_import_organizations(request.revision)?)
        })
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn native_folder_import_apply(
    window: tauri::WebviewWindow,
    request: crate::storage::FolderImportRequest,
    review_token: String,
) -> Result<ImportReceipt, String> {
    if review_token.len() != 64 || !review_token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid import review token.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let store = window.state::<Storage>();
        with_folder_prepared(&store, window.label(), &request, |prepared, preview, job, rule| {
            let current = review(prepared, preview, &(&request, rule), b"folder-auto-review-v1\0", &store.folder_import_organizations(request.revision)?)?;
            if current.review_token != review_token || !is_clean_new_folder_review(&current) {
                return Err("Изменения, пересечения, замены и замечания требуют ручной проверки.".into());
            }
            store.apply_import(prepared, preview, ImportConfirmation::default(), job, |_|{})
        })
    }).await.map_err(|e|e.to_string())?
}
