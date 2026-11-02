
//! Public (unauthenticated) download-link endpoints — `portal.routes.public_downloads`.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::downloads::{branding, resolve_source};
use crate::common::{lockout, net, ratelimit};
use crate::state::AppState;
use crate::storage::frameio_backend::FrameioStorageBackend;
use crate::storage::DestinationConfig;
use crate::verify;

/// What a send link holds, known without asking Frame.io: links made from the picker store their
/// files' count and total size. None for folders (their contents can change).
fn source_summary(source: &Value) -> Value {
    let count = match source.get("type").and_then(|v| v.as_str()) {
        Some("file") => 1,
        Some("selection") => source.get("file_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        _ => return Value::Null,
    };
    json!({ "files": count, "bytes": source.get("bytes").and_then(|v| v.as_i64()) })
}

async fn link_row(state: &AppState, token: &str) -> Result<sqlx::postgres::PgRow, Response> {
    let row = sqlx::query("SELECT * FROM links WHERE kind = 'download' AND token = $1")
        .bind(token)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"))?;
    row.ok_or_else(|| crate::error::detail(StatusCode::NOT_FOUND, "This download link does not exist"))
}

fn link_state_of(revoked_at: Option<DateTime<Utc>>, expires_at: Option<DateTime<Utc>>) -> &'static str {
    if revoked_at.is_some() {
        return "revoked";
    }
    if let Some(exp) = expires_at
        && exp <= Utc::now() {
            return "expired";
        }
    "ok"
}

fn require_usable(row: &sqlx::postgres::PgRow) -> Result<(), Response> {
    let state = link_state_of(row.get("revoked_at"), row.get("expires_at"));
    if state != "ok" {
        return Err(crate::error::detail(StatusCode::GONE, &format!("This link is {state}")));
    }
    Ok(())
}

