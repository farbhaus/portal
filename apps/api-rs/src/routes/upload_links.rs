
//! Admin upload links — `portal.routes.upload_links` (`/api/upload-links`).
//!
//! Upload links are the `kind = 'upload'` rows of `links` (migration e1f2a3b4c5d6). A link
//! delivers into its own Frame.io `folder` (`{type, account_id, workspace_id, project_id,
//! folder_id, path}`). Datetime formatting follows `pyfmt::iso`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::routes::link_options;
use crate::services::frameio_paths::{clean_path, resolve_folder_path, stamp_path};
use crate::state::AppState;

/// `MEDIA_EXTENSIONS_PRESET` from `portal.uploads.links`.
pub const MEDIA_EXTENSIONS_PRESET: [&str; 26] = [
    "r3d", "braw", "ari", "arx", "mxf", "mov", "mp4", "mts", "m4v", "avi", "prores", "dng",
    "cdng", "exr", "dpx", "tiff", "tif", "png", "jpg", "jpeg", "wav", "aif", "aiff", "mp3",
    "bwf", "zip",
];

pub fn link_state(revoked_at: Option<DateTime<Utc>>, expires_at: Option<DateTime<Utc>>) -> &'static str {
    if revoked_at.is_some() {
        return "revoked";
    }
    if let Some(exp) = expires_at
        && exp <= Utc::now() {
            return "expired";
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

/// The `.turbosort` value: one Live_Work project folder name, no path separators.
fn clean_turbosort(value: &str) -> Result<String, Response> {
    let v = value.trim();
    if v.is_empty() {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must not be empty"));
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.contains('\n') || v.contains('\r') {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must be a single folder name"));
    }
    Ok(v.to_string())
}

fn turbosort_of(folder: &Value) -> Option<String> {
    folder.get("turbosort").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// `folder.sync` — per-link "sync to live work". FioTransfer downloads the client's drive into a
/// local folder and uploads another local folder into Frame.io, adding files in both directions
/// and never deleting any (`add_only`), so a person can clear Frame.io once they have their own
/// copy without losing what is on the NAS.
///
/// The only mode there is; an unknown one is rejected rather than guessed at.
const SYNC_MODES: [&str; 1] = ["add_only"];

/// One local folder name: a single path segment, no separators, no `%`, no control characters and
/// not `.`/`..` — it names one folder inside the project on the NAS.
fn clean_sync_folder(side: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(format!("sync.{side}.local must not be empty"));
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.contains('%')
        || v.chars().any(char::is_control) {
        return Err(format!("sync.{side}.local must be a single folder name"));
    }
    Ok(v.to_string())
}

/// A Frame.io V4 id is a UUID; anything else is a typo we would rather not store.
fn sync_folder_id(side: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if Uuid::parse_str(v).is_err() {
        return Err(format!("sync.{side}.folder_id must be a Frame.io folder id"));
    }
    Ok(v.to_string())
}

/// Validate `folder.sync` and return it as stored. `None` means the link has no sync: the key was
/// absent or null. Absent `folder_id`s stay absent — the consumer reads them as "the link's own
/// folder". `mode` is stored explicitly so the client never has to guess.
///
/// The two sides are independent switches in the composer, so either may be sent on its own:
/// `drive` alone is a link that only downloads the client's drive, `exports` alone is a link that
/// only uploads. A block with neither is not a sync at all and is rejected.
pub fn validate_sync(raw: Option<&Value>) -> Result<Option<Value>, String> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(obj) = raw.as_object() else { return Err("sync must be an object".to_string()) };
    let accepted = SYNC_MODES.map(|m| format!("'{m}'")).join(", ");
    let mode = match obj.get("mode") {
        None | Some(Value::Null) => SYNC_MODES[0].to_string(),
        Some(Value::String(m)) if SYNC_MODES.contains(&m.as_str()) => m.clone(),
        Some(_) => return Err(format!("sync.mode must be one of [{accepted}]")),
    };
    let side = |name: &str| -> Result<Option<Value>, String> {
        let Some(v) = obj.get(name).filter(|v| !v.is_null()) else { return Ok(None) };
        let Some(s) = v.as_object() else { return Err(format!("sync.{name} must be an object")) };
        let Some(local) = s.get("local").and_then(|v| v.as_str()) else {
            return Err(format!("sync.{name}.local is required"));
        };
        let mut out = Map::new();
        out.insert("local".to_string(), json!(clean_sync_folder(name, local)?));
        if let Some(id) = s.get("folder_id").and_then(|v| v.as_str()) {
            out.insert("folder_id".to_string(), json!(sync_folder_id(name, id)?));
        }
        Ok(Some(Value::Object(out)))
    };
    // At least one side has to be there: a sync block with neither lane does nothing.
    let drive = side("drive")?;
    let exports = side("exports")?;
    if drive.is_none() && exports.is_none() {
        return Err("sync needs at least one of drive or exports".to_string());
    }
    // The two local folders must be different — but only when both sides are stored.
    if let (Some(drive), Some(exp)) = (&drive, &exports)
        && exp.get("local") == drive.get("local") {
            return Err("sync.drive.local and sync.exports.local must be different folders".to_string());
        }
    let mut out = Map::new();
    out.insert("mode".to_string(), json!(mode));
    if let Some(drive) = drive {
        out.insert("drive".to_string(), drive);
    }
    if let Some(exp) = exports {
        out.insert("exports".to_string(), exp);
    }
    Ok(Some(Value::Object(out)))
}

/// The columns every admin view of an upload link reads.
pub const LINK_COLUMNS: &str = "SELECT l.id, l.token, l.name, l.expires_at, l.password_hash, l.max_file_size,
        l.verify_email, l.allowed_extensions, l.uploader_fields_required, l.revoked_at,
        l.brand_display_name, l.brand_subtitle, l.created_at, l.target_folder_id,
        l.target_folder_name, l.subfolder_template, l.folder, l.brand_profile_id,
        l.notify_emails, l.remind_before_expiry
   FROM links l WHERE l.kind = 'upload'";

/// Render one link's `UploadLinkOut`, or None if it doesn't exist.
async fn fetch_out(state: &AppState, id: Uuid) -> Option<Value> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{LINK_COLUMNS} AND l.id = $1")))
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()?;
    Some(render_row(state, &row))
}

/// The name the admin sees: its own, else the client-facing title, else the folder's name.
pub fn effective_name(name: Option<String>, brand: Option<String>, folder: &Value) -> String {
    let nonempty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    nonempty(name)
        .or_else(|| nonempty(brand))
        .or_else(|| folder.get("folder_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "Untitled".to_string())
}

pub fn render_row(state: &AppState, row: &sqlx::postgres::PgRow) -> Value {
    let token: String = row.get("token");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let verify_email: bool = row.get("verify_email");
    let fields: Option<Value> = row.get("uploader_fields_required");
    let fields = fields.unwrap_or_else(|| json!({}));
    let flag = |k: &str| fields.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let email = flag("email") || verify_email;
    let folder: Value = row.get::<Option<Value>, _>("folder").unwrap_or(Value::Null);
    let turbosort = turbosort_of(&folder);
    let created_at: DateTime<Utc> = row.get("created_at");
    let brand: Option<String> = row.get("brand_display_name");
    let base = state.settings.base_url.trim_end_matches('/');
    let mut out = json!({
        "id": row.get::<Uuid, _>("id").to_string(),
        "kind": "upload",
        "name": effective_name(row.get("name"), brand.clone(), &folder),
        "token": token,
        "public_url": format!("{base}/u/{token}"),
        "state": link_state(revoked_at, expires_at),
        "folder": folder,
        "turbosort": turbosort,
        "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
        "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
        "max_file_size": row.get::<Option<i64>, _>("max_file_size"),
        "allowed_extensions": row.get::<Option<Value>, _>("allowed_extensions").unwrap_or(Value::Null),
        "uploader_fields_required": { "name": flag("name"), "email": email, "message": flag("message") },
        "verify_email": verify_email,
        "target_folder_id": row.get::<Option<String>, _>("target_folder_id"),
        "target_folder_name": row.get::<Option<String>, _>("target_folder_name"),
        "subfolder_template": row.get::<Option<String>, _>("subfolder_template"),
        "brand_display_name": brand,
        "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
        "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
        "created_at": crate::pyfmt::iso(&created_at),
    });
    link_options::render(row, &mut out);
    out
}

fn not_found() -> Response {
    detail(StatusCode::NOT_FOUND, "Upload link not found")
}

/// Check a picked Frame.io folder and build the config stored on the link: ids, the folder's name
/// from Frame.io, and the display breadcrumb (the picker's, else resolved best-effort).
pub async fn validated_folder(state: &AppState, raw: &Value) -> Result<Value, Response> {
    // `sync` is ours alone: check it here, before any Frame.io call, and carry it into the stored
    // config. It rides along in `folder`, so it is returned verbatim by every read.
    let sync = validate_sync(raw.get("sync")).map_err(|m| detail(StatusCode::UNPROCESSABLE_ENTITY, &m))?;
    let get = |k: &str| str_or_none(raw.get(k));
    let (Some(account_id), Some(folder_id)) = (get("account_id"), get("folder_id")) else {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "folder needs account_id and folder_id"));
    };
    let folder = match state.frameio.get_folder(&account_id, &folder_id).await {
        Ok(f) => f,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return Err(detail(StatusCode::CONFLICT, "Frame.io is not connected"))
        }
        Err(_) => return Err(detail(StatusCode::BAD_REQUEST, "Frame.io folder not found or inaccessible")),
    };
    let project_id = get("project_id").or(folder.project_id.clone()).unwrap_or_default();
    let project_name = get("project_name");
    let mut stored = json!({
        "type": "frameio",
        "account_id": account_id,
        "workspace_id": get("workspace_id"),
        "project_id": project_id,
        "folder_id": folder_id,
        "folder_name": folder.name,
    });
    if let Some(sync) = sync
        && let Some(obj) = stored.as_object_mut() {
            obj.insert("sync".to_string(), sync);
        }
    if let Some(path) = clean_path(raw.get("path"), &folder_id) {
        return Ok(stamp_path(&stored, &path, project_name.as_deref()));
    }
    if let Some((path, pn)) = resolve_folder_path(state, &account_id, &project_id, &folder_id).await {
        return Ok(stamp_path(&stored, &path, pn.as_deref()));
    }
    Ok(stored)
}

