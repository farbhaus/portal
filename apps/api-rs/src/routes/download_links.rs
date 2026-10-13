
//! `GET /api/download-links` — the download-link list, mirroring `portal.routes.download_links`.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::Admin;
use crate::state::AppState;

fn link_state(revoked_at: Option<DateTime<Utc>>, expires_at: Option<DateTime<Utc>>) -> &'static str {
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

/// `downloads.links.validate_source` — raises a message on a malformed source.
fn validate_source(source: &Value) -> Result<(), String> {
    let account_id = source.get("account_id").and_then(|v| v.as_str());
    if account_id.map(|s| s.is_empty()).unwrap_or(true) {
        return Err("source.account_id is required".into());
    }
    match source.get("type").and_then(|v| v.as_str()) {
        Some("file") => {
            if source.get("file_id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                return Err("file source requires file_id".into());
            }
        }
        Some("folder") => {
            if source.get("folder_id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                return Err("folder source requires folder_id".into());
            }
        }
        Some("selection") => {
            let ok = source
                .get("file_ids")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !ok {
                return Err("selection source requires a non-empty file_ids list".into());
            }
        }
        other => return Err(format!("unknown source type: {:?}", other)),
    }
    Ok(())
}

fn generate_token() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("os random");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

const LINK_COLUMNS: &str = "SELECT id, token, source, expires_at, password_hash, max_downloads,
        downloads_count, viewer_fields_required, verify_email, allow_preview, brand_display_name,
        brand_subtitle, revoked_at, created_at
   FROM download_links";

fn render_row(state: &AppState, row: &sqlx::postgres::PgRow) -> Value {
    let token: String = row.get("token");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let created_at: DateTime<Utc> = row.get("created_at");
    let fields: Option<Value> = row.get("viewer_fields_required");
    let fields = fields.unwrap_or_else(|| json!({}));
    let verify_email: bool = row.get("verify_email");
    let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let base = state.settings.base_url.trim_end_matches('/');
    json!({
        "id": row.get::<uuid::Uuid, _>("id").to_string(),
        "token": token,
        "public_url": format!("{base}/d/{token}"),
        "state": link_state(revoked_at, expires_at),
        "source": row.get::<Value, _>("source"),
        "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
        "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
        "max_downloads": row.get::<Option<i32>, _>("max_downloads"),
        "downloads_count": row.get::<i32, _>("downloads_count"),
        "viewer_fields_required": { "name": flag("name"), "email": flag("email") || verify_email },
        "verify_email": verify_email,
        "allow_preview": row.get::<bool, _>("allow_preview"),
        "brand_display_name": row.get::<Option<String>, _>("brand_display_name"),
        "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
        "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
        "created_at": crate::pyfmt::iso(&created_at),
    })
}

async fn fetch_out(state: &AppState, id: uuid::Uuid) -> Option<Value> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{LINK_COLUMNS} WHERE id = $1")))
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()?;
    Some(render_row(state, &row))
}

fn not_found() -> Response {
    crate::error::detail(axum::http::StatusCode::NOT_FOUND, "Download link not found")
}

