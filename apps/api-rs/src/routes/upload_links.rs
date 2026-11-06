
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
use crate::services::fiotransfer::{clean_folder_name, default_project_folder, lane_names, LaneNames};
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

/// The `turbosort` value: the project's folder on the user's machine, which every FioTransfer
/// reads (`<root>/<turbosort>/<Imports>` and `/<Exports>`). A synced link always has one: when
/// none is given it is the job folder the link's Imports sit in. One path segment: no
/// separators, no control characters, not `.`/`..`.
fn clean_turbosort(value: &str) -> Result<String, Response> {
    let v = value.trim();
    if v.is_empty() {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must not be empty"));
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.chars().any(char::is_control) {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Project folder must be a single folder name"));
    }
    Ok(v.to_string())
}

fn turbosort_of(folder: &Value) -> Option<String> {
    folder.get("turbosort").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// `folder.sync` — "Sync this project's folders with FioTransfer". Portal only stores it; the
/// FioTransfer client reads it from `GET /api/upload-links` and keeps two one-way folders under
/// the link's project folder (`turbosort`), adding files and never deleting any:
///
/// ```json
/// "sync": {
///   "mode": "add_only",
///   "drive":   { "local": "1_DRIVE",   "folder_id": "<optional: else the link's own folder>" },
///   "exports": { "local": "0_EXPORTS", "folder_id": "<required>" }
/// }
/// ```
///
/// - `drive` is Imports: what the client sends comes down into `local`. Absent, nothing comes
///   down.
/// - `exports` is Exports: what the user puts in `local` goes up to `folder_id`. Absent, nothing
///   goes up.
/// - A missing `local` gets the Imports/Exports name from Settings, so a naming convention is
///   set once, not per link.
///
/// Portal 2.2 and 2.3 wrote `download`, `upload` and `name` instead, and took `{}` for "on". They
/// are still taken and stored in this shape (`download` as `drive`, `upload` as `exports`, `name`
/// as the link's `turbosort`, `{}` as Imports with its defaults), so API clients written against
/// them keep working.
const SYNC_MODES: [&str; 1] = ["add_only"];

/// A Frame.io V4 id is a UUID; anything else is a typo we would rather not store.
fn sync_folder_id(lane: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if Uuid::parse_str(v).is_err() {
        return Err(format!("sync.{lane}.folder_id must be a Frame.io folder id"));
    }
    Ok(v.to_string())
}

/// One lane of a sync block under its name or the 2.2 name it had (never both), cleaned: its
/// `local` name (the Settings default when missing) and optional `folder_id`. `None` is a lane
/// that is off: absent or null.
fn sync_lane(sync: &Map<String, Value>, key: &str, older: &str, default_local: &str) -> Result<Option<Map<String, Value>>, String> {
    let present = |k: &str| sync.get(k).filter(|v| !v.is_null());
    let lane = match (present(key), present(older)) {
        (Some(_), Some(_)) => return Err(format!("sync.{older} is Portal 2.2's name for sync.{key}: send one of them")),
        (Some(v), None) | (None, Some(v)) => v,
        (None, None) => return Ok(None),
    };
    let Some(lane) = lane.as_object() else { return Err(format!("sync.{key} must be an object")) };
    let mut out = Map::new();
    let local = match lane.get("local").filter(|v| !v.is_null()) {
        Some(Value::String(s)) if !s.trim().is_empty() => s.as_str(),
        Some(Value::String(_)) | None => default_local,
        Some(_) => return Err(format!("sync.{key}.local must be a string")),
    };
    out.insert("local".to_string(), json!(clean_folder_name(&format!("sync.{key}.local"), local)?));
    if let Some(id) = lane.get("folder_id").filter(|v| !v.is_null()) {
        let Some(id) = id.as_str().filter(|s| !s.trim().is_empty()) else {
            return Err(format!("sync.{key}.folder_id must be a Frame.io folder id"));
        };
        out.insert("folder_id".to_string(), json!(sync_folder_id(key, id)?));
    }
    Ok(Some(out))
}

/// A validated sync block, and the project folder it named in Portal 2.2's spelling (`sync.name`)
/// for the caller to store as the link's `turbosort`.
#[derive(Debug, PartialEq)]
pub struct ValidSync {
    pub sync: Value,
    pub name: Option<String>,
}

/// Validate `folder.sync` for a link receiving into `link_folder_id`, and return it in the shape
/// it is stored in. `None` means the link isn't synced: the key was absent or null.
pub fn validate_sync(raw: Option<&Value>, link_folder_id: &str, names: &LaneNames) -> Result<Option<ValidSync>, String> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(obj) = raw.as_object() else { return Err("sync must be an object".to_string()) };
    match obj.get("mode") {
        None | Some(Value::Null) => {}
        Some(Value::String(m)) if SYNC_MODES.contains(&m.as_str()) => {}
        Some(_) => {
            let accepted = SYNC_MODES.map(|m| format!("'{m}'")).join(", ");
            return Err(format!("sync.mode must be one of [{accepted}]"));
        }
    }
    let name = match obj.get("name").filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(clean_folder_name("sync.name", s)?),
        Some(_) => return Err("sync.name must be a string".to_string()),
    };
    // No lane named at all is Portal 2.2's `{}`: Imports, with its defaults.
    let names_a_lane = ["drive", "download", "exports", "upload"].iter().any(|k| obj.contains_key(*k));
    let drive = if names_a_lane {
        sync_lane(obj, "drive", "download", &names.imports)?
    } else {
        sync_lane(&Map::from_iter([("drive".to_string(), json!({}))]), "drive", "download", &names.imports)?
    };
    let exports = sync_lane(obj, "exports", "upload", &names.exports)?;
    if drive.is_none() && exports.is_none() {
        return Err("sync needs Imports (sync.drive) or Exports (sync.exports)".to_string());
    }
    let text = |lane: &Option<Map<String, Value>>, f: &str| lane.as_ref().and_then(|l| l.get(f)).and_then(|v| v.as_str()).map(str::to_string);
    if let Some(exp) = &exports {
        let Some(up) = exp.get("folder_id").and_then(|v| v.as_str()) else {
            return Err("sync.exports.folder_id is required: the Frame.io folder Exports go up to".to_string());
        };
        // Exports go up somewhere Imports don't come down from.
        if drive.is_some() && up == text(&drive, "folder_id").as_deref().unwrap_or(link_folder_id) {
            return Err("sync.exports.folder_id must be a different folder from the one Imports come down from".to_string());
        }
        // macOS and Windows ignore case, so RUSHES and rushes are one folder.
        if let (Some(d), Some(e)) = (text(&drive, "local"), text(&exports, "local"))
            && d.to_lowercase() == e.to_lowercase() {
                return Err("sync.drive.local and sync.exports.local must be different folders".to_string());
            }
    }
    let mut out = Map::new();
    out.insert("mode".to_string(), json!(SYNC_MODES[0]));
    if let Some(d) = drive {
        out.insert("drive".to_string(), Value::Object(d));
    }
    if let Some(e) = exports {
        out.insert("exports".to_string(), Value::Object(e));
    }
    Ok(Some(ValidSync { sync: Value::Object(out), name }))
}