pub async fn get_link(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn create_link(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    // Where uploads land: the picked Frame.io folder.
    let Some(raw) = body.get("folder").filter(|v| v.is_object()) else {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "folder is required");
    };
    let mut folder = match validated_folder(&state, raw).await {
        Ok(f) => f,
        Err(r) => return r,
    };
    let brand_profile = match link_options::brand_profile(&state, body.get("brand_profile_id")).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let notify = match link_options::notify_emails(body.get("notify_emails")) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let password = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    let password_hash = match password {
        Some(p) => bcrypt::hash(p, bcrypt::DEFAULT_COST).ok(),
        None => None,
    };
    let verify_email = body.get("verify_email").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut fields = body.get("uploader_fields_required").cloned().unwrap_or_else(|| json!({}));
    if verify_email {
        fields["email"] = json!(true);
    }
    let brand = str_or_none(body.get("brand_display_name"));
    let name = effective_name(str_or_none(body.get("name")), brand.clone(), &folder);
    let turbosort = match body.get("turbosort") {
        Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => match clean_turbosort(s) {
            Ok(v) => Some(v),
            Err(r) => return r,
        },
        Some(_) => return detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must be a string"),
        None => None,
    };
    if let Some(ts) = &turbosort {
        folder["turbosort"] = json!(ts);
    }
    let token = generate_token();
    let inserted = sqlx::query(
        "INSERT INTO links (kind, token, name, folder, expires_at, password_hash, max_file_size,
            allowed_extensions, uploader_fields_required, verify_email, subfolder_template,
            brand_display_name, brand_subtitle, brand_profile_id, notify_emails, remind_before_expiry)
         VALUES ('upload',$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id",
    )
    .bind(&token)
    .bind(&name)
    .bind(&folder)
    .bind(body.get("expires_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc)))
    .bind(password_hash)
    .bind(body.get("max_file_size").and_then(|v| v.as_i64()))
    .bind(normalize_extensions(body.get("allowed_extensions")))
    .bind(&fields)
    .bind(verify_email)
    .bind(str_or_none(body.get("subfolder_template")))
    .bind(brand)
    .bind(body.get("brand_subtitle").and_then(|v| v.as_str()).map(|s| s.to_string()))
    .bind(brand_profile)
    .bind(notify)
    .bind(body.get("remind_before_expiry").and_then(|v| v.as_bool()).unwrap_or(false))
    .fetch_one(&state.db)
    .await;
    let id: Uuid = match inserted {
        Ok(r) => r.get("id"),
        Err(e) => {
            tracing::error!("upload_links.create_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.created',$2)")
        .bind(admin.id)
        .bind(json!({ "upload_link_id": id.to_string(), "folder_id": folder.get("folder_id") }))
        .execute(&state.db).await;
    match fetch_out(&state, id).await {
        Some(v) => (StatusCode::CREATED, Json(v)).into_response(),
        None => not_found(),
    }
}

pub async fn update_link(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> Response {
    let raw = match sqlx::query("SELECT folder FROM links WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("upload_links.update_load_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let old_folder: Value = raw.get::<Option<Value>, _>("folder").unwrap_or(Value::Null);

    // `exclude_unset`: only keys present in the body are applied.
    let folder_changed = body.get("folder").filter(|v| v.is_object()).is_some();
    let mut new_folder = match body.get("folder").filter(|v| v.is_object()) {
        Some(raw_folder) => {
            let mut f = match validated_folder(&state, raw_folder).await {
                Ok(f) => f,
                Err(r) => return r,
            };
            // A body that says nothing about `sync` leaves it as it was; only the key present as
            // `null` (or a new object) changes it.
            if raw_folder.get("sync").is_none()
                && let (Some(old_sync), Some(obj)) = (old_folder.get("sync"), f.as_object_mut()) {
                    obj.insert("sync".to_string(), old_sync.clone());
                }
            f
        }
        None => old_folder.clone(),
    };
    let turbosort_change: Option<Option<String>> = match body.get("turbosort") {
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) if s.trim().is_empty() => Some(None),
        Some(Value::String(s)) => match clean_turbosort(s) {
            Ok(v) => Some(Some(v)),
            Err(r) => return r,
        },
        Some(_) => return detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must be a string"),
        None => None,
    };
    let new_turbosort = turbosort_change.clone().unwrap_or_else(|| turbosort_of(&old_folder));

    if folder_changed || turbosort_change.is_some() {
        if let Some(obj) = new_folder.as_object_mut() {
            match &new_turbosort {
                Some(ts) => { obj.insert("turbosort".to_string(), json!(ts)); }
                None => { obj.remove("turbosort"); }
            }
        }
        if folder_changed && new_folder.get("folder_id") != old_folder.get("folder_id") {
            // The folder is now exactly where uploads land: drop any older base-subfolder override.
            // A `folder` sent only to carry a new `sync` leaves the override alone.
            let _ = sqlx::query(
                "UPDATE links SET folder = $1, target_folder_id = NULL, target_folder_name = NULL WHERE id = $2",
            )
            .bind(&new_folder).bind(id).execute(&state.db).await;
        } else if folder_changed || turbosort_change.is_some() {
            let _ = sqlx::query("UPDATE links SET folder = $1 WHERE id = $2")
                .bind(&new_folder).bind(id).execute(&state.db).await;
        }
    }
    if let Err(r) = link_options::apply_update(&state, id, &body).await {
        return r;
    }
    if body.contains_key("password") {
        let pw = body.get("password").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
        let hash = pw.and_then(|p| bcrypt::hash(p, bcrypt::DEFAULT_COST).ok());
        let _ = sqlx::query("UPDATE links SET password_hash = $1 WHERE id = $2").bind(hash).bind(id).execute(&state.db).await;
    }
    if body.contains_key("allowed_extensions") {
        let _ = sqlx::query("UPDATE links SET allowed_extensions = $1 WHERE id = $2")
            .bind(normalize_extensions(body.get("allowed_extensions")))
            .bind(id).execute(&state.db).await;
    }
    if body.contains_key("uploader_fields_required") {
        let _ = sqlx::query("UPDATE links SET uploader_fields_required = $1 WHERE id = $2")
            .bind(body.get("uploader_fields_required").cloned().unwrap_or(json!({})))
            .bind(id).execute(&state.db).await;
    }
    for key in ["name", "target_folder_id", "target_folder_name", "subfolder_template"] {
        if body.contains_key(key) {
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE links SET {key} = $1 WHERE id = $2")))
                .bind(str_or_none(body.get(key))).bind(id).execute(&state.db).await;
        }
    }
    for key in ["expires_at", "max_file_size", "verify_email", "brand_display_name", "brand_subtitle"] {
        if body.contains_key(key) {
            let v = body.get(key).unwrap();
            if key == "expires_at" {
                let dt = v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc));
                let _ = sqlx::query("UPDATE links SET expires_at = $1 WHERE id = $2").bind(dt).bind(id).execute(&state.db).await;
            } else if key == "max_file_size" {
                let _ = sqlx::query("UPDATE links SET max_file_size = $1 WHERE id = $2").bind(v.as_i64()).bind(id).execute(&state.db).await;
            } else if key == "verify_email" {
                let _ = sqlx::query("UPDATE links SET verify_email = $1 WHERE id = $2").bind(v.as_bool().unwrap_or(false)).bind(id).execute(&state.db).await;
            } else {
                let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE links SET {key} = $1 WHERE id = $2"))).bind(v.as_str()).bind(id).execute(&state.db).await;
            }
        }
    }
    // Verification implies the email field is required.
    let verify: Option<bool> = sqlx::query_scalar("SELECT verify_email FROM links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if verify == Some(true) {
        let _ = sqlx::query("UPDATE links SET uploader_fields_required = jsonb_set(COALESCE(uploader_fields_required, '{}'::jsonb), '{email}', 'true') WHERE id = $1").bind(id).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn revoke_link(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let already: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT revoked_at FROM links WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if already.is_none() {
        let _ = sqlx::query("UPDATE links SET revoked_at = now(), updated_at = now() WHERE id = $1").bind(id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.revoked',$2)")
            .bind(admin.id).bind(json!({ "upload_link_id": id.to_string() })).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

/// `POST /api/upload-links/{id}/restore` — undo a revoke.
pub async fn restore_link(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let res = sqlx::query("UPDATE links SET revoked_at = NULL, updated_at = now() WHERE id = $1 AND revoked_at IS NOT NULL")
        .bind(id).execute(&state.db).await;
    if res.map(|r| r.rows_affected()).unwrap_or(0) > 0 {
        let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.restored',$2)")
            .bind(admin.id).bind(json!({ "upload_link_id": id.to_string() })).execute(&state.db).await;
    }
    match fetch_out(&state, id).await {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    }
}

pub async fn delete_link(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    // The row's existence is the only thing the delete needs to know: it is the 404 test.
    match sqlx::query("SELECT 1 FROM links WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("upload_links.delete_load_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    }
    let _ = sqlx::query("DELETE FROM links WHERE id = $1").bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'upload_link.deleted',$2)")
        .bind(admin.id).bind(json!({ "upload_link_id": id.to_string() })).execute(&state.db).await;
    StatusCode::NO_CONTENT.into_response()
}

pub async fn stats(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
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

/// `GET /api/upload-links/{id}/sessions` — the link's recent upload sessions, newest first:
/// who, how many files, how much, and whether it finished.
pub async fn sessions(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>, Query(q): Query<HashMap<String, String>>) -> Response {
    if fetch_out(&state, id).await.is_none() {
        return not_found();
    }
    let limit = q.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(10).clamp(1, 100);
    let rows = sqlx::query(
        "SELECT id, uploader_name, uploader_email, status, frameio_asset_ids, total_bytes, uploaded_bytes,
                started_at, completed_at, last_activity_at, created_at
           FROM upload_sessions
          WHERE upload_link_id = $1
            AND (status <> 'abandoned' OR jsonb_array_length(COALESCE(frameio_asset_ids, '[]'::jsonb)) > 0)
          ORDER BY COALESCE(completed_at, last_activity_at, started_at, created_at) DESC
          LIMIT $2",
    )
    .bind(id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            let files: Vec<Value> = r.get::<Option<Value>, _>("frameio_asset_ids").and_then(|v| v.as_array().cloned()).unwrap_or_default();
            let planned: i64 = files.iter().filter_map(|f| f.get("size").and_then(|s| s.as_i64())).sum();
            let iso = |k: &str| r.get::<Option<DateTime<Utc>>, _>(k).map(|d| crate::pyfmt::iso(&d));
            json!({
                "id": r.get::<Uuid, _>("id").to_string(),
                "uploader_name": r.get::<Option<String>, _>("uploader_name"),
                "uploader_email": r.get::<Option<String>, _>("uploader_email"),
                "status": r.get::<String, _>("status"),
                "files": files.len(),
                "total_bytes": r.get::<Option<i64>, _>("total_bytes").or(if planned > 0 { Some(planned) } else { None }),
                "uploaded_bytes": r.get::<Option<i64>, _>("uploaded_bytes"),
                "started_at": iso("started_at"),
                "completed_at": iso("completed_at"),
                "last_activity_at": iso("last_activity_at"),
            })
        })
        .collect();
    Json(Value::Array(out)).into_response()
}

pub async fn list_links(State(state): State<AppState>, _: Admin) -> Response {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("{LINK_COLUMNS} ORDER BY l.created_at DESC")))
        .fetch_all(&state.db)
        .await;
    match rows {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| render_row(&state, r)).collect())).into_response(),
        Err(e) => {
            tracing::error!("upload_links.list_failed: {e}");
            detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_name_prefers_own_then_title_then_folder() {
        let folder = json!({ "folder_name": "Day 3" });
        assert_eq!(effective_name(Some("Cards".into()), Some("Send us".into()), &folder), "Cards");
        assert_eq!(effective_name(Some("  ".into()), Some("Send us".into()), &folder), "Send us");
        assert_eq!(effective_name(None, None, &folder), "Day 3");
        assert_eq!(effective_name(None, None, &Value::Null), "Untitled");
    }

    #[test]
    fn extensions_are_normalized() {
        let v = json!([".MOV", "mov", " r3d ", ""]);
        assert_eq!(normalize_extensions(Some(&v)), Some(json!(["mov", "r3d"])));
        assert_eq!(normalize_extensions(Some(&json!([]))), None);
    }

    #[test]
    fn sync_is_optional_and_null_clears_it() {
        assert_eq!(validate_sync(None), Ok(None));
        assert_eq!(validate_sync(Some(&Value::Null)), Ok(None));
    }

    #[test]
    fn sync_stores_add_only_and_keeps_each_side_as_given() {
        // Omitted folder ids stay omitted: the client reads them as "the link's own folder".
        let raw = json!({ "drive": { "local": " 1_DRIVE " } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" } })))
        );
        let raw = json!({
            "mode": "add_only",
            "drive": { "local": "1_DRIVE", "folder_id": "8008cf70-f28e-474d-a209-d67afb9fddb6" },
            "exports": { "local": "0_EXPORTS", "folder_id": "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3" },
        });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({
                "mode": "add_only",
                "drive": { "local": "1_DRIVE", "folder_id": "8008cf70-f28e-474d-a209-d67afb9fddb6" },
                "exports": { "local": "0_EXPORTS", "folder_id": "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3" },
            })))
        );
    }

    #[test]
    fn sync_local_folder_is_one_path_segment() {
        for bad in [".", "..", "1_DRIVE/out", "1_DRIVE\\out", "1_DRIVE%2Fout", "a\nb", "a\tb"] {
            let raw = json!({ "drive": { "local": bad } });
            assert_eq!(
                validate_sync(Some(&raw)),
                Err("sync.drive.local must be a single folder name".to_string()),
                "{bad:?} must be rejected"
            );
        }
        for empty in ["", "  "] {
            let raw = json!({ "drive": { "local": empty } });
            assert_eq!(
                validate_sync(Some(&raw)),
                Err("sync.drive.local must not be empty".to_string()),
                "{empty:?} must be rejected"
            );
        }
    }

    #[test]
    fn sync_folder_ids_must_be_frameio_ids() {
        let raw = json!({ "drive": { "local": "1_DRIVE", "folder_id": "1_DRIVE" } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Err("sync.drive.folder_id must be a Frame.io folder id".to_string())
        );
        let raw = json!({ "drive": { "local": "1_DRIVE" }, "exports": { "local": "0_EXPORTS", "folder_id": "" } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Err("sync.exports.folder_id must be a Frame.io folder id".to_string())
        );
    }

    #[test]
    fn sync_shape_and_mode_errors_name_what_is_accepted() {
        assert_eq!(validate_sync(Some(&json!("add_only"))), Err("sync must be an object".to_string()));
        assert_eq!(
            validate_sync(Some(&json!({}))),
            Err("sync needs at least one of drive or exports".to_string())
        );
        assert_eq!(
            validate_sync(Some(&json!({ "drive": "1_DRIVE" }))),
            Err("sync.drive must be an object".to_string())
        );
        assert_eq!(
            validate_sync(Some(&json!({ "drive": {} }))),
            Err("sync.drive.local is required".to_string())
        );
        assert_eq!(
            validate_sync(Some(&json!({ "mode": "mirror", "drive": { "local": "1_DRIVE" } }))),
            Err("sync.mode must be one of ['add_only']".to_string())
        );
    }

    #[test]
    fn sync_takes_either_side_on_its_own() {
        // Exports alone: a link that only uploads. No `drive` key is invented.
        let raw = json!({ "exports": { "local": " 0_EXPORTS " } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS" } })))
        );
        let raw = json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS", "folder_id": "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3" } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({
                "mode": "add_only",
                "exports": { "local": "0_EXPORTS", "folder_id": "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3" },
            })))
        );
        // Drive alone is still what it was.
        assert_eq!(
            validate_sync(Some(&json!({ "drive": { "local": "1_DRIVE" } }))),
            Ok(Some(json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" } })))
        );
        // A null side is no side.
        let raw = json!({ "drive": null, "exports": { "local": "0_EXPORTS" } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS" } })))
        );
        // Neither side is not a sync, and says so.
        assert_eq!(
            validate_sync(Some(&json!({ "mode": "add_only" }))),
            Err("sync needs at least one of drive or exports".to_string())
        );
        assert_eq!(
            validate_sync(Some(&json!({ "drive": null, "exports": null }))),
            Err("sync needs at least one of drive or exports".to_string())
        );
    }

    #[test]
    fn sync_sides_must_name_different_local_folders() {
        let raw = json!({
            "drive": { "local": "1_DRIVE" },
            "exports": { "local": " 1_DRIVE " },
        });
        assert_eq!(
            validate_sync(Some(&raw)),
            Err("sync.drive.local and sync.exports.local must be different folders".to_string())
        );
        // The rule only bites when both sides are stored: one side may be named anything.
        let raw = json!({ "exports": { "local": "1_DRIVE" } });
        assert_eq!(
            validate_sync(Some(&raw)),
            Ok(Some(json!({ "mode": "add_only", "exports": { "local": "1_DRIVE" } })))
        );
    }
}