pub async fn get_link(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn create_link(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let source = body.get("source").cloned().unwrap_or_else(|| json!({}));
    if let Err(msg) = validate_source(&source) {
        return crate::error::detail(axum::http::StatusCode::BAD_REQUEST, &msg);
    }
    let verify_email = body.get("verify_email").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut fields = body.get("viewer_fields_required").cloned().unwrap_or_else(|| json!({}));
    if verify_email {
        fields["email"] = json!(true);
    }
    let password = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    let password_hash = password.and_then(|p| bcrypt::hash(p, bcrypt::DEFAULT_COST).ok());
    let token = generate_token();
    let inserted = sqlx::query(
        // downloads_count has no DB default (SQLAlchemy sets it at the model level).
        "INSERT INTO download_links (token, source, expires_at, password_hash, max_downloads,
            viewer_fields_required, verify_email, allow_preview, brand_display_name, brand_subtitle,
            downloads_count)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,0) RETURNING id",
    )
    .bind(&token)
    .bind(&source)
    .bind(body.get("expires_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc)))
    .bind(password_hash)
    .bind(body.get("max_downloads").and_then(|v| v.as_i64()).map(|v| v as i32))
    .bind(&fields)
    .bind(verify_email)
    .bind(body.get("allow_preview").and_then(|v| v.as_bool()).unwrap_or(false))
    .bind(body.get("brand_display_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
    .bind(body.get("brand_subtitle").and_then(|v| v.as_str()).map(|s| s.to_string()))
    .fetch_one(&state.db)
    .await;
    let id: uuid::Uuid = match inserted {
        Ok(r) => r.get("id"),
        Err(e) => {
            tracing::error!("download_links.create_failed: {e}");
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'download_link.created',$2)")
        .bind(admin.id)
        .bind(json!({ "download_link_id": id.to_string(), "source_type": source.get("type") }))
        .execute(&state.db).await;
    match fetch_out(&state, id).await {
        Some(v) => (axum::http::StatusCode::CREATED, Json(v)).into_response(),
        None => not_found(),
    }
}

pub async fn update_link(State(state): State<AppState>, admin: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>, Json(body): Json<serde_json::Map<String, Value>>) -> Response {
    let Some(existing) = fetch_out(&state, id).await else { return not_found() };
    if let Some(new_source) = body.get("source") {
        if Some(new_source) != existing.get("source") {
            if let Err(msg) = validate_source(new_source) {
                return crate::error::detail(axum::http::StatusCode::BAD_REQUEST, &msg);
            }
            let from_type = existing.get("source").and_then(|s| s.get("type")).cloned();
            let _ = sqlx::query("UPDATE download_links SET source = $1 WHERE id = $2").bind(new_source).bind(id).execute(&state.db).await;
            let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'download_link.source_changed',$2)")
                .bind(admin.id)
                .bind(json!({ "download_link_id": id.to_string(), "from_type": from_type, "to_type": new_source.get("type") }))
                .execute(&state.db).await;
        }
    }
    if body.contains_key("password") {
        let pw = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
        let hash = pw.and_then(|p| bcrypt::hash(p, bcrypt::DEFAULT_COST).ok());
        let _ = sqlx::query("UPDATE download_links SET password_hash = $1 WHERE id = $2").bind(hash).bind(id).execute(&state.db).await;
    }
    if body.contains_key("viewer_fields_required") {
        let _ = sqlx::query("UPDATE download_links SET viewer_fields_required = $1 WHERE id = $2")
            .bind(body.get("viewer_fields_required").cloned().unwrap_or(json!({})))
            .bind(id).execute(&state.db).await;
    }
    for key in ["expires_at", "max_downloads", "verify_email", "allow_preview", "brand_display_name", "brand_subtitle"] {
        if body.contains_key(key) {
            let v = body.get(key).unwrap();
            match key {
                "expires_at" => {
                    let dt = v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc));
                    let _ = sqlx::query("UPDATE download_links SET expires_at = $1 WHERE id = $2").bind(dt).bind(id).execute(&state.db).await;
                }
                "max_downloads" => { let _ = sqlx::query("UPDATE download_links SET max_downloads = $1 WHERE id = $2").bind(v.as_i64().map(|x| x as i32)).bind(id).execute(&state.db).await; }
                "verify_email" => { let _ = sqlx::query("UPDATE download_links SET verify_email = $1 WHERE id = $2").bind(v.as_bool().unwrap_or(false)).bind(id).execute(&state.db).await; }
                "allow_preview" => { let _ = sqlx::query("UPDATE download_links SET allow_preview = $1 WHERE id = $2").bind(v.as_bool().unwrap_or(false)).bind(id).execute(&state.db).await; }
                _ => { let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE download_links SET {key} = $1 WHERE id = $2"))).bind(v.as_str()).bind(id).execute(&state.db).await; }
            }
        }
    }
    let verify: Option<bool> = sqlx::query_scalar("SELECT verify_email FROM download_links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if verify == Some(true) {
        let _ = sqlx::query("UPDATE download_links SET viewer_fields_required = jsonb_set(COALESCE(viewer_fields_required,'{}'::jsonb), '{email}', 'true') WHERE id = $1").bind(id).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn revoke_link(State(state): State<AppState>, admin: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() { return not_found(); }
    let already: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT revoked_at FROM download_links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if already.is_none() {
        let _ = sqlx::query("UPDATE download_links SET revoked_at = now() WHERE id = $1").bind(id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'download_link.revoked',$2)")
            .bind(admin.id).bind(json!({ "download_link_id": id.to_string() })).execute(&state.db).await;
    }
    match fetch_out(&state, id).await { Some(v) => Json(v).into_response(), None => not_found() }
}

pub async fn delete_link(State(state): State<AppState>, admin: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() { return not_found(); }
    let _ = sqlx::query("DELETE FROM download_links WHERE id = $1").bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'download_link.deleted',$2)")
        .bind(admin.id).bind(json!({ "download_link_id": id.to_string() })).execute(&state.db).await;
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub async fn list_events(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>, axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>) -> Response {
    if fetch_out(&state, id).await.is_none() { return not_found(); }
    let limit = q.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query(
        "SELECT e.frameio_file_id, e.file_name, e.completed, e.started_at, e.ip, e.user_agent,
                s.viewer_name, s.viewer_email, s.ip AS sess_ip, s.user_agent AS sess_ua
           FROM download_events e JOIN download_sessions s ON e.download_session_id = s.id
          WHERE s.download_link_id = $1 ORDER BY e.started_at DESC LIMIT $2",
    ).bind(id).bind(limit).fetch_all(&state.db).await.unwrap_or_default();
    let out: Vec<Value> = rows.iter().map(|r| {
        let ip: Option<String> = r.get::<Option<String>,_>("ip").or_else(|| r.get("sess_ip"));
        let ua: Option<String> = r.get::<Option<String>,_>("user_agent").or_else(|| r.get("sess_ua"));
        json!({
            "frameio_file_id": r.get::<String,_>("frameio_file_id"),
            "file_name": r.get::<Option<String>,_>("file_name"),
            "viewer_name": r.get::<Option<String>,_>("viewer_name"),
            "viewer_email": r.get::<Option<String>,_>("viewer_email"),
            "ip": ip,
            "user_agent": ua,
            "completed": r.get::<bool,_>("completed"),
            "started_at": crate::pyfmt::iso(&r.get::<DateTime<Utc>,_>("started_at")),
        })
    }).collect();
    Json(Value::Array(out)).into_response()
}

pub async fn stats(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() { return not_found(); }
    let downloads: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM download_events e JOIN download_sessions s ON e.download_session_id = s.id WHERE s.download_link_id = $1",
    ).bind(id).fetch_one(&state.db).await.unwrap_or(0);
    let unique: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT COALESCE(s.viewer_email, s.ip)) FROM download_sessions s WHERE s.download_link_id = $1",
    ).bind(id).fetch_one(&state.db).await.unwrap_or(0);
    let last: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(started_at) FROM download_sessions WHERE download_link_id = $1",
    ).bind(id).fetch_optional(&state.db).await.ok().flatten();
    Json(json!({ "downloads": downloads, "unique_viewers": unique, "last_activity": last.map(|d| crate::pyfmt::iso(&d)) })).into_response()
}

