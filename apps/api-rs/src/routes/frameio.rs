
//! Frame.io linking + pickers + explorer actions — `portal.routes.frameio`.
//! `/status` lives in `account.rs`; everything else is here.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::auth::{session_user_id, Admin, SessionExt};
use crate::error::detail;
use crate::frameio::connection::oauth_config;
use crate::frameio::{oauth, FrameioError};
use crate::state::AppState;

const STATE_SESSION_KEY: &str = "frameio_oauth_state";

/// Map a client error to the API response the Python routes produce.
fn picker_error(exc: FrameioError) -> Response {
    match exc {
        FrameioError::NotConnected => detail(StatusCode::CONFLICT, "Frame.io is not connected"),
        FrameioError::RateLimited => detail(StatusCode::SERVICE_UNAVAILABLE, "Frame.io is busy, try again in a moment"),
        FrameioError::NotFound => detail(StatusCode::NOT_FOUND, "Frame.io item not found"),
        _ => detail(StatusCode::BAD_GATEWAY, "Frame.io request failed"),
    }
}

fn redirect(state: &AppState, suffix: &str) -> Response {
    let url = format!("{}{}", state.settings.base_url.trim_end_matches('/'), suffix);
    let mut res = StatusCode::FOUND.into_response();
    if let Ok(v) = HeaderValue::from_str(&url) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    res
}

pub async fn connect(State(state): State<AppState>, _: Admin, SessionExt(session): SessionExt) -> Response {
    let Ok(config) = oauth_config(&state.db, &state.settings).await else {
        return redirect(&state, "/settings?frameio=error");
    };
    if config.client_id.is_empty() || config.client_secret.is_empty() {
        return redirect(&state, "/settings?frameio=unconfigured");
    }
    let st = random_token();
    session.lock().unwrap().set(STATE_SESSION_KEY, json!(st));
    let url = oauth::build_authorize_url(&config, &st);
    let mut res = StatusCode::FOUND.into_response();
    if let Ok(v) = HeaderValue::from_str(&url) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    res
}

fn random_token() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("os random");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn callback(
    State(state): State<AppState>,
    SessionExt(session): SessionExt,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    // pop the stored state, exactly like `request.session.pop(_STATE_SESSION_KEY)`.
    let expected = {
        let mut s = session.lock().unwrap();
        s.remove(STATE_SESSION_KEY).and_then(|v| v.as_str().map(|s| s.to_string()))
    };
    let Some(query_state) = q.get("state") else {
        return redirect(&state, "/settings?frameio=error");
    };
    let Some(code) = q.get("code") else {
        return redirect(&state, "/settings?frameio=error");
    };
    if q.contains_key("error") || expected.as_deref() != Some(query_state.as_str()) {
        return redirect(&state, "/settings?frameio=error");
    }

    let Some(uid) = session_user_id_from(&session) else {
        return redirect(&state, "/login");
    };
    let Ok(config) = oauth_config(&state.db, &state.settings).await else {
        return redirect(&state, "/settings?frameio=error");
    };
    let tokens = match oauth::exchange_code(&state.frameio.http_client(), &config, code).await {
        Ok(t) => t,
        Err(_) => return redirect(&state, "/settings?frameio=error"),
    };
    let identity = match oauth::fetch_identity(&state.frameio.http_client(), &tokens.access_token).await {
        Ok(i) => i,
        Err(_) => return redirect(&state, "/settings?frameio=error"),
    };
    if crate::frameio::connection::upsert_connection(&state.db, &state.settings, uid, &tokens, &identity)
        .await
        .is_err()
    {
        return redirect(&state, "/settings?frameio=error");
    }
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1, 'frameio.connected', $2)")
        .bind(uid)
        .bind(json!({ "email": identity.email }))
        .execute(&state.db)
        .await;
    redirect(&state, "/settings?frameio=connected")
}

fn session_user_id_from(session: &crate::session::SessionHandle) -> Option<uuid::Uuid> {
    let mut s = session.lock().unwrap();
    s.get("user_id")
        .and_then(|v| v.as_str())
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
}

pub async fn disconnect(State(state): State<AppState>, admin: Admin) -> Response {
    let _ = sqlx::query("DELETE FROM frameio_connections WHERE user_id = $1")
        .bind(admin.id)
        .execute(&state.db)
        .await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1, 'frameio.disconnected')")
        .bind(admin.id)
        .execute(&state.db)
        .await;
    Json(json!({ "ok": true })).into_response()
}

