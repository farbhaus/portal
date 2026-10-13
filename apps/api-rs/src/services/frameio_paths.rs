
//! Best-effort resolution/caching of Frame.io folder breadcrumbs — port of
//! `portal.services.frameio_paths`. `clean_path` and `stamp_path` are pure; the resolvers call
//! Frame.io and return None on any error (path data is display-only and must never fail a request).

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::frameio::FrameioError;
use crate::state::AppState;

pub const PATH_TTL_SECONDS: i64 = 3600;

/// `datetime.now(UTC).isoformat()` for the cached `path_resolved_at` stamp.
fn now_iso() -> String {
    crate::pyfmt::iso(&Utc::now())
}

/// Validate a client-supplied breadcrumb; None unless well-formed and ending at `folder_id`.
pub fn clean_path(raw: Option<&Value>, folder_id: &str) -> Option<Vec<Value>> {
    let arr = raw?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut path = Vec::with_capacity(arr.len());
    for seg in arr {
        let id = seg.get("id").and_then(|v| v.as_str())?;
        let name = seg.get("name").and_then(|v| v.as_str())?;
        if id.is_empty() || name.is_empty() {
            return None;
        }
        path.push(json!({ "id": id, "name": name }));
    }
    if path.last()?.get("id").and_then(|v| v.as_str()) != Some(folder_id) {
        return None;
    }
    Some(path)
}

fn path_is_fresh(config: &Value) -> bool {
    let has_path = config.get("path").map(|v| !v.is_null()).unwrap_or(false);
    if !has_path {
        return false;
    }
    let Ok(resolved) = DateTime::parse_from_rfc3339(
        config.get("path_resolved_at").and_then(|v| v.as_str()).unwrap_or(""),
    ) else {
        return false;
    };
    (Utc::now() - resolved.with_timezone(&Utc)).num_seconds() < PATH_TTL_SECONDS
}

/// Copy of config with the breadcrumb fields set (folder_name only filled when missing).
pub fn stamp_path(config: &Value, path: &[Value], project_name: Option<&str>) -> Value {
    let mut out = config.as_object().cloned().unwrap_or_default();
    out.insert("path".to_string(), Value::Array(path.to_vec()));
    if !out.contains_key("folder_name") {
        if let Some(name) = path.last().and_then(|s| s.get("name")) {
            out.insert("folder_name".to_string(), name.clone());
        }
    }
    if let Some(p) = project_name {
        out.insert("project_name".to_string(), json!(p));
    }
    out.insert("path_resolved_at".to_string(), json!(now_iso()));
    Value::Object(out)
}

/// Resolve breadcrumb + project name from Frame.io; None on any Frame.io error.
pub async fn resolve_folder_path(
    state: &AppState,
    account_id: &str,
    project_id: &str,
    folder_id: &str,
) -> Option<(Vec<Value>, Option<String>)> {
    let client = &state.frameio;
    let ancestry = client.folder_ancestry(account_id, folder_id).await.ok()?;
    if ancestry.is_empty() {
        return None;
    }
    let mut path: Vec<Value> = ancestry
        .iter()
        .map(|i| json!({ "id": i.id, "name": i.name }))
        .collect();
    let mut project_name = None;
    if let Ok(project) = client.get_project(account_id, project_id).await {
        project_name = Some(project.name.clone());
        if let (Some(root), Some(first)) = (project.root_folder_id, path.first()) {
            if first.get("id").and_then(|v| v.as_str()) == Some(root.as_str()) {
                path[0] = json!({ "id": root, "name": format!("{} (root)", project.name) });
            }
        }
    }
    Some((path, project_name))
}

/// Refreshed copy of a frameio config's breadcrumb; None when fresh or unresolvable.
pub async fn refresh_config_path(state: &AppState, config: &Value) -> Option<Value> {
    if config.get("type").and_then(|v| v.as_str()) != Some("frameio") || path_is_fresh(config) {
        return None;
    }
    let get = |k: &str| config.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let (Some(account_id), Some(project_id), Some(folder_id)) =
        (get("account_id"), get("project_id"), get("folder_id"))
    else {
        return None;
    };
    let (path, project_name) = resolve_folder_path(state, &account_id, &project_id, &folder_id).await?;
    Some(stamp_path(config, &path, project_name.as_deref()))
}

/// Kept for parity with the Python module's error surface.
pub type Result<T> = std::result::Result<T, FrameioError>;