pub async fn list_links(State(state): State<AppState>, _: Admin) -> Response {
    let rows = sqlx::query(
        "SELECT id, token, source, expires_at, password_hash, max_downloads, downloads_count,
                viewer_fields_required, verify_email, allow_preview, brand_display_name,
                brand_subtitle, revoked_at, created_at
           FROM download_links ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("download_links.list_failed: {e}");
            return crate::error::detail(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            );
        }
    };

    let base = state.settings.base_url.trim_end_matches('/').to_string();
    let mut out: Vec<Value> = Vec::with_capacity(rows.len());
    for row in rows {
        let token: String = row.get("token");
        let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
        let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
        let created_at: DateTime<Utc> = row.get("created_at");
        let fields: Option<Value> = row.get("viewer_fields_required");
        let fields = fields.unwrap_or_else(|| json!({}));
        let verify_email: bool = row.get("verify_email");
        let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

        out.push(json!({
            "id": row.get::<uuid::Uuid, _>("id").to_string(),
            "token": token,
            "public_url": format!("{base}/d/{token}"),
            "state": link_state(revoked_at, expires_at),
            "source": row.get::<Value, _>("source"),
            "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
            "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
            "max_downloads": row.get::<Option<i32>, _>("max_downloads"),
            "downloads_count": row.get::<i32, _>("downloads_count"),
            "viewer_fields_required": {
                "name": flag("name"),
                "email": flag("email") || verify_email,
            },
            "verify_email": verify_email,
            "allow_preview": row.get::<bool, _>("allow_preview"),
            "brand_display_name": row.get::<Option<String>, _>("brand_display_name"),
            "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
            "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
            "created_at": crate::pyfmt::iso(&created_at),
        }));
    }
    Json(Value::Array(out)).into_response()
}