async fn picker(state: &AppState, f: impl std::future::Future<Output = Result<Vec<crate::frameio::PickerItem>, FrameioError>>) -> Response {
    match f.await {
        Ok(items) => Json(json!(items
            .iter()
            .map(|i| json!({ "id": i.id, "name": i.name }))
            .collect::<Vec<_>>()))
        .into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn list_accounts(State(state): State<AppState>, _: Admin) -> Response {
    picker(&state, state.frameio.list_accounts()).await
}

pub async fn list_workspaces(State(state): State<AppState>, _: Admin, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    picker(&state, state.frameio.list_workspaces(&account_id)).await
}

pub async fn list_projects(State(state): State<AppState>, _: Admin, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    let workspace_id = q.get("workspace_id").cloned().unwrap_or_default();
    match state.frameio.list_projects(&account_id, &workspace_id).await {
        Ok(items) => Json(json!(items
            .iter()
            .map(|p| json!({ "id": p.id, "name": p.name, "root_folder_id": p.root_folder_id }))
            .collect::<Vec<_>>()))
        .into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn create_project(State(state): State<AppState>, _: Admin, Json(body): Json<Value>) -> Response {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Project name is required");
    }
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let workspace_id = body.get("workspace_id").and_then(|v| v.as_str()).unwrap_or("");
    let restricted = body.get("restricted").and_then(|v| v.as_bool()).unwrap_or(false);
    match state.frameio.create_project(account_id, workspace_id, &name, restricted).await {
        Ok(p) => Json(json!({ "id": p.id, "name": p.name, "root_folder_id": p.root_folder_id })).into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn delete_project(State(state): State<AppState>, _: Admin, Path(project_id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    match state.frameio.delete_project(&account_id, &project_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn list_folders(State(state): State<AppState>, _: Admin, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    let folder_id = q.get("folder_id").cloned().unwrap_or_default();
    picker(&state, state.frameio.list_subfolders(&account_id, &folder_id)).await
}

pub async fn create_folder(State(state): State<AppState>, _: Admin, Json(body): Json<Value>) -> Response {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Folder name is required");
    }
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let parent = body.get("parent_folder_id").and_then(|v| v.as_str()).unwrap_or("");
    match state.frameio.create_folder(account_id, parent, &name).await {
        Ok(f) => Json(json!({ "id": f.id, "name": f.name })).into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn delete_folder(State(state): State<AppState>, _: Admin, Path(folder_id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    match state.frameio.delete_folder(&account_id, &folder_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn list_files(State(state): State<AppState>, _: Admin, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    let folder_id = q.get("folder_id").cloned().unwrap_or_default();
    match state.frameio.list_files(&account_id, &folder_id).await {
        Ok(items) => Json(json!(items
            .iter()
            .map(|(id, name, size, media)| json!({ "id": id, "name": name, "file_size": size, "media_type": media }))
            .collect::<Vec<_>>()))
        .into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn file_download_url(State(state): State<AppState>, _: Admin, Path(file_id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    match state.frameio.get_download_url(&account_id, &file_id).await {
        Ok(url) => Json(json!({ "url": url })).into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn delete_file(State(state): State<AppState>, _: Admin, Path(file_id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let account_id = q.get("account_id").cloned().unwrap_or_default();
    match state.frameio.delete_file(&account_id, &file_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn move_file(State(state): State<AppState>, _: Admin, Path(file_id): Path<String>, Json(body): Json<Value>) -> Response {
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let parent_id = body.get("parent_id").and_then(|v| v.as_str()).unwrap_or("");
    match state.frameio.move_file(account_id, &file_id, parent_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn copy_file(State(state): State<AppState>, _: Admin, Path(file_id): Path<String>, Json(body): Json<Value>) -> Response {
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let parent_id = body.get("parent_id").and_then(|v| v.as_str()).unwrap_or("");
    match state.frameio.copy_file(account_id, &file_id, parent_id).await {
        Ok((id, name, size, media)) => Json(json!({ "id": id, "name": name, "file_size": size, "media_type": media })).into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn move_folder(State(state): State<AppState>, _: Admin, Path(folder_id): Path<String>, Json(body): Json<Value>) -> Response {
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let parent_id = body.get("parent_id").and_then(|v| v.as_str()).unwrap_or("");
    match state.frameio.move_folder(account_id, &folder_id, parent_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn copy_folder(State(state): State<AppState>, _: Admin, Path(folder_id): Path<String>, Json(body): Json<Value>) -> Response {
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let parent_id = body.get("parent_id").and_then(|v| v.as_str()).unwrap_or("");
    match state.frameio.copy_folder(account_id, &folder_id, parent_id).await {
        Ok(f) => Json(json!({ "id": f.id, "name": f.name })).into_response(),
        Err(e) => picker_error(e),
    }
}

pub async fn create_share(State(state): State<AppState>, _: Admin, Json(body): Json<Value>) -> Response {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Share name is required");
    }
    let asset_ids: Vec<String> = body
        .get("asset_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if asset_ids.is_empty() {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Select at least one file or folder to share");
    }
    let access = body.get("access").and_then(|v| v.as_str()).unwrap_or("public").to_string();
    if access != "public" && access != "secure" {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "access must be 'public' or 'secure'");
    }
    let account_id = body.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let project_id = body.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
    let downloading = body.get("downloading_enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let expiration = body.get("expiration").and_then(|v| v.as_str()).map(|s| s.to_string());
    let passphrase = body.get("passphrase").and_then(|v| v.as_str()).map(|s| s.to_string());
    match state
        .frameio
        .create_share(account_id, project_id, &name, &asset_ids, &access, downloading, expiration.as_deref(), passphrase.as_deref())
        .await
    {
        Ok(d) => Json(json!({
            "id": d.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "name": d.get("name"),
            "short_url": d.get("short_url"),
            "access": d.get("access"),
            "passphrase": d.get("passphrase"),
            "expiration": d.get("expiration"),
            "downloading_enabled": d.get("downloading_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
            "enabled": d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        }))
        .into_response(),
        Err(e) => picker_error(e),
    }
}
