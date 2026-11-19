use crate::{storage::*, with_selected_work_names};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

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
    fn with_scope<T>(
        &self,
        store: &Storage,
        action: impl FnOnce(&AnalysisScope, Option<&str>) -> Result<T, String>,
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
                |s| action(s, grouping),
            )
        })();
        let cleanup = store.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
}

#[tauri::command]
pub async fn native_changes_page(
    app: AppHandle,
    request: AdvancedRequest,
    after: Option<ChangeCursor>,
) -> Result<AdvancedChangesPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        request.with_scope(&store, |s, g| {
            store.advanced_changes_page(request.revision, s, &request.query, g, after.as_ref())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn native_changes_csv(
    app: AppHandle,
    request: AdvancedRequest,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
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
        let store = app.state::<Storage>();
        request.with_scope(&store, |s, g| {
            store.advanced_changes_csv(request.revision, s, &request.query, g, path)
        })
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
) -> Result<PatternPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        request.with_scope(&store, |s, g| {
            store.advanced_patterns(
                request.revision,
                s,
                &request.query,
                g,
                &mode,
                window_size,
                after.as_deref(),
            )
        })
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
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Storage>();
        request.with_scope(&store, |s, g| {
            store.advanced_composition(
                request.revision,
                s,
                &request.query,
                g,
                &before,
                &mode,
                &metric,
                after.as_ref(),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