/// The folder Imports come down from (its own `folder_id`, else the link's) when Imports is on,
/// and the folder Exports go up to when Exports is on, of a stored sync block.
fn sync_folders<'a>(sync: &'a Value, link_folder_id: &'a str) -> (Option<&'a str>, Option<&'a str>) {
    let lane = |k: &str| sync.get(k).filter(|l| l.is_object());
    (
        lane("drive").map(|l| l.get("folder_id").and_then(|v| v.as_str()).unwrap_or(link_folder_id)),
        lane("exports").and_then(|l| l.get("folder_id")).and_then(|v| v.as_str()),
    )
}

/// A synced link without a project folder gets the job folder its Imports sit in.
fn ensure_project_folder(folder: &mut Value, names: &LaneNames) -> Result<(), Response> {
    if folder.get("sync").is_none_or(|s| s.is_null()) || turbosort_of(folder).is_some() {
        return Ok(());
    }
    let Some(name) = default_project_folder(folder, &names.imports) else {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Name the project folder FioTransfer keeps this link's folders in"));
    };
    folder["turbosort"] = json!(name);
    Ok(())
}

/// The columns every admin view of an upload link reads.
pub const LINK_COLUMNS: &str = "SELECT l.id, l.token, l.name, l.expires_at, l.password_hash, l.max_file_size,
        l.verify_email, l.allowed_extensions, l.uploader_fields_required, l.revoked_at,
        l.brand_display_name, l.brand_subtitle, l.created_at, l.target_folder_id,
        l.target_folder_name, l.subfolder_template, l.folder,
        l.notify_emails, l.remind_before_expiry, l.send_link, r.id AS return_link_id, r.token AS return_token
   FROM links l LEFT JOIN links r ON r.return_for = l.id WHERE l.kind = 'upload'";

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
        .or_else(|| job_folder_name(folder))
        .or_else(|| folder.get("folder_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .or_else(|| nonempty(brand))
        .unwrap_or_else(|| "Untitled".to_string())
}

/// What an unnamed upload link is called: the first folder below the project (a client's job
/// folder, whatever the link delivers into inside it), or the project itself when the link
/// delivers into its root. FioTransfer names the link's folder on disk after it.
fn job_folder_name(folder: &Value) -> Option<String> {
    let path = folder.get("path")?.as_array()?;
    let seg = path.get(1).or_else(|| path.first())?;
    let name = seg.get("name")?.as_str()?.trim_end_matches(" (root)").trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub fn render_row(state: &AppState, row: &sqlx::postgres::PgRow) -> Value {
    let token: String = row.get("token");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let verify_email: bool = row.get("verify_email");
    let fields = crate::services::upload_paths::with_template_asks(
        row.get("uploader_fields_required"),
        row.get::<Option<String>, _>("subfolder_template").as_deref(),
    );
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
        // Whether Portal keeps a Send link on the Exports folder (services::return_links).
        "send_link": row.try_get::<bool, _>("send_link").unwrap_or(true),
        "expires_at": expires_at.map(|d| crate::pyfmt::iso(&d)),
        "password_protected": row.get::<Option<String>, _>("password_hash").is_some(),
        "max_file_size": row.get::<Option<i64>, _>("max_file_size"),
        "allowed_extensions": row.get::<Option<Value>, _>("allowed_extensions").unwrap_or(Value::Null),
        "uploader_fields_required": { "name": flag("name"), "email": email, "message": flag("message"), "project": flag("project") },
        "verify_email": verify_email,
        "target_folder_id": row.get::<Option<String>, _>("target_folder_id"),
        "target_folder_name": row.get::<Option<String>, _>("target_folder_name"),
        "subfolder_template": row.get::<Option<String>, _>("subfolder_template"),
        "brand_display_name": brand,
        "brand_subtitle": row.get::<Option<String>, _>("brand_subtitle"),
        "revoked_at": revoked_at.map(|d| crate::pyfmt::iso(&d)),
        "created_at": crate::pyfmt::iso(&created_at),
    });
    // The Send link for its return folder (services::return_links). Lists that read every column
    // of `links` don't join it in and fill it themselves.
    let return_id: Option<Uuid> = row.try_get("return_link_id").ok().flatten();
    let return_token: Option<String> = row.try_get("return_token").ok().flatten();
    out["return_link"] = match return_id.zip(return_token) {
        Some((id, token)) => json!({ "id": id.to_string(), "public_url": format!("{base}/d/{token}") }),
        None => Value::Null,
    };
    link_options::render(row, &mut out);
    out
}

