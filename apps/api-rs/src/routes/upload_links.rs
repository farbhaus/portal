
//! Admin upload-link list — `portal.routes.upload_links` (`GET /api/upload-links`).
//!
//! Only the list endpoint is ported here; create/update/delete land with the rest of the admin
//! CRUD. Field order and datetime formatting are part of the byte contract (see `pyfmt::iso`).

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::auth::Admin;

/// `MEDIA_EXTENSIONS_PRESET` from `portal.uploads.links`.
pub const MEDIA_EXTENSIONS_PRESET: [&str; 26] = [
    "r3d", "braw", "ari", "arx", "mxf", "mov", "mp4", "mts", "m4v", "avi", "prores", "dng",
    "cdng", "exr", "dpx", "tiff", "tif", "png", "jpg", "jpeg", "wav", "aif", "aiff", "mp3",
    "bwf", "zip",
];
use crate::error::detail;
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

pub async fn extension_preset(_: Admin) -> Response {
    Json(serde_json::json!(MEDIA_EXTENSIONS_PRESET)).into_response()
}

/// `secrets.token_urlsafe(24)` — 32 urlsafe base64 chars, no padding.
fn generate_token() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("os random");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `normalize_extensions`: lowercase, strip leading dots, dedupe preserving order; empty -> None.
fn normalize_extensions(v: Option<&Value>) -> Option<Value> {
    let arr = v?.as_array()?;
    let mut seen: Vec<String> = Vec::new();
    for ext in arr {
        let Some(s) = ext.as_str() else { continue };
        let cleaned = s.trim().trim_start_matches('.').to_lowercase();
        if !cleaned.is_empty() && !seen.contains(&cleaned) {
            seen.push(cleaned);
        }
    }
    if seen.is_empty() {
        None
    } else {
        Some(Value::Array(seen.into_iter().map(Value::String).collect()))
    }
}

fn str_or_none(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

const LINK_COLUMNS: &str = "SELECT l.id, l.token, l.expires_at, l.password_hash, l.max_file_size,
        l.verify_email, l.allowed_extensions, l.uploader_fields_required, l.revoked_at,
        l.brand_display_name, l.brand_subtitle, l.created_at, l.target_folder_id,
        l.target_folder_name, l.subfolder_template, l.destination_id,
        d.display_name AS destination_name, d.config AS destination_config
   FROM upload_links l JOIN destinations d ON d.id = l.destination_id";

/// Render one link's `UploadLinkOut`, or None if it doesn't exist.
async fn fetch_out(state: &AppState, id: uuid::Uuid) -> Option<Value> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{LINK_COLUMNS} WHERE l.id = $1")))
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()?;
    Some(render_row(state, &row))
}

fn render_row(state: &AppState, row: &sqlx::postgres::PgRow) -> Value {
    let token: String = row.get("token");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let verify_email: bool = row.get("verify_email");
    let fields: Option<Value> = row.get("uploader_fields_required");
    let fields = fields.unwrap_or_else(|| json!({}));
    let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let email = flag("email") || verify_email;
    let dest_config: Value = row.get("destination_config");
    let created_at: DateTime<Utc> = row.get("created_at");
    let base = state.settings.base_url.trim_end_matches('/');
    json!({
        "id": row.get::<uuid::Uuid, _>("id").to_string(),
        "token": token,
        "public_url": format!("{base}/u/{token}"),
        "state": link_state(revoked_at, expires_at),
        "destination_id": row.get::<uuid::Uuid, _>("destination_id").to_string(),
        "destination_name": row.get::<String, _>("destination_name"),
        "destination_path": dest_config.get("path").cloned().unwrap_or(Value::Null),
        "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
        "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
        "max_file_size": row.get::<Option<i64>, _>("max_file_size"),
        "allowed_extensions": row.get::<Option<Value>, _>("allowed_extensions").unwrap_or(Value::Null),
        "uploader_fields_required": { "name": flag("name"), "email": email, "message": flag("message") },
        "verify_email": verify_email,
        "target_folder_id": row.get::<Option<String>, _>("target_folder_id"),
        "target_folder_name": row.get::<Option<String>, _>("target_folder_name"),
        "subfolder_template": row.get::<Option<String>, _>("subfolder_template"),
        "brand_display_name": row.get::<Option<String>, _>("brand_display_name"),
        "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
        "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
        "created_at": crate::pyfmt::iso(&created_at),
    })
}

fn not_found() -> Response {
    crate::error::detail(axum::http::StatusCode::NOT_FOUND, "Upload link not found")
}

