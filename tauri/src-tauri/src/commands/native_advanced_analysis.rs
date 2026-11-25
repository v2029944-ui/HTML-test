use crate::{storage::*, with_selected_work_names};
use serde::Deserialize;
use tauri::AppHandle;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedRequest {
    revision: i64,
    source: String,
    display_type: String,
    organizations: Vec<String>,
    selected_names: Vec<String>,
    query: ChangeQuery,
}
impl AdvancedRequest {
    pub(crate) fn with_scope<T>(
        &self,
        store: &Storage,
        action: impl FnOnce(i64, &AnalysisScope, &ChangeQuery, Option<&str>) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.organizations.is_empty() || self.organizations.len() > 100 {
            return Err("Select 1..=100 organizations".into());
        }
        let display = self.display_type.as_str();
        let (kind, grouping) = match (self.source.as_str(), display) {
            ("daily", "day" | "week" | "month" | "year") => ("day", Some(display)),
            ("imported", "day" | "week" | "month") => (display, None),
            _ => return Err("Invalid source or calendar period".into()),
        };
        let rev = self.revision;
        let id = store.analysis_filter_create(rev, "organizations")?;
        let result = (|| {
            store.analysis_filter_append(rev, &id, &self.organizations)?;
            store.analysis_filter_seal(rev, &id)?;
            let scope = AnalysisScope {
                report_type: kind.into(),
                organizations: id.clone(),
                names: None,
            };
            with_selected_work_names(
                store,
                rev,
                None,
                Some(self.selected_names.clone()),
                scope,
                |s| action(rev, s, &self.query, grouping),
            )
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }

    pub(crate) fn with_range_scope<T>(
        &self,
        store: &Storage,
        action: impl FnOnce(i64, &AnalysisScope, &RangeQuery, Option<&str>) -> Result<T, String>,
    ) -> Result<T, String> {
        let range = self.range();
        self.with_scope(store, |revision, scope, _, grouping| {
            action(revision, scope, &range, grouping)
        })
    }

    pub(crate) fn with_comparison_scope<T>(
        &self,
        store: &Storage,
        before: RangeQuery,
        organization: Option<&str>,
        action: impl FnOnce(i64, &AnalysisScope, &ComparisonQuery) -> Result<T, String>,
    ) -> Result<T, String> {
        if organization.is_some_and(|org| !self.organizations.iter().any(|name| name == org)) {
            return Err("Selected organization is outside the analysis scope".into());
        }
        let query = ComparisonQuery {
            before,
            after: self.range(),
            percent: self.query.percent,
            min_count: self.query.min_count,
            min_amount: self.query.min_amount,
        };
        self.with_scope(store, |revision, scope, _, _| {
            action(revision, scope, &query)
        })
    }

    fn range(&self) -> RangeQuery {
        RangeQuery {
            from: self.query.from.clone(),
            to: self.query.to.clone(),
        }
    }
}

#[tauri::command]
pub async fn native_changes_page(
    app: AppHandle,
    request: AdvancedRequest,
    after: Option<ChangeCursor>,
    workspace_token: Option<String>,
) -> Result<AdvancedChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                request.with_scope(&store, |revision, scope, query, grouping| {
                    store.advanced_changes_page(revision, scope, query, grouping, after.as_ref())
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn native_changes_csv(
    app: AppHandle,
    request: AdvancedRequest,
    destination: String,
    workspace_token: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let path = std::path::Path::new(&destination);
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
                    store.advanced_changes_csv(revision, scope, query, grouping, path)
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn native_pattern_page(
    app: AppHandle,
    request: AdvancedRequest,
    mode: String,
    window_size: i32,
    after: Option<String>,
    workspace_token: Option<String>,
) -> Result<PatternPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_shared_runtime::execute(
            &app,
            workspace_token.as_deref(),
            crate::storage::shared_runtime::Access::Read,
            |routed_storage| {
                let store = routed_storage;
                request.with_scope(&store, |revision, scope, query, grouping| {
                    store.advanced_patterns(
                        revision,
                        scope,
                        query,
                        grouping,
                        &mode,
                        window_size,
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
pub async fn native_composition_page(
    app: AppHandle,
    request: AdvancedRequest,
    before: RangeQuery,
    mode: String,
    metric: String,
    after: Option<ContributionCursor>,
    structure_limit: Option<usize>,
    page_index: Option<usize>,
    workspace_token: Option<String>,
) -> Result<serde_json::Value, String> {
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
                        return store.advanced_composition_at_page(
                            revision,
                            scope,
                            query,
                            grouping,
                            &before,
                            &mode,
                            &metric,
                            structure_limit.unwrap_or(12),
                            index,
                        );
                    }
                    store.advanced_composition_limit(
                        revision,
                        scope,
                        query,
                        grouping,
                        &before,
                        &mode,
                        &metric,
                        after.as_ref(),
                        structure_limit.unwrap_or(12),
                    )
                })
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
