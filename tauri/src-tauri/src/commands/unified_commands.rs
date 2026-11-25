//! Bounded IPC adapters for the canonical interface. Domain methods retain
//! revision checks, streaming, transaction and source-integrity ownership.
use crate::{native_advanced_analysis::AdvancedRequest, storage::*};
use std::path::Path;
use tauri::AppHandle;

#[tauri::command]
pub async fn unified_period_options(
    app: AppHandle,
    revision: i64,
    report_type: String,
    grouping: Option<String>,
    after: Option<String>,
    limit: usize,
    week_calendar_month: Option<String>,
    workspace_token: Option<String>,
) -> Result<UnifiedPeriodOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_period_options(
                    revision,
                    &report_type,
                    grouping.as_deref(),
                    after.as_deref(),
                    limit,
                    week_calendar_month.as_deref(),
                )
            },
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
    selected_keys: Option<Vec<String>>,
    all_series: Option<bool>,
    workspace_token: Option<String>,
) -> Result<UnifiedNameOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                if all_series == Some(true) {
                    let index = page_index.ok_or("Work series requires a page index")?;
                    if !search.is_empty() || after.is_some() || selected_keys.is_some() {
                        return Err("Work series cannot be combined with a name filter".into());
                    }
                    return store.unified_work_series_at_page(revision, index, limit);
                }
                if let Some(keys) = &selected_keys {
                    if !search.is_empty() || page_index.is_some() || after.is_some() || limit != 50 {
                        return Err("Selected name lookup cannot be combined with search or paging".into());
                    }
                    return store.unified_selected_name_options(revision, keys);
                }
                if let Some(index) = page_index {
                    if after.is_some() {
                        return Err("Do not combine a cursor and a page index".into());
                    }
                    store.unified_name_options_at_page(revision, &search, index, limit)
                } else {
                    store.unified_name_options(revision, &search, after.as_deref(), limit)
                }
            },
        )
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
    workspace_token: Option<String>,
) -> Result<UnifiedChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if page_index.is_some() && after.is_some() {
                    return Err("Do not combine a cursor and a page index".into());
                }
                let store = routed_storage;
                request.with_scope(&store, |revision, scope, query, grouping| {
                    if let Some(index) = page_index {
                        return store.unified_changes_page_at_page(
                            revision,
                            scope,
                            query,
                            grouping,
                            &options,
                            index,
                        );
                    }
                    store.unified_changes_page(
                        revision,
                        scope,
                        query,
                        grouping,
                        &options,
                        after.as_deref(),
                    )
                })
            },
        )
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
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
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
                let store = routed_storage;
                request.with_scope(&store, |revision, scope, query, grouping| {
                    store.unified_changes_csv(revision, scope, query, grouping, &options, path)
                })
            },
        )
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
    workspace_token: Option<String>,
) -> Result<PatternPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if page_index.is_some() && after.is_some() {
                    return Err("Do not combine a cursor and a page index".into());
                }
                let store = routed_storage;
                request.with_scope(&store, |revision, scope, query, grouping| {
                    if let Some(index) = page_index {
                        return store.unified_pattern_page_at_page(
                            revision,
                            scope,
                            query,
                            grouping,
                            &mode,
                            window_size,
                            &options,
                            index,
                        );
                    }
                    store.unified_pattern_page(
                        revision,
                        scope,
                        query,
                        grouping,
                        &mode,
                        window_size,
                        &options,
                        after.as_deref(),
                    )
                })
            },
        )
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
    workspace_token: Option<String>,
) -> Result<Vec<ComparisonValue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !(1..=50).contains(&limit) {
                    return Err("Comparison page must contain 1..=50 rows".into());
                }
                let store = routed_storage;
                let organization = organization.as_deref();
                request.with_comparison_scope(
                    store,
                    before,
                    organization,
                    |revision, scope, query| {
                        store.analysis_comparison_page(
                            revision,
                            scope,
                            query,
                            organization,
                            after.as_deref(),
                            limit,
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
pub async fn unified_comparison_window(
    app: AppHandle,
    request: AdvancedRequest,
    before: RangeQuery,
    organization: Option<String>,
    page_index: usize,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<SqlPage<ComparisonValue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !(1..=50).contains(&limit) {
                    return Err("Comparison page must contain 1..=50 rows".into());
                }
                let store = routed_storage;
                let organization = organization.as_deref();
                request.with_comparison_scope(
                    store,
                    before,
                    organization,
                    |revision, scope, query| {
                        store.analysis_comparison_window(
                            revision,
                            scope,
                            query,
                            organization,
                            page_index,
                            limit,
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
pub async fn unified_organization_work_window(
    app: AppHandle,
    request: AdvancedRequest,
    page_index: usize,
    limit: usize,
    workspace_token: Option<String>,
) -> Result<SqlPage<OrganizationWorkValues>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |store| {
                request.with_range_scope(store, |revision, scope, range, _| {
                    store.organization_work_window(revision, scope, range, page_index, limit)
                })
            },
        )
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
    workspace_token: Option<String>,
) -> Result<UnifiedGroupAuditPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                request.with_range_scope(store, |revision, scope, range, _| {
                    store.unified_group_audit_page(
                        revision, scope, range, &mode, &search, offset, limit,
                    )
                })
            },
        )
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
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| {
                routed_storage.unified_rename_organizations(
                    revision,
                    &names,
                    Path::new(&destination),
                    &ManagementJob::new(),
                )
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_clear_reports(
    app: AppHandle,
    revision: i64,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |routed_storage| routed_storage.unified_clear_reports(revision, &ManagementJob::new()),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_deletion_count(
    app: AppHandle,
    revision: i64,
    scope: crate::storage::UnifiedReportDeletion,
    workspace_token: Option<String>,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |store| store.unified_report_deletion_count(revision, &scope),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_delete_reports(
    app: AppHandle,
    revision: i64,
    scope: crate::storage::UnifiedReportDeletion,
    confirmed_count: i64,
    workspace_token: Option<String>,
) -> Result<ManagementReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Write,
            |store| store.unified_delete_reports(revision, &scope, confirmed_count, &ManagementJob::new()),
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
    workspace_token: Option<String>,
) -> Result<Vec<ManagementReport>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !(1..=50).contains(&limit) {
                    return Err("Report page must contain 1..=50 rows".into());
                }
                routed_storage.management_report_page(revision, &filter, after.as_ref(), limit)
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_report_summary(
    app: AppHandle,
    revision: i64,
    filter: ManagementReportFilter,
    workspace_token: Option<String>,
) -> Result<ManagementReportSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.management_report_summary(revision, &filter),
        )
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
    workspace_token: Option<String>,
) -> Result<Vec<(i64, String)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                if !(1..=50).contains(&limit) {
                    return Err("Report organization page must contain 1..=50 rows".into());
                }
                routed_storage.management_report_org_page(revision, &id, after, limit)
            },
        )
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
    workspace_token: Option<String>,
) -> Result<UnifiedQualityPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.unified_quality_page(revision, &filter, offset, limit),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_calendar(
    app: AppHandle,
    revision: i64,
    year: i32,
    workspace_token: Option<String>,
) -> Result<UnifiedCalendar, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| routed_storage.unified_calendar(revision, year),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_quality_csv(
    app: AppHandle,
    revision: i64,
    filter: UnifiedQualityFilter,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                routed_storage.unified_quality_csv(revision, &filter, Path::new(&destination))
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unified_group_audit_csv(
    app: AppHandle,
    request: AdvancedRequest,
    mode: String,
    search: String,
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
                request.with_range_scope(store, |revision, scope, range, _| {
                    store.unified_group_audit_csv(
                        revision,
                        scope,
                        range,
                        &mode,
                        &search,
                        Path::new(&destination),
                    )
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