pub async fn get_link(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn create_link(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let Some(dest_id) = body.get("destination_id").and_then(|v| v.as_str()).and_then(|v| uuid::Uuid::parse_str(v).ok()) else {
        return crate::error::detail(axum::http::StatusCode::UNPROCESSABLE_ENTITY, "Field required");
    };
    let exists: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM destinations WHERE id = $1")
        .bind(dest_id).fetch_optional(&state.db).await.ok().flatten();
    if exists.is_none() {
        return crate::error::detail(axum::http::StatusCode::BAD_REQUEST, "Destination not found");
    }
    let password = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    let password_hash = match password {
        Some(p) => match bcrypt::hash(p, bcrypt::DEFAULT_COST) { Ok(h) => Some(h), Err(_) => None },
        None => None,
    };
    let verify_email = body.get("verify_email").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut fields = body.get("uploader_fields_required").cloned().unwrap_or_else(|| json!({}));
    if verify_email {
        fields["email"] = json!(true);
    }
    let token = generate_token();
    let inserted = sqlx::query(
        "INSERT INTO upload_links (token, destination_id, expires_at, password_hash, max_file_size,
            allowed_extensions, uploader_fields_required, verify_email, target_folder_id,
            target_folder_name, subfolder_template, brand_display_name, brand_subtitle)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id",
    )
    .bind(&token)
    .bind(dest_id)
    .bind(body.get("expires_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc)))
    .bind(password_hash)
    .bind(body.get("max_file_size").and_then(|v| v.as_i64()))
    .bind(normalize_extensions(body.get("allowed_extensions")))
    .bind(&fields)
    .bind(verify_email)
    .bind(str_or_none(body.get("target_folder_id")))
    .bind(str_or_none(body.get("target_folder_name")))
    .bind(str_or_none(body.get("subfolder_template")))
    .bind(body.get("brand_display_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
    .bind(body.get("brand_subtitle").and_then(|v| v.as_str()).map(|s| s.to_string()))
    .fetch_one(&state.db)
    .await;
    let id: uuid::Uuid = match inserted {
        Ok(r) => r.get("id"),
        Err(e) => {
            tracing::error!("upload_links.create_failed: {e}");
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.created',$2)")
        .bind(admin.id)
        .bind(json!({ "upload_link_id": id.to_string(), "destination_id": dest_id.to_string() }))
        .execute(&state.db).await;
    match fetch_out(&state, id).await {
        Some(v) => (axum::http::StatusCode::CREATED, Json(v)).into_response(),
        None => not_found(),
    }
}

pub async fn update_link(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>, Json(body): Json<Map<String, Value>>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    // `exclude_unset`: only keys present in the body are applied.
    if let Some(v) = body.get("destination_id").and_then(|v| v.as_str()).and_then(|v| uuid::Uuid::parse_str(v).ok()) {
        let exists: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM destinations WHERE id = $1")
            .bind(v).fetch_optional(&state.db).await.ok().flatten();
        if exists.is_none() {
            return crate::error::detail(axum::http::StatusCode::BAD_REQUEST, "Destination not found");
        }
        let _ = sqlx::query("UPDATE upload_links SET destination_id = $1 WHERE id = $2").bind(v).bind(id).execute(&state.db).await;
    }
    if body.contains_key("password") {
        let pw = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
        let hash = pw.and_then(|p| bcrypt::hash(p, bcrypt::DEFAULT_COST).ok());
        let _ = sqlx::query("UPDATE upload_links SET password_hash = $1 WHERE id = $2").bind(hash).bind(id).execute(&state.db).await;
    }
    if body.contains_key("allowed_extensions") {
        let _ = sqlx::query("UPDATE upload_links SET allowed_extensions = $1 WHERE id = $2")
            .bind(normalize_extensions(body.get("allowed_extensions")))
            .bind(id).execute(&state.db).await;
    }
    if body.contains_key("uploader_fields_required") {
        let _ = sqlx::query("UPDATE upload_links SET uploader_fields_required = $1 WHERE id = $2")
            .bind(body.get("uploader_fields_required").cloned().unwrap_or(json!({})))
            .bind(id).execute(&state.db).await;
    }
    for key in ["target_folder_id", "target_folder_name", "subfolder_template"] {
        if body.contains_key(key) {
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE upload_links SET {key} = $1 WHERE id = $2")))
                .bind(str_or_none(body.get(key))).bind(id).execute(&state.db).await;
        }
    }
    for key in ["expires_at", "max_file_size", "verify_email", "brand_display_name", "brand_subtitle"] {
        if body.contains_key(key) {
            let v = body.get(key).unwrap();
            if key == "expires_at" {
                let dt = v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc));
                let _ = sqlx::query("UPDATE upload_links SET expires_at = $1 WHERE id = $2").bind(dt).bind(id).execute(&state.db).await;
            } else if key == "max_file_size" {
                let _ = sqlx::query("UPDATE upload_links SET max_file_size = $1 WHERE id = $2").bind(v.as_i64()).bind(id).execute(&state.db).await;
            } else if key == "verify_email" {
                let _ = sqlx::query("UPDATE upload_links SET verify_email = $1 WHERE id = $2").bind(v.as_bool().unwrap_or(false)).bind(id).execute(&state.db).await;
            } else {
                let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE upload_links SET {key} = $1 WHERE id = $2"))).bind(v.as_str()).bind(id).execute(&state.db).await;
            }
        }
    }
    // Verification implies the email field is required.
    let verify: Option<bool> = sqlx::query_scalar("SELECT verify_email FROM upload_links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if verify == Some(true) {
        let _ = sqlx::query("UPDATE upload_links SET uploader_fields_required = jsonb_set(COALESCE(uploader_fields_required, '{}'::jsonb), '{email}', 'true') WHERE id = $1").bind(id).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn revoke_link(State(state): State<AppState>, admin: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let already: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT revoked_at FROM upload_links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if already.is_none() {
        let _ = sqlx::query("UPDATE upload_links SET revoked_at = now() WHERE id = $1").bind(id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.revoked',$2)")
            .bind(admin.id).bind(json!({ "upload_link_id": id.to_string() })).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn delete_link(State(state): State<AppState>, admin: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let _ = sqlx::query("DELETE FROM upload_links WHERE id = $1").bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.deleted',$2)")
        .bind(admin.id).bind(json!({ "upload_link_id": id.to_string() })).execute(&state.db).await;
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub async fn stats(State(state): State<AppState>, _: Admin, axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let uploads: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_sessions WHERE upload_link_id = $1 AND status = 'completed'").bind(id).fetch_one(&state.db).await.unwrap_or(0);
    let total: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(total_bytes),0) FROM upload_sessions WHERE upload_link_id = $1 AND status = 'completed'").bind(id).fetch_one(&state.db).await.unwrap_or(0);
    let last: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT max(completed_at) FROM upload_sessions WHERE upload_link_id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    Json(json!({
        "uploads": uploads,
        "total_bytes": total,
        "last_activity": last.map(|d| crate::pyfmt::iso(&d)),
    })).into_response()
}

pub async fn list_links(State(state): State<AppState>, _: Admin) -> Response {
    let rows = sqlx::query(
        "SELECT l.id, l.token, l.expires_at, l.password_hash, l.max_file_size, l.verify_email,
                l.allowed_extensions, l.uploader_fields_required, l.revoked_at,
                l.brand_display_name, l.brand_subtitle, l.created_at, l.target_folder_id,
                l.target_folder_name, l.subfolder_template, l.destination_id,
                d.display_name AS destination_name, d.config AS destination_config
           FROM upload_links l
           JOIN destinations d ON d.id = l.destination_id
          ORDER BY l.created_at DESC",
    )
    .fetch_all(&state.db)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("upload_links.list_failed: {e}");
            return detail(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            );
        }
    };

    let mut out: Vec<Value> = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(render_row(&state, &row));
    }
    return Json(Value::Array(out)).into_response();
    #[allow(unreachable_code)]
    let base = state.settings.base_url.trim_end_matches('/').to_string();
    let mut out2: Vec<Value> = Vec::with_capacity(0);
    for row in Vec::<sqlx::postgres::PgRow>::new() {
        let token: String = row.get("token");
        let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
        let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
        let verify_email: bool = row.get("verify_email");
        let fields: Option<Value> = row.get("uploader_fields_required");
        let fields = fields.unwrap_or_else(|| json!({}));
        let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let email = flag("email") || verify_email;
        let dest_config: Value = row.get("destination_config");
        let created_at: DateTime<Utc> = row.get("created_at");

        out.push(json!({
            "id": row.get::<uuid::Uuid, _>("id").to_string(),
            "token": token,
            "public_url": format!("{base}/u/{token}"),
            "state": link_state(revoked_at, expires_at),
            "destination_id": row.get::<uuid::Uuid, _>("destination_id").to_string(),
            "destination_name": row.get::<String, _>("destination_name"),
            "destination_path": dest_config.get("path").cloned().unwrap_or(Value::Null),
            "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
            "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
            "max_file_size": row.get::<Option<i64>, _>("max_file_size"),
            "allowed_extensions": row.get::<Option<Value>, _>("allowed_extensions").unwrap_or(Value::Null),
            "uploader_fields_required": {
                "name": flag("name"),
                "email": email,
                "message": flag("message"),
            },
            "verify_email": verify_email,
            "target_folder_id": row.get::<Option<String>, _>("target_folder_id"),
            "target_folder_name": row.get::<Option<String>, _>("target_folder_name"),
            "subfolder_template": row.get::<Option<String>, _>("subfolder_template"),
            "brand_display_name": row.get::<Option<String>, _>("brand_display_name"),
            "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
            "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
            "created_at": crate::pyfmt::iso(&created_at),
        }));
    }

    Json(Value::Array(out)).into_response()
}