fn not_found() -> Response {
    detail(StatusCode::NOT_FOUND, "Upload link not found")
}

/// Check a picked Frame.io folder and build the config stored on the link: ids, the folder's name
/// from Frame.io, and the display breadcrumb (the picker's, else resolved best-effort).
pub async fn validated_folder(state: &AppState, raw: &Value) -> Result<Value, Response> {
    let get = |k: &str| str_or_none(raw.get(k));
    let (Some(account_id), Some(folder_id)) = (get("account_id"), get("folder_id")) else {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "folder needs account_id and folder_id"));
    };
    // `sync` is ours alone: check it here, before any Frame.io call, and carry it into the stored
    // config. It rides along in `folder`, so it is returned verbatim by every read.
    let names = match raw.get("sync").filter(|v| !v.is_null()) {
        Some(_) => lane_names(&state.db).await,
        None => LaneNames::default(),
    };
    let sync = validate_sync(raw.get("sync"), &folder_id, &names).map_err(|m| detail(StatusCode::UNPROCESSABLE_ENTITY, &m))?;
    let folder = match state.frameio.get_folder(&account_id, &folder_id).await {
        Ok(f) => f,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return Err(detail(StatusCode::CONFLICT, "Frame.io is not connected"))
        }
        Err(_) => return Err(detail(StatusCode::BAD_REQUEST, "Frame.io folder not found or inaccessible")),
    };
    // The Exports folder can't be inside the folder Imports come down from: FioTransfer brings that
    // folder down whole, so what goes up would come straight back down. (Best-effort: skipped when
    // Frame.io can't say where the Exports folder is.)
    if let Some(valid) = &sync
        && let (Some(down), Some(up)) = sync_folders(&valid.sync, &folder_id)
        && let Ok(chain) = state.frameio.folder_ancestry(&account_id, up).await
        && chain.iter().any(|f| f.id == down && f.id != up)
    {
        return Err(detail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The Exports folder can't be inside the folder Imports come down from: what you add to it would come straight back down",
        ));
    }
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
    if let Some(valid) = sync {
        stored["sync"] = valid.sync;
        if let Some(name) = valid.name {
            stored["turbosort"] = json!(name);
        }
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
    if folder.get("sync").is_some() {
        let names = lane_names(&state.db).await;
        if let Err(r) = ensure_project_folder(&mut folder, &names) {
            return r;
        }
    }
    let send_link = match body.get("send_link") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return detail(StatusCode::UNPROCESSABLE_ENTITY, "send_link must be true or false"),
    };
    let token = generate_token();
    let inserted = sqlx::query(
        "INSERT INTO links (kind, token, name, folder, expires_at, password_hash, max_file_size,
            allowed_extensions, uploader_fields_required, verify_email, subfolder_template,
            brand_display_name, brand_subtitle, notify_emails, remind_before_expiry, send_link)
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
    .bind(notify)
    .bind(body.get("remind_before_expiry").and_then(|v| v.as_bool()).unwrap_or(false))
    .bind(send_link)
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
    crate::services::return_links::sync(&state, id).await;
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
    // A project folder named in the sync block (Portal 2.2's `sync.name`) counts as sent.
    let carried = if folder_changed { turbosort_of(&new_folder) } else { None };
    // Turning the sync off takes the project folder with it: a `turbosort` without a `sync` is a
    // two-way mirror to every FioTransfer, and off means nothing is synced.
    let synced = |f: &Value| f.get("sync").is_some_and(|s| !s.is_null());
    let sync_removed = synced(&old_folder) && !synced(&new_folder);
    let new_turbosort = match turbosort_change {
        Some(v) => v,
        None if sync_removed => None,
        None => carried.or_else(|| turbosort_of(&old_folder)),
    };
    if let Some(obj) = new_folder.as_object_mut() {
        match &new_turbosort {
            Some(ts) => { obj.insert("turbosort".to_string(), json!(ts)); }
            None => { obj.remove("turbosort"); }
        }
    }
    if synced(&new_folder) {
        let names = lane_names(&state.db).await;
        if let Err(r) = ensure_project_folder(&mut new_folder, &names) {
            return r;
        }
    }
    let send_link = match body.get("send_link") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return detail(StatusCode::UNPROCESSABLE_ENTITY, "send_link must be true or false"),
    };

    if new_folder != old_folder {
        if folder_changed && new_folder.get("folder_id") != old_folder.get("folder_id") {
            // The folder is now exactly where uploads land: drop any older base-subfolder override.
            // A `folder` sent only to carry a new `sync` leaves the override alone.
            let _ = sqlx::query(
                "UPDATE links SET folder = $1, target_folder_id = NULL, target_folder_name = NULL WHERE id = $2",
            )
            .bind(&new_folder).bind(id).execute(&state.db).await;
        } else {
            let _ = sqlx::query("UPDATE links SET folder = $1 WHERE id = $2")
                .bind(&new_folder).bind(id).execute(&state.db).await;
        }
    }
    if let Some(on) = send_link {
        let _ = sqlx::query("UPDATE links SET send_link = $1 WHERE id = $2").bind(on).bind(id).execute(&state.db).await;
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
    crate::services::return_links::sync(&state, id).await;
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
    crate::services::return_links::sync(&state, id).await;
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
    crate::services::return_links::sync(&state, id).await;
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
        "SELECT id, uploader_name, uploader_email, uploader_project, status, frameio_asset_ids, total_bytes, uploaded_bytes,
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
                "uploader_project": r.get::<Option<String>, _>("uploader_project"),
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
    fn effective_name_prefers_own_then_the_job_folder() {
        let folder = json!({ "folder_name": "RUSHES", "path": [
            { "id": "r", "name": "ACME (root)" }, { "id": "j", "name": "2026_10_30_TestMcTest" }, { "id": "f", "name": "RUSHES" },
        ] });
        assert_eq!(effective_name(Some("Cards".into()), Some("Send us".into()), &folder), "Cards");
        // Unnamed: the job folder below the project, not the folder it delivers into or the title.
        assert_eq!(effective_name(Some("  ".into()), Some("Send us".into()), &folder), "2026_10_30_TestMcTest");
        // Delivering into the project root: the project's name.
        let root = json!({ "folder_name": "ACME", "path": [{ "id": "r", "name": "ACME (root)" }] });
        assert_eq!(effective_name(None, None, &root), "ACME");
        // No breadcrumb: the folder's name, then the page title.
        assert_eq!(effective_name(None, None, &json!({ "folder_name": "Day 3" })), "Day 3");
        assert_eq!(effective_name(None, Some("Send us".into()), &Value::Null), "Send us");
        assert_eq!(effective_name(None, None, &Value::Null), "Untitled");
    }

    #[test]
    fn extensions_are_normalized() {
        let v = json!([".MOV", "mov", " r3d ", ""]);
        assert_eq!(normalize_extensions(Some(&v)), Some(json!(["mov", "r3d"])));
        assert_eq!(normalize_extensions(Some(&json!([]))), None);
    }

    #[test]
    fn turbosort_is_one_folder_name() {
        assert_eq!(clean_turbosort(" 20261028_ACME ").ok(), Some("20261028_ACME".to_string()));
        for bad in [".", "..", "a/b", "a\\b", "a\nb", "a\tb", "a\u{0}b"] {
            assert!(clean_turbosort(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(clean_turbosort("   ").is_err());
    }

    const LINK: &str = "11111111-1111-4111-8111-111111111111";
    const EXPORTS: &str = "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3";
    const OTHER: &str = "8008cf70-f28e-474d-a209-d67afb9fddb6";

    fn names() -> LaneNames {
        LaneNames::default()
    }

    fn valid(raw: Value) -> Result<Option<Value>, String> {
        validate_sync(Some(&raw), LINK, &names()).map(|v| v.map(|v| v.sync))
    }

    #[test]
    fn sync_is_optional_and_null_clears_it() {
        assert_eq!(validate_sync(None, LINK, &names()), Ok(None));
        assert_eq!(validate_sync(Some(&Value::Null), LINK, &names()), Ok(None));
    }

    #[test]
    fn lanes_are_stored_in_chaos_shape_with_the_settings_names() {
        // Her composer's payload is stored as sent.
        let raw = json!({
            "mode": "add_only",
            "drive": { "local": "1_DRIVE", "folder_id": OTHER },
            "exports": { "local": "0_EXPORTS", "folder_id": EXPORTS },
        });
        assert_eq!(valid(raw.clone()), Ok(Some(raw)));
        // A lane without a name gets the one from Settings; `mode` is always stored.
        let mine = LaneNames { imports: "RUSHES".into(), exports: "RENDER".into() };
        let got = validate_sync(Some(&json!({ "drive": {}, "exports": { "folder_id": EXPORTS } })), LINK, &mine).unwrap().unwrap();
        assert_eq!(got.sync, json!({ "mode": "add_only", "drive": { "local": "RUSHES" }, "exports": { "local": "RENDER", "folder_id": EXPORTS } }));
        assert_eq!(got.name, None);
    }

    #[test]
    fn a_missing_lane_is_off() {
        // Exports only: nothing comes down.
        assert_eq!(
            valid(json!({ "exports": { "local": "0_EXPORTS", "folder_id": EXPORTS } })),
            Ok(Some(json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS", "folder_id": EXPORTS } })))
        );
        // Imports only: nothing goes up.
        assert_eq!(valid(json!({ "drive": null, "exports": null, "upload": null, "download": { "local": "IN" } })),
            Ok(Some(json!({ "mode": "add_only", "drive": { "local": "IN" } }))));
        // Both named off is not a sync.
        assert_eq!(
            valid(json!({ "drive": null, "exports": null })),
            Err("sync needs Imports (sync.drive) or Exports (sync.exports)".to_string())
        );
    }

    #[test]
    fn portal_2_2_spellings_are_taken_and_stored_in_this_shape() {
        // `{}` was "on": Imports, with its defaults.
        assert_eq!(valid(json!({})), Ok(Some(json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" } }))));
        // `download`/`upload` are `drive`/`exports`; `name` is handed back as the project folder.
        let got = validate_sync(
            Some(&json!({ "name": " Acme ", "download": { "local": "RUSHES" }, "upload": { "folder_id": EXPORTS, "local": "RENDER" } })),
            LINK,
            &names(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(got.sync, json!({ "mode": "add_only", "drive": { "local": "RUSHES" }, "exports": { "local": "RENDER", "folder_id": EXPORTS } }));
        assert_eq!(got.name.as_deref(), Some("Acme"));
        // One name per lane.
        assert_eq!(
            valid(json!({ "drive": { "local": "A" }, "download": { "local": "B" } })),
            Err("sync.download is Portal 2.2's name for sync.drive: send one of them".to_string())
        );
        assert_eq!(
            valid(json!({ "exports": { "folder_id": EXPORTS }, "upload": { "folder_id": EXPORTS } })),
            Err("sync.upload is Portal 2.2's name for sync.exports: send one of them".to_string())
        );
    }

    #[test]
    fn exports_need_a_folder_of_their_own() {
        assert_eq!(
            valid(json!({ "drive": {}, "exports": { "local": "0_EXPORTS" } })),
            Err("sync.exports.folder_id is required: the Frame.io folder Exports go up to".to_string())
        );
        // Not the folder Imports come down from: the link's own, or the one Imports names.
        assert_eq!(
            valid(json!({ "drive": {}, "exports": { "folder_id": LINK } })),
            Err("sync.exports.folder_id must be a different folder from the one Imports come down from".to_string())
        );
        assert_eq!(
            valid(json!({ "drive": { "folder_id": OTHER }, "exports": { "folder_id": OTHER } })),
            Err("sync.exports.folder_id must be a different folder from the one Imports come down from".to_string())
        );
        // With Imports off, the link's own folder may take Exports.
        assert!(valid(json!({ "exports": { "folder_id": LINK } })).is_ok());
        assert!(valid(json!({ "drive": { "folder_id": OTHER }, "exports": { "folder_id": LINK } })).is_ok());
    }

    #[test]
    fn the_two_lanes_are_different_folders_on_disk() {
        for exports in ["1_DRIVE", " 1_drive "] {
            assert_eq!(
                valid(json!({ "drive": { "local": "1_DRIVE" }, "exports": { "local": exports, "folder_id": EXPORTS } })),
                Err("sync.drive.local and sync.exports.local must be different folders".to_string()),
                "{exports:?}"
            );
        }
        // Alone, a lane may be called anything.
        assert!(valid(json!({ "exports": { "local": "1_DRIVE", "folder_id": EXPORTS } })).is_ok());
    }

    #[test]
    fn sync_names_are_one_path_segment() {
        for bad in [".", "..", "OUT/x", "OUT\\x", "OUT%2Fx", "a\nb", "a\tb"] {
            assert_eq!(
                valid(json!({ "exports": { "local": bad, "folder_id": EXPORTS } })),
                Err("sync.exports.local must be a single folder name".to_string()),
                "{bad:?} must be rejected"
            );
        }
        assert_eq!(valid(json!({ "name": "a".repeat(256) })), Err("sync.name must be a single folder name".to_string()));
        assert!(valid(json!({ "name": "a".repeat(255) })).is_ok());
        // An empty name is no name: the Settings default.
        assert_eq!(valid(json!({ "drive": { "local": "  " } })), Ok(Some(json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" } }))));
    }

    #[test]
    fn sync_shape_errors_name_the_field() {
        assert_eq!(valid(json!("add_only")), Err("sync must be an object".to_string()));
        assert_eq!(valid(json!({ "exports": "OUT" })), Err("sync.exports must be an object".to_string()));
        assert_eq!(valid(json!({ "name": 7 })), Err("sync.name must be a string".to_string()));
        assert_eq!(valid(json!({ "drive": { "local": 7 } })), Err("sync.drive.local must be a string".to_string()));
        assert_eq!(valid(json!({ "mode": "mirror" })), Err("sync.mode must be one of ['add_only']".to_string()));
        assert_eq!(
            valid(json!({ "drive": { "folder_id": "1_DRIVE" } })),
            Err("sync.drive.folder_id must be a Frame.io folder id".to_string())
        );
        assert_eq!(
            valid(json!({ "exports": { "folder_id": "" } })),
            Err("sync.exports.folder_id must be a Frame.io folder id".to_string())
        );
    }

    #[test]
    fn sync_folders_names_imports_and_exports() {
        let link = "link";
        assert_eq!(sync_folders(&json!({ "drive": {} }), link), (Some("link"), None));
        assert_eq!(sync_folders(&json!({ "drive": { "folder_id": "d" }, "exports": { "folder_id": "e" } }), link), (Some("d"), Some("e")));
        assert_eq!(sync_folders(&json!({ "exports": { "folder_id": "e" } }), link), (None, Some("e")));
    }

    #[test]
    fn a_synced_link_always_has_a_project_folder() {
        let at = |path: Value| json!({ "sync": { "drive": {} }, "path": path });
        // None given: the job folder its Imports sit in.
        let mut f = at(json!([{ "id": "r", "name": "ACME (root)" }, { "id": "d", "name": "1_DRIVE" }]));
        ensure_project_folder(&mut f, &names()).unwrap();
        assert_eq!(f["turbosort"], json!("ACME"));
        // One given is kept.
        let mut f = at(json!([{ "id": "r", "name": "ACME (root)" }]));
        f["turbosort"] = json!("20261028_ACME");
        ensure_project_folder(&mut f, &names()).unwrap();
        assert_eq!(f["turbosort"], json!("20261028_ACME"));
        // Not synced: left alone.
        let mut f = json!({ "path": [{ "id": "r", "name": "ACME (root)" }] });
        ensure_project_folder(&mut f, &names()).unwrap();
        assert!(f.get("turbosort").is_none());
        // Nothing to name it after: the caller has to.
        let mut f = json!({ "sync": { "drive": {} } });
        assert!(ensure_project_folder(&mut f, &names()).is_err());
    }
}