pub async fn resolve_link(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let mut redis = state.redis.clone();
    if let Err(msg) = ratelimit::check(&mut redis, &state.settings, "default", &token, &headers, Some(peer)).await {
        return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let (display_name, subtitle) = branding(
        &row.get::<Option<String>, _>("brand_display_name"),
        &row.get::<Option<String>, _>("brand_subtitle"),
    );
    let brand = crate::services::runtime_config::link_brand(&state.db, &state.settings).await;
    // A link without its own title takes its brand's name (the downloads `branding` helper's
    // generic fallback otherwise).
    let display_name = row
        .get::<Option<String>, _>("brand_display_name")
        .filter(|s| !s.trim().is_empty())
        .or(brand.name.clone())
        .unwrap_or(display_name);
    let fields: Value = row.get::<Option<Value>, _>("viewer_fields_required").unwrap_or_else(|| json!({}));
    let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    Json(json!({
        "state": link_state_of(row.get("revoked_at"), row.get("expires_at")),
        "display_name": display_name,
        "subtitle": subtitle,
        "logo_url": brand.logo_url,
        "accent_color": brand.accent,
        "password_required": row.get::<Option<String>, _>("password_hash").is_some(),
        "viewer_fields_required": { "name": flag("name"), "email": flag("email") },
        "verify_email": row.get::<bool, _>("verify_email"),
        "allow_preview": row.get::<bool, _>("allow_preview"),
        "summary": source_summary(&row.get::<Value, _>("source")),
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
        return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await { Ok(r) => r, Err(r) => return r };
    if let Err(r) = require_usable(&row) { return r; }
    if !row.get::<bool, _>("verify_email") {
        return crate::error::detail(StatusCode::BAD_REQUEST, "This link does not require verification");
    }
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if email.is_empty() {
        return crate::error::detail(StatusCode::UNPROCESSABLE_ENTITY, "Email is required");
    }
    let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
    if verify::is_trusted(&state.settings, cookies, &email) {
        return Json(json!({ "trusted": true, "sent": false })).into_response();
    }
    match verify::request_code(&state.db, &state.settings, &email).await {
        Ok(()) => Json(json!({ "trusted": false, "sent": true })).into_response(),
        Err(e) => crate::error::detail(StatusCode::from_u16(e.status).unwrap(), &e.detail),
    }
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
        return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, &msg);
    }
    let row = match link_row(&state, &token).await { Ok(r) => r, Err(r) => return r };
    if let Err(r) = require_usable(&row) { return r; }
    let password_hash: Option<String> = row.get("password_hash");
    if let Some(hash) = &password_hash {
        if lockout::check_password_lockout(&mut redis, &state.settings, "download", &token).await.is_err() {
            return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, "Too many incorrect password attempts — try again later");
        }
        let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
        if !crate::auth::verify_password(password, hash) {
            lockout::register_password_failure(&mut redis, &state.settings, "download", &token).await;
            return crate::error::detail(StatusCode::FORBIDDEN, "Incorrect password");
        }
    }
    let required: Value = row.get::<Option<Value>, _>("viewer_fields_required").unwrap_or_else(|| json!({}));
    let name = body.get("name").and_then(|v| v.as_str());
    let email = body.get("email").and_then(|v| v.as_str());
    let missing: Vec<&str> = [("name", name), ("email", email)]
        .iter()
        .filter(|(k, v)| required.get(*k).and_then(|x| x.as_bool()).unwrap_or(false) && v.is_none())
        .map(|(k, _)| *k)
        .collect();
    if !missing.is_empty() {
        return crate::error::detail(StatusCode::UNPROCESSABLE_ENTITY, &format!("Missing required field(s): {}", missing.join(", ")));
    }
    if row.get::<bool, _>("verify_email") {
        let email_str = email.unwrap_or("").trim().to_string();
        if email_str.is_empty() {
            return crate::error::detail(StatusCode::UNPROCESSABLE_ENTITY, "Email is required");
        }
        let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !verify::is_trusted(&state.settings, cookies, &email_str) {
            match body.get("code").and_then(|v| v.as_str()) {
                None => return crate::error::detail(StatusCode::UNAUTHORIZED, "Verification required"),
                Some(c) => {
                    if !verify::verify_code(&state.db, &state.settings, &email_str, c).await {
                        return crate::error::detail(StatusCode::FORBIDDEN, "That code is incorrect or expired");
                    }
                }
            }
        }
    }

    let source: Value = row.get("source");
    // Python indexes `source["account_id"]` directly, so a malformed source is a 500 there.
    if source.get("account_id").and_then(|v| v.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
        tracing::error!("public.download.malformed_source");
        return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    }
    let resolved = match resolve_source(&state.frameio, &source).await {
        Ok(r) => r,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return crate::error::detail(StatusCode::SERVICE_UNAVAILABLE, "Downloads are temporarily unavailable")
        }
        Err(e) => {
            tracing::warn!("public.download.resolve_failed: {e:?}");
            return crate::error::detail(StatusCode::BAD_GATEWAY, "Could not load the files");
        }
    };

    let link_id: Uuid = row.get("id");
    let allow_preview = row.get::<bool, _>("allow_preview");
    let snapshot: Value = Value::Array(resolved.files.iter().map(|f| json!({
        "id": f.id, "name": f.name, "size": f.file_size, "path": f.path
    })).collect());
    let ip = net::client_ip(&headers, Some(peer), &state.settings);
    let user_agent = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO download_sessions (download_link_id, viewer_name, viewer_email, ip, user_agent, started_at, files)
         VALUES ($1,$2,$3,$4,$5,now(),$6) RETURNING id",
    )
    .bind(link_id).bind(name).bind(email).bind(&ip).bind(&user_agent).bind(&snapshot)
    .fetch_one(&state.db).await;
    let session_id = match inserted {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("public.download.start_failed: {e}");
            return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    let files: Vec<Value> = resolved.files.iter().map(|f| json!({
        "id": f.id,
        "name": f.name,
        "size": f.file_size,
        "media_type": f.media_type,
        "thumbnail_url": if allow_preview { f.thumbnail_url.clone() } else { None },
        "path": f.path,
        "ready": f.ready(),
    })).collect();
    let mut res = Json(json!({ "session_id": session_id.to_string(), "files": files })).into_response();
    if row.get::<bool, _>("verify_email")
        && let Some(email_str) = email
            && let Ok(v) = axum::http::HeaderValue::from_str(&verify::trust_cookie(&state.settings, email_str)) {
                res.headers_mut().append(axum::http::header::SET_COOKIE, v);
            }
    res
}

async fn load_session(state: &AppState, link_id: Uuid, session_id: &str) -> Result<sqlx::postgres::PgRow, Response> {
    let Ok(sid) = Uuid::parse_str(session_id) else {
        return Err(crate::error::detail(StatusCode::NOT_FOUND, "Download session not found"));
    };
    let row = sqlx::query("SELECT * FROM download_sessions WHERE id = $1")
        .bind(sid).fetch_optional(&state.db).await.ok().flatten();
    match row {
        Some(r) if r.get::<Uuid, _>("download_link_id") == link_id => Ok(r),
        _ => Err(crate::error::detail(StatusCode::NOT_FOUND, "Download session not found")),
    }
}

