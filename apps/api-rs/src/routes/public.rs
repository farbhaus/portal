
//! Public (unauthenticated) upload-link endpoints — `portal.routes.public`.

use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::lib::{lockout, net, ratelimit};
use crate::services::runtime_config::{email_brand, get_email_config};
use crate::storage::frameio_backend::FrameioStorageBackend;
use crate::storage::{DestinationConfig, RemoteFile, UploadSession as StorageUploadSession};
use crate::sync::paths::render_session_prefix;
use crate::sync::queue::{self, RUN_SYNC_JOB};
use crate::state::AppState;
use crate::verify;

fn detail_response(status: StatusCode, message: &str) -> Response {
    crate::error::detail(status, message)
}

async fn link_row(state: &AppState, token: &str) -> Result<sqlx::postgres::PgRow, Response> {
    let row = sqlx::query(
        "SELECT l.*, d.display_name AS destination_name, d.subtitle AS destination_subtitle,
                d.config AS destination_config
           FROM upload_links l JOIN destinations d ON d.id = l.destination_id
          WHERE l.token = $1",
    )
    .bind(token)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| detail_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"))?;
    row.ok_or_else(|| detail_response(StatusCode::NOT_FOUND, "This upload link does not exist"))
}

fn link_state_of(revoked_at: Option<DateTime<Utc>>, expires_at: Option<DateTime<Utc>>) -> &'static str {
    if revoked_at.is_some() {
        return "revoked";
    }
    if let Some(exp) = expires_at {
        if exp <= Utc::now() {
            return "expired";
        }
    }
    "ok"
}

fn require_usable(row: &sqlx::postgres::PgRow) -> Result<(), Response> {
    let state = link_state_of(row.get("revoked_at"), row.get("expires_at"));
    if state != "ok" {
        return Err(detail_response(StatusCode::GONE, &format!("This link is {state}")));
    }
    Ok(())
}

fn or_else(a: Option<String>, b: Option<String>) -> Option<String> {
    match a {
        Some(s) if !s.is_empty() => Some(s),
        _ => match b {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        },
    }
}

pub async fn resolve_link(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let app = sqlx::query(
        "SELECT logo_png, logo_updated_at, brand_display_name, brand_accent_color FROM app_settings LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let (app_logo, app_name, app_accent) = match app {
        Some(r) => {
            let has_logo = r.get::<Option<Vec<u8>>, _>("logo_png").is_some();
            let updated: Option<DateTime<Utc>> = r.get("logo_updated_at");
            let logo = if has_logo {
                updated.map(|d| format!("{}/api/branding/logo?v={}", state.settings.base_url.trim_end_matches('/'), d.timestamp()))
            } else {
                None
            };
            (logo, r.get::<Option<String>, _>("brand_display_name"), r.get::<Option<String>, _>("brand_accent_color"))
        }
        None => (None, None, None),
    };
    let branding_name = or_else(row.get("brand_display_name"), row.get("destination_name"));
    let branding_subtitle = or_else(row.get("brand_subtitle"), row.get("destination_subtitle"));
    let display_name = or_else(branding_name, app_name).unwrap_or_else(|| "Upload".to_string());
    let fields: Option<Value> = row.get("uploader_fields_required");
    let fields = fields.unwrap_or_else(|| json!({}));
    let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    Json(json!({
        "state": link_state_of(row.get("revoked_at"), row.get("expires_at")),
        "display_name": display_name,
        "subtitle": branding_subtitle,
        "logo_url": app_logo,
        "accent_color": app_accent,
        "password_required": row.get::<Option<String>, _>("password_hash").is_some(),
        "max_file_size": row.get::<Option<i64>, _>("max_file_size"),
        "allowed_extensions": row.get::<Option<Value>, _>("allowed_extensions").unwrap_or(Value::Null),
        "uploader_fields_required": { "name": flag("name"), "email": flag("email"), "message": flag("message") },
        "verify_email": row.get::<bool, _>("verify_email"),
    }))
    .into_response()
}