fn generic_label(source: &Value) -> String {
    match source.get("type").and_then(|v| v.as_str()) {
        Some("file") => "Single file".to_string(),
        Some("folder") => {
            if source.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false) {
                "Folder (recursive)".to_string()
            } else {
                "Folder".to_string()
            }
        }
        Some("selection") => format!(
            "{} files",
            source.get("file_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
        ),
        _ => "—".to_string(),
    }
}

/// `GET /api/download-links/{link_id}/source` — best-effort human-readable source for the editor.
pub async fn source_info(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    let Some(link) = fetch_out(&state, id).await else { return not_found() };
    let source = link.get("source").cloned().unwrap_or_else(|| json!({}));
    let stype = source.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let account_id = source.get("account_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let result: Option<Value> = match stype.as_str() {
        "folder" => {
            let folder_id = source.get("folder_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            match state.frameio.get_folder(&account_id, &folder_id).await {
                Ok(folder) => {
                    let ancestry = state.frameio.folder_ancestry(&account_id, &folder_id).await.unwrap_or_default();
                    let project = match &folder.project_id {
                        Some(pid) => state.frameio.get_project(&account_id, pid).await.ok(),
                        None => None,
                    };
                    let mut path: Vec<Value> = ancestry.iter().map(|i| json!({ "id": i.id, "name": i.name })).collect();
                    if let Some(p) = &project {
                        if let Some(root) = &p.root_folder_id {
                            if path.first().and_then(|s| s.get("id")).and_then(|v| v.as_str()) == Some(root.as_str()) {
                                path[0] = json!({ "id": root, "name": format!("{} (root)", p.name) });
                            }
                        }
                    }
                    let names: Vec<String> = path.iter().filter_map(|p| p.get("name").and_then(|v| v.as_str()).map(|s| s.to_string())).collect();
                    let names = if names.is_empty() { vec![folder_id.clone()] } else { names };
                    let full = names.join("/");
                    Some(json!({
                        "type": "folder",
                        "label": full,
                        "account_id": account_id,
                        "workspace_id": project.as_ref().and_then(|p| p.workspace_id.clone()),
                        "project_id": folder.project_id,
                        "project_name": project.as_ref().map(|p| p.name.clone()),
                        "folder_id": folder_id,
                        "folder_name": ancestry.last().map(|i| i.name.clone()).unwrap_or(folder_id.clone()),
                        "folder_path": full,
                        "path": path,
                        "recursive": source.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false),
                        "resolved": true,
                    }))
                }
                Err(_) => None,
            }
        }
        "file" => {
            let file_id = source.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
            match state.frameio.get_file(&account_id, file_id).await {
                Ok(f) => {
                    let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some(json!({ "type": "file", "label": name, "files": [name], "resolved": true }))
                }
                Err(_) => None,
            }
        }
        "selection" => match crate::downloads::resolve_source(&state.frameio, &source).await {
            Ok(resolved) => {
                let names: Vec<String> = resolved.files.iter().map(|f| f.name.clone()).collect();
                Some(json!({ "type": "selection", "label": format!("{} files", names.len()), "files": names, "resolved": true }))
            }
            Err(_) => None,
        },
        _ => None,
    };

    match result {
        Some(v) => Json(v).into_response(),
        None => Json(json!({
            "type": if stype.is_empty() { "unknown".to_string() } else { stype },
            "label": generic_label(&source),
            "account_id": Value::Null,
            "workspace_id": Value::Null,
            "project_id": Value::Null,
            "project_name": Value::Null,
            "folder_id": Value::Null,
            "folder_name": Value::Null,
            "folder_path": Value::Null,
            "path": Value::Null,
            "recursive": Value::Null,
            "files": Value::Null,
            "resolved": false,
        })).into_response(),
    }
}