pub async fn file_download_url(
    State(state): State<AppState>,
    Path((token, session_id, file_id)): Path<(String, String, String)>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let row = match link_row(&state, &token).await { Ok(r) => r, Err(r) => return r };
    if let Err(r) = require_usable(&row) { return r; }
    let link_id: Uuid = row.get("id");
    let session = match load_session(&state, link_id, &session_id).await { Ok(s) => s, Err(r) => return r };

    let max_downloads: Option<i32> = row.get("max_downloads");
    let downloads_count: i32 = row.get("downloads_count");
    if let Some(max) = max_downloads
        && downloads_count >= max {
            return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, "This link's download limit has been reached");
        }

    // The recipient may only fetch a file that was in their session's resolved listing; that
    // snapshot also carries the name/size for the download event. Sessions created before the
    // snapshot column existed fall back to a live resolve.
    let source: Value = row.get("source");
    let files: Option<Value> = session.get::<Option<Value>, _>("files").filter(|v| !v.is_null());
    let mut matched = files
        .as_ref()
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|f| f.get("id").and_then(|i| i.as_str()) == Some(file_id.as_str())))
        .cloned();
    if matched.is_none() && files.is_none() {
        match crate::downloads::resolve_file(&state.frameio, &source, &file_id).await {
            Ok(found) => matched = found.map(|f| json!({ "id": f.id, "name": f.name, "size": f.file_size })),
            Err(crate::frameio::FrameioError::NotConnected) => {
                return crate::error::detail(StatusCode::SERVICE_UNAVAILABLE, "Downloads are temporarily unavailable")
            }
            Err(e) => {
                tracing::warn!("public.download.resolve_failed: {e:?}");
                return crate::error::detail(StatusCode::BAD_GATEWAY, "Could not generate the download");
            }
        }
    }
    let Some(matched) = matched else {
        return crate::error::detail(StatusCode::NOT_FOUND, "File not found in this link");
    };

    let account_id = source.get("account_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dest = DestinationConfig::new("frameio", json!({ "account_id": account_id }));
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    let download = match backend.get_download_url(&dest, &file_id).await {
        Ok(d) => d,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return crate::error::detail(StatusCode::SERVICE_UNAVAILABLE, "Downloads are temporarily unavailable")
        }
        Err(e) => {
            tracing::warn!("public.download.url_failed: {e:?}");
            return crate::error::detail(StatusCode::BAD_GATEWAY, "Could not generate the download");
        }
    };

    // Atomically claim one download against the cap.
    let claimed: Option<Uuid> = sqlx::query_scalar(
        "UPDATE links SET downloads_count = downloads_count + 1, updated_at = now()
          WHERE id = $1 AND (max_downloads IS NULL OR downloads_count < max_downloads) RETURNING id",
    )
    .bind(link_id).fetch_optional(&state.db).await.ok().flatten();
    if claimed.is_none() {
        return crate::error::detail(StatusCode::TOO_MANY_REQUESTS, "This link's download limit has been reached");
    }

    let ip = net::client_ip(&headers, Some(peer), &state.settings);
    let user_agent = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let file_name = matched.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
    let file_size = matched.get("size").and_then(|v| v.as_i64());
    let _ = sqlx::query(
        "INSERT INTO download_events (download_session_id, frameio_file_id, file_name, bytes_served, completed, ip, user_agent)
         VALUES ($1,$2,$3,$4,true,$5,$6)",
    )
    .bind(session.get::<Uuid, _>("id")).bind(&file_id).bind(file_name).bind(file_size).bind(&ip).bind(&user_agent)
    .execute(&state.db).await;

    let mut res = Json(json!({ "url": download.url })).into_response();
    // Slide the device trust window on real activity — only for an address this browser already
    // verified. A session started before the link switched verification on carries an unverified
    // email, and minting trust for it would let it pass upload links' verification too.
    let viewer_email: Option<String> = session.get("viewer_email");
    if let Some(email) = viewer_email.filter(|e| !e.is_empty()) {
        let cookies = headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
        if row.get::<bool, _>("verify_email") && verify::is_trusted(&state.settings, cookies, &email)
            && let Ok(v) = axum::http::HeaderValue::from_str(&verify::trust_cookie(&state.settings, &email)) {
                res.headers_mut().append(axum::http::header::SET_COOKIE, v);
            }
    }
    res
}