pub async fn request_code(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let mut redis = state.redis.clone();
    if let Err(msg) = ratelimit::check(&mut redis, &state.settings, "strict", &token, &headers, Some(peer)).await {
        return detail_response(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = require_usable(&row) {
        return r;
    }
    if !row.get::<bool, _>("verify_email") {
        return detail_response(StatusCode::BAD_REQUEST, "This link does not require verification");
    }
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if email.is_empty() {
        return detail_response(StatusCode::UNPROCESSABLE_ENTITY, "Email is required");
    }
    let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
    if verify::is_trusted(&state.settings, cookies, &email) {
        return Json(json!({ "trusted": true, "sent": false })).into_response();
    }
    match verify::request_code(&state.db, &state.settings, &email).await {
        Ok(()) => Json(json!({ "trusted": false, "sent": true })).into_response(),
        Err(e) => detail_response(StatusCode::from_u16(e.status).unwrap(), &e.detail),
    }
}

pub async fn verify_link_code(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let mut redis = state.redis.clone();
    if let Err(msg) = ratelimit::check(&mut redis, &state.settings, "strict", &token, &headers, Some(peer)).await {
        return detail_response(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = require_usable(&row) {
        return r;
    }
    if !row.get::<bool, _>("verify_email") {
        return Json(json!({ "ok": true })).into_response();
    }
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if email.is_empty() {
        return detail_response(StatusCode::UNPROCESSABLE_ENTITY, "Email is required");
    }
    let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
    if verify::is_trusted(&state.settings, cookies, &email) {
        return Json(json!({ "ok": true })).into_response();
    }
    let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if !verify::verify_code(&state.db, &state.settings, &email, code).await {
        return detail_response(StatusCode::FORBIDDEN, "That code is incorrect or expired");
    }
    let cookie = verify::trust_cookie(&state.settings, &email);
    let mut res = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
        res.headers_mut().append(axum::http::header::SET_COOKIE, v);
    }
    res
}

pub async fn verify_link_password(State(state): State<AppState>, Path(token): Path<String>, Json(body): Json<Value>) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let password_hash: Option<String> = row.get("password_hash");
    if password_hash.is_none() {
        return Json(json!({ "ok": link_state_of(row.get("revoked_at"), row.get("expires_at")) == "ok" })).into_response();
    }
    let mut redis = state.redis.clone();
    if let Err(msg) = lockout::check_password_lockout(&mut redis, &state.settings, "upload", &token).await {
        return detail_response(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let state_ok = link_state_of(row.get("revoked_at"), row.get("expires_at")) == "ok";
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let ok = state_ok && crate::auth::verify_password(password, password_hash.as_deref().unwrap_or(""));
    if state_ok && !ok {
        lockout::register_password_failure(&mut redis, &state.settings, "upload", &token).await;
    }
    Json(json!({ "ok": ok })).into_response()
}

fn destination_config(config: &Value, folder_override: Option<String>) -> DestinationConfig {
    let mut data = config.clone();
    if let Some(fid) = folder_override {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("folder_id".to_string(), json!(fid));
        }
    }
    let t = data.get("type").and_then(|v| v.as_str()).unwrap_or("frameio").to_string();
    DestinationConfig::new(&t, data)
}

fn ext_allowed(path: &str, allowed: &Option<Value>) -> bool {
    let Some(arr) = allowed.as_ref().and_then(|v| v.as_array()) else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    let ext = path.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
    // Only treat as an extension when a dot is present.
    if !path.contains('.') {
        return false;
    }
    arr.iter().any(|v| v.as_str().map(|s| s.to_lowercase()) == Some(ext.clone()))
}

pub async fn start_session(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let mut redis = state.redis.clone();
    if let Err(msg) = ratelimit::check(&mut redis, &state.settings, "default", &token, &headers, Some(peer)).await {
        return detail_response(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = require_usable(&row) {
        return r;
    }
    let password_hash: Option<String> = row.get("password_hash");
    if password_hash.is_some() {
        if lockout::check_password_lockout(&mut redis, &state.settings, "upload", &token).await.is_err() {
            return detail_response(StatusCode::TOO_MANY_REQUESTS, "Too many incorrect password attempts — try again later");
        }
        let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
        if !crate::auth::verify_password(password, password_hash.as_deref().unwrap_or("")) {
            lockout::register_password_failure(&mut redis, &state.settings, "upload", &token).await;
            return detail_response(StatusCode::FORBIDDEN, "Incorrect password");
        }
    }
    let required: Value = row.get::<Option<Value>, _>("uploader_fields_required").unwrap_or_else(|| json!({}));
    let name = body.get("name").and_then(|v| v.as_str());
    let email = body.get("email").and_then(|v| v.as_str());
    let message = body.get("message").and_then(|v| v.as_str());
    let provided = [("name", name), ("email", email), ("message", message)];
    let missing: Vec<&str> = provided
        .iter()
        .filter(|(k, v)| required.get(k).and_then(|x| x.as_bool()).unwrap_or(false) && v.is_none())
        .map(|(k, _)| *k)
        .collect();
    if !missing.is_empty() {
        return detail_response(StatusCode::UNPROCESSABLE_ENTITY, &format!("Missing required field(s): {}", missing.join(", ")));
    }
    if row.get::<bool, _>("verify_email") {
        let email_str = email.unwrap_or("").trim().to_string();
        if email_str.is_empty() {
            return detail_response(StatusCode::UNPROCESSABLE_ENTITY, "Email is required");
        }
        let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !verify::is_trusted(&state.settings, cookies, &email_str) {
            let code = body.get("code").and_then(|v| v.as_str());
            match code {
                None => return detail_response(StatusCode::UNAUTHORIZED, "Verification required"),
                Some(c) => {
                    if !verify::verify_code(&state.db, &state.settings, &email_str, c).await {
                        return detail_response(StatusCode::FORBIDDEN, "That code is incorrect or expired");
                    }
                }
            }
        }
    }
    let link_id: Uuid = row.get("id");
    // upload_sessions.status has no DB default.
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO upload_sessions (upload_link_id, uploader_name, uploader_email, uploader_message, status, started_at)
         VALUES ($1,$2,$3,$4,'in_progress',now()) RETURNING id",
    )
    .bind(link_id).bind(name).bind(email).bind(message)
    .fetch_one(&state.db).await;
    match inserted {
        Ok(id) => {
            let mut res = Json(json!({ "session_id": id.to_string() })).into_response();
            if row.get::<bool, _>("verify_email") {
                if let Some(email_str) = email {
                    if let Ok(v) = axum::http::HeaderValue::from_str(&verify::trust_cookie(&state.settings, email_str)) {
                        res.headers_mut().append(axum::http::header::SET_COOKIE, v);
                    }
                }
            }
            res
        }
        Err(e) => {
            tracing::error!("public.start_session.insert_failed: {e}");
            detail_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
    }
}

async fn load_session(state: &AppState, link_id: Uuid, session_id: &str) -> Result<sqlx::postgres::PgRow, Response> {
    let Ok(sid) = Uuid::parse_str(session_id) else {
        return Err(detail_response(StatusCode::NOT_FOUND, "Upload session not found"));
    };
    let row = sqlx::query("SELECT * FROM upload_sessions WHERE id = $1")
        .bind(sid)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    match row {
        Some(r) if r.get::<Uuid, _>("upload_link_id") == link_id => Ok(r),
        _ => Err(detail_response(StatusCode::NOT_FOUND, "Upload session not found")),
    }
}

pub async fn request_file_upload(
    State(state): State<AppState>,
    Path((token, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = require_usable(&row) {
        return r;
    }
    let link_id: Uuid = row.get("id");
    let session = match load_session(&state, link_id, &session_id).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let path = body.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let size = body.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
    let allowed: Option<Value> = row.get("allowed_extensions");
    if !ext_allowed(path, &allowed) {
        return detail_response(StatusCode::BAD_REQUEST, "This file type is not allowed");
    }
    let max_size: Option<i64> = row.get("max_file_size");
    if let Some(max) = max_size {
        if size > max {
            return detail_response(StatusCode::BAD_REQUEST, "File exceeds the maximum allowed size");
        }
    }
    let uploader_name: Option<String> = session.get("uploader_name");
    let started_at: Option<DateTime<Utc>> = session.get("started_at");
    let template: Option<String> = row.get("subfolder_template");
    let prefix = render_session_prefix(template.as_deref(), uploader_name.as_deref().unwrap_or(""), started_at.map(|d| d.with_timezone(&chrono::Local)));
    let effective_path = if prefix.is_empty() { path.to_string() } else { format!("{prefix}/{path}") };

    let dest_cfg: Value = row.get("destination_config");
    let target_folder: Option<String> = row.get("target_folder_id");
    let dest = destination_config(&dest_cfg, target_folder);
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    let session_storage = match backend.create_upload_session(&dest, &effective_path, size).await {
        Ok(s) => s,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return detail_response(StatusCode::SERVICE_UNAVAILABLE, "Uploads are temporarily unavailable")
        }
        Err(e) => {
            tracing::warn!("public.upload.create_failed: {e:?}");
            return detail_response(StatusCode::BAD_GATEWAY, "Could not start the upload");
        }
    };
    let creds = backend.get_upload_credentials(&session_storage);

    let file_id = session_storage.backend_data.get("file_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let entry = json!({ "file_id": file_id, "name": path, "size": size });
    let _ = sqlx::query(
        "UPDATE upload_sessions SET frameio_asset_ids = COALESCE(frameio_asset_ids, '[]'::jsonb) || $1::jsonb,
            last_activity_at = now(), status = CASE WHEN status = 'abandoned' THEN 'in_progress' ELSE status END,
            updated_at = now() WHERE id = $2",
    )
    .bind(json!([entry]))
    .bind(session.get::<Uuid, _>("id"))
    .execute(&state.db).await;

    Json(json!({
        "file_id": file_id,
        "chunks": creds.chunks.iter().map(|c| json!({"part": c.part, "size": c.size, "url": c.url})).collect::<Vec<_>>(),
        "headers": creds.headers,
    }))
    .into_response()
}

pub async fn complete_file(
    State(state): State<AppState>,
    Path((token, session_id, file_id)): Path<(String, String, String)>,
) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let link_id: Uuid = row.get("id");
    if let Err(r) = load_session(&state, link_id, &session_id).await {
        return r;
    }
    let dest_cfg: Value = row.get("destination_config");
    let dest = destination_config(&dest_cfg, None);
    let storage_session = StorageUploadSession {
        id: file_id.clone(),
        destination: dest,
        filename: String::new(),
        size: 0,
        backend_data: json!({ "file_id": file_id }),
    };
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    match backend.complete_upload(&storage_session).await {
        Ok(_) => {}
        Err(e) => {
            if crate::storage::frameio_backend::is_not_ready(&e).is_some() {
                return Json(json!({ "complete": false, "failed": false })).into_response();
            }
            return Json(json!({ "complete": false, "failed": true })).into_response();
        }
    }
    // Start syncing this file now (dedupes by (rule, file_id) in the job table).
    let cfg = dest_cfg.as_object().cloned().unwrap_or_default();
    let account = cfg.get("account_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let folder = cfg.get("folder_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !account.is_empty() && !folder.is_empty() {
        let db = state.db.clone();
        let mut redis = state.redis.clone();
        let fid = file_id.clone();
        tokio::spawn(async move {
            let created = crate::sync::events::trigger_sync_for_upload(&db, &account, &folder, &[fid]).await;
            for job_id in created {
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
            }
        });
    }
    Json(json!({ "complete": true, "failed": false })).into_response()
}

pub async fn complete_session(
    State(state): State<AppState>,
    Path((token, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let link_id: Uuid = row.get("id");
    let session = match load_session(&state, link_id, &session_id).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let total_bytes = body.get("total_bytes").and_then(|v| v.as_i64());
    let _ = sqlx::query("UPDATE upload_sessions SET status='completed', completed_at=now(), total_bytes=$1, updated_at=now() WHERE id=$2")
        .bind(total_bytes).bind(session.get::<Uuid, _>("id")).execute(&state.db).await;

    // Snapshot the notification inputs before the session is gone.
    let entries: Vec<Value> = session.get::<Option<Value>, _>("frameio_asset_ids").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let files: Vec<crate::notify::UploadedFile> = entries.iter().map(|e| crate::notify::UploadedFile {
        name: e.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        size: e.get("size").and_then(|v| v.as_i64()),
    }).collect();
    let total = total_bytes.unwrap_or_else(|| files.iter().filter_map(|f| f.size).sum());
    let started_at: Option<DateTime<Utc>> = session.get("started_at");
    let duration = started_at.map(|s| (Utc::now() - s).num_seconds() as f64).unwrap_or(0.0);
    let link_name = row.get::<Option<String>, _>("brand_display_name")
        .filter(|s| !s.is_empty())
        .or_else(|| row.get::<Option<String>, _>("destination_name"))
        .unwrap_or_default();
    let uploader_name: Option<String> = session.get("uploader_name");
    let uploader_email: Option<String> = session.get("uploader_email");
    let uploader_message: Option<String> = session.get("uploader_message");
    let cfg = row.get::<Value, _>("destination_config").as_object().cloned().unwrap_or_default();
    let account = cfg.get("account_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let folder = cfg.get("folder_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let file_ids: Vec<String> = entries.iter().filter_map(|e| e.get("file_id").and_then(|v| v.as_str()).map(|s| s.to_string())).collect();

    // Background: the completion email + the sync trigger.
    let email_config = get_email_config(&state.db, &state.settings).await;
    let brand = email_brand(&state.db, &state.settings).await;
    tokio::spawn(async move {
        crate::notify::send_upload_completion(
            &email_config, &link_name, uploader_name.as_deref(), uploader_email.as_deref(), uploader_message.as_deref(),
            &files, total, duration, &brand,
        ).await;
    });
    if !account.is_empty() && !folder.is_empty() && !file_ids.is_empty() {
        let db = state.db.clone();
        let mut redis = state.redis.clone();
        tokio::spawn(async move {
            let created = crate::sync::events::trigger_sync_for_upload(&db, &account, &folder, &file_ids).await;
            for job_id in created {
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
            }
        });
    }
    Json(json!({ "ok": true })).into_response()
}

pub async fn session_heartbeat(
    State(state): State<AppState>,
    Path((token, session_id)): Path<(String, String)>,
    body: Option<Json<Value>>,
) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let link_id: Uuid = row.get("id");
    let session = match load_session(&state, link_id, &session_id).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let uploaded = body.as_ref().and_then(|b| b.0.get("uploaded_bytes")).and_then(|v| v.as_i64());
    let _ = sqlx::query(
        "UPDATE upload_sessions SET status = CASE WHEN status='abandoned' THEN 'in_progress' ELSE status END,
            last_activity_at = now(), uploaded_bytes = COALESCE($1, uploaded_bytes), updated_at = now() WHERE id=$2",
    )
    .bind(uploaded).bind(session.get::<Uuid, _>("id")).execute(&state.db).await;
    Json(json!({ "ok": true })).into_response()
}

pub async fn abandon_session(
    State(state): State<AppState>,
    Path((token, session_id)): Path<(String, String)>,
) -> Response {
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let link_id: Uuid = row.get("id");
    let session = match load_session(&state, link_id, &session_id).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let _ = sqlx::query("UPDATE upload_sessions SET status='abandoned', updated_at=now() WHERE id=$1 AND status='in_progress'")
        .bind(session.get::<Uuid, _>("id")).execute(&state.db).await;
    Json(json!({ "ok": true })).into_response()
}
