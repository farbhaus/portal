
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

/// The `turbosort` value: the older name for the project's folder on disk, which FioTransfer still
/// reads when `sync.name` is absent. The composer no longer sets it; the API keeps it so links
/// that have one keep their folder. One path segment: no separators, no control characters, not
/// `.`/`..`.
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
    folder.get("turbosort").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// `folder.sync` — the link's "Sync to FioTransfer" switch. Portal only stores it; the FioTransfer
/// client reads it from `GET /api/upload-links` and gives the link a folder on the user's disk:
/// what the client uploads comes down into its `IN`, and what the user puts in `OUT` goes up to
/// the return folder. Files are only ever added, on either side, never deleted.
///
/// Its presence is the switch, so `{}` is a whole sync. Every field in it is optional:
/// - `upload.folder_id`: the return folder `OUT` goes up to; without it there is no `OUT`.
/// - `download.folder_id`: where `IN` comes from, when it isn't the link's own folder.
/// - `name`, `download.local`, `upload.local`: on-disk names, when they shouldn't be the link's
///   name and the `IN`/`OUT` names set in FioTransfer.
///
/// The older lanes are still taken and stored as sent, so links set up that way keep their
/// folders: the client reads `drive` as `download` and `exports` as `upload`, and ignores `mode`
/// (only ever `add_only`).
const SYNC_MODES: [&str; 1] = ["add_only"];

/// One on-disk name: a single path segment of at most 255 bytes, no separators, no `%`, no
/// control characters and not `.`/`..`.
fn clean_sync_name(field: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.contains('%')
        || v.chars().any(char::is_control) || v.len() > 255 {
        return Err(format!("{field} must be a single folder name"));
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

/// One direction of a sync block, cleaned: its optional `local` name and `folder_id`.
fn sync_lane(sync: &Map<String, Value>, key: &str) -> Result<Option<Map<String, Value>>, String> {
    let Some(v) = sync.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(lane) = v.as_object() else { return Err(format!("sync.{key} must be an object")) };
    let mut out = Map::new();
    if let Some(local) = lane.get("local").filter(|v| !v.is_null()) {
        let Some(local) = local.as_str() else { return Err(format!("sync.{key}.local must be a string")) };
        out.insert("local".to_string(), json!(clean_sync_name(&format!("sync.{key}.local"), local)?));
    }
    if let Some(id) = lane.get("folder_id").filter(|v| !v.is_null()) {
        let Some(id) = id.as_str() else { return Err(format!("sync.{key}.folder_id must be a Frame.io folder id")) };
        out.insert("folder_id".to_string(), json!(sync_folder_id(key, id)?));
    }
    Ok(Some(out))
}

/// A cleaned sync direction and the key it came in under (`upload`, or the older `exports`).
type SyncDirection = (String, Map<String, Value>);

/// A direction under its name or the older one it replaced (never both).
fn sync_direction(sync: &Map<String, Value>, key: &str, older: &str) -> Result<Option<SyncDirection>, String> {
    match (sync_lane(sync, key)?, sync_lane(sync, older)?) {
        (Some(_), Some(_)) => Err(format!("sync.{older} is the older name for sync.{key}: send one of them")),
        (Some(lane), None) => Ok(Some((key.to_string(), lane))),
        (None, Some(lane)) => Ok(Some((older.to_string(), lane))),
        (None, None) => Ok(None),
    }
}

/// Validate `folder.sync` for a link delivering into `link_folder_id`, and return it as stored.
/// `None` means the link isn't synced: the key was absent or null. Absent fields stay absent, so
/// FioTransfer applies its own defaults.
pub fn validate_sync(raw: Option<&Value>, link_folder_id: &str) -> Result<Option<Value>, String> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(obj) = raw.as_object() else { return Err("sync must be an object".to_string()) };
    let mut out = Map::new();
    match obj.get("mode") {
        None | Some(Value::Null) => {}
        Some(Value::String(m)) if SYNC_MODES.contains(&m.as_str()) => {
            out.insert("mode".to_string(), json!(m));
        }
        Some(_) => {
            let accepted = SYNC_MODES.map(|m| format!("'{m}'")).join(", ");
            return Err(format!("sync.mode must be one of [{accepted}]"));
        }
    }
    if let Some(name) = obj.get("name").filter(|v| !v.is_null()) {
        let Some(name) = name.as_str() else { return Err("sync.name must be a string".to_string()) };
        out.insert("name".to_string(), json!(clean_sync_name("sync.name", name)?));
    }
    let down = sync_direction(obj, "download", "drive")?;
    let up = sync_direction(obj, "upload", "exports")?;
    let field = |dir: &Option<SyncDirection>, f: &str| {
        dir.as_ref().and_then(|(_, lane)| lane.get(f)).and_then(|v| v.as_str()).map(|s| s.to_string())
    };
    if let (Some((dk, _)), Some((uk, _))) = (&down, &up)
        && field(&down, "local").is_some() && field(&down, "local") == field(&up, "local") {
            return Err(format!("sync.{dk}.local and sync.{uk}.local must be different folders"));
        }
    // OUT goes up somewhere IN doesn't come down from.
    if let (Some((uk, _)), Some(up_folder)) = (&up, field(&up, "folder_id")) {
        let down_folder = field(&down, "folder_id").unwrap_or_else(|| link_folder_id.to_string());
        if up_folder == down_folder {
            return Err(format!("sync.{uk}.folder_id must be a different folder from the one IN comes from"));
        }
    }
    for (key, lane) in [down, up].into_iter().flatten() {
        out.insert(key, Value::Object(lane));
    }
    Ok(Some(Value::Object(out)))
}

/// The folder IN comes down from (its lane's `folder_id`, else the link's own) and the return
/// folder OUT goes up to, if the stored sync block names one.
fn sync_folders<'a>(sync: &'a Value, link_folder_id: &'a str) -> (&'a str, Option<&'a str>) {
    fn lane<'v>(sync: &'v Value, keys: [&str; 2]) -> Option<&'v str> {
        keys.iter().find_map(|k| sync.get(*k)?.get("folder_id")?.as_str())
    }
    (lane(sync, ["download", "drive"]).unwrap_or(link_folder_id), lane(sync, ["upload", "exports"]))
}

/// A Frame.io folder name made usable as one folder on disk: separators and `%` become `-`,
/// control characters go, at most 255 bytes. None when nothing usable is left.
fn disk_name(name: &str) -> Option<String> {
    let mut out: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '/' | '\\' | '%') { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string();
    while out.len() > 255 {
        out.pop();
    }
    let out = out.trim().to_string();
    (!out.is_empty() && out != "." && out != "..").then_some(out)
}

/// FioTransfer names the link's two folders on disk after their `local` names; Portal fills the
/// missing ones with their Frame.io folders' names, so the folders on disk mirror Frame.io: the
/// client's uploads come down into a folder named like the link's (unless its lane names another
/// Frame.io folder), and the return folder's twin is named like the return folder. A name already
/// there is kept. If both would be the same, the return one is left to FioTransfer's default.
fn fill_local_names(sync: &mut Value, link_folder_name: &str, return_folder_name: Option<&str>) {
    let Some(obj) = sync.as_object_mut() else { return };
    let down_key = if obj.contains_key("drive") && !obj.contains_key("download") { "drive" } else { "download" };
    let up_key = if obj.contains_key("exports") && !obj.contains_key("upload") { "exports" } else { "upload" };
    let down = obj.entry(down_key).or_insert_with(|| json!({}));
    if down.get("folder_id").is_none()
        && down.get("local").is_none()
        && let (Some(lane), Some(name)) = (down.as_object_mut(), disk_name(link_folder_name))
    {
        lane.insert("local".to_string(), json!(name));
    }
    let down_local = obj.get(down_key).and_then(|l| l.get("local")).and_then(|v| v.as_str()).map(str::to_string);
    if let Some(lane) = obj.get_mut(up_key).and_then(|l| l.as_object_mut())
        && lane.contains_key("folder_id")
        && !lane.contains_key("local")
        && let Some(name) = return_folder_name.and_then(disk_name)
        && Some(&name) != down_local.as_ref()
    {
        lane.insert("local".to_string(), json!(name));
    }
}

/// The columns every admin view of an upload link reads.
pub const LINK_COLUMNS: &str = "SELECT l.id, l.token, l.name, l.expires_at, l.password_hash, l.max_file_size,
        l.verify_email, l.allowed_extensions, l.uploader_fields_required, l.revoked_at,
        l.brand_display_name, l.brand_subtitle, l.created_at, l.target_folder_id,
        l.target_folder_name, l.subfolder_template, l.folder,
        l.notify_emails, l.remind_before_expiry, r.id AS return_link_id, r.token AS return_token
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
    let sync = validate_sync(raw.get("sync"), &folder_id).map_err(|m| detail(StatusCode::UNPROCESSABLE_ENTITY, &m))?;
    let folder = match state.frameio.get_folder(&account_id, &folder_id).await {
        Ok(f) => f,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return Err(detail(StatusCode::CONFLICT, "Frame.io is not connected"))
        }
        Err(_) => return Err(detail(StatusCode::BAD_REQUEST, "Frame.io folder not found or inaccessible")),
    };
    // The return folder's place in Frame.io (best-effort: None if Frame.io can't say).
    let return_chain = match sync.as_ref().and_then(|s| sync_folders(s, &folder_id).1) {
        Some(up) => state.frameio.folder_ancestry(&account_id, up).await.ok(),
        None => None,
    };
    // It can't be inside the folder the client's uploads come down from: FioTransfer brings that
    // folder down whole, so what goes up would come straight back down.
    if let (Some(sync), Some(chain)) = (&sync, &return_chain)
        && let (down, Some(up)) = sync_folders(sync, &folder_id)
        && chain.iter().any(|f| f.id == down && f.id != up)
    {
        return Err(detail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The return folder can't be inside the folder the client's uploads come down from: what you add to it would come straight back down",
        ));
    }
    let return_name = return_chain.as_ref().and_then(|c| c.last()).map(|f| f.name.clone());
    let sync = sync.map(|mut sync| {
        fill_local_names(&mut sync, &folder.name, return_name.as_deref());
        sync
    });
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
            brand_display_name, brand_subtitle, notify_emails, remind_before_expiry)
         VALUES ('upload',$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING id",
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
    const RETURN: &str = "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3";
    const OTHER: &str = "8008cf70-f28e-474d-a209-d67afb9fddb6";

    #[test]
    fn sync_is_optional_and_null_clears_it() {
        assert_eq!(validate_sync(None, LINK), Ok(None));
        assert_eq!(validate_sync(Some(&Value::Null), LINK), Ok(None));
    }

    #[test]
    fn an_empty_sync_is_the_switch_on() {
        // Nothing is invented: FioTransfer fills in its own defaults.
        assert_eq!(validate_sync(Some(&json!({})), LINK), Ok(Some(json!({}))));
    }

    #[test]
    fn sync_keeps_the_return_folder_and_names_as_given() {
        let raw = json!({ "upload": { "folder_id": format!(" {RETURN} ") } });
        assert_eq!(validate_sync(Some(&raw), LINK), Ok(Some(json!({ "upload": { "folder_id": RETURN } }))));
        let raw = json!({
            "name": " Acme Spot 30s ",
            "download": { "folder_id": OTHER, "local": "IN" },
            "upload": { "folder_id": RETURN, "local": "OUT" },
        });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Ok(Some(json!({
                "name": "Acme Spot 30s",
                "download": { "folder_id": OTHER, "local": "IN" },
                "upload": { "folder_id": RETURN, "local": "OUT" },
            })))
        );
    }

    #[test]
    fn older_lanes_are_stored_as_sent() {
        // Links set up before the switch keep their folders: same keys, same values.
        let raw = json!({
            "mode": "add_only",
            "drive": { "local": " 1_DRIVE " },
            "exports": { "local": "0_EXPORTS", "folder_id": RETURN },
        });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Ok(Some(json!({
                "mode": "add_only",
                "drive": { "local": "1_DRIVE" },
                "exports": { "local": "0_EXPORTS", "folder_id": RETURN },
            })))
        );
        assert_eq!(
            validate_sync(Some(&json!({ "mode": "mirror" })), LINK),
            Err("sync.mode must be one of ['add_only']".to_string())
        );
    }

    #[test]
    fn a_direction_comes_under_one_name_only() {
        let raw = json!({ "upload": { "folder_id": RETURN }, "exports": { "local": "0_EXPORTS" } });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Err("sync.exports is the older name for sync.upload: send one of them".to_string())
        );
        let raw = json!({ "download": {}, "drive": { "local": "1_DRIVE" } });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Err("sync.drive is the older name for sync.download: send one of them".to_string())
        );
        // A null side is no side.
        let raw = json!({ "drive": null, "download": { "local": "IN" } });
        assert_eq!(validate_sync(Some(&raw), LINK), Ok(Some(json!({ "download": { "local": "IN" } }))));
    }

    #[test]
    fn sync_names_are_one_path_segment() {
        for bad in [".", "..", "IN/out", "IN\\out", "IN%2Fout", "a\nb", "a\tb"] {
            let raw = json!({ "upload": { "local": bad } });
            assert_eq!(
                validate_sync(Some(&raw), LINK),
                Err("sync.upload.local must be a single folder name".to_string()),
                "{bad:?} must be rejected"
            );
        }
        let long = "a".repeat(256);
        assert_eq!(
            validate_sync(Some(&json!({ "name": long })), LINK),
            Err("sync.name must be a single folder name".to_string())
        );
        assert!(validate_sync(Some(&json!({ "name": "a".repeat(255) })), LINK).is_ok());
        for empty in ["", "  "] {
            assert_eq!(
                validate_sync(Some(&json!({ "drive": { "local": empty } })), LINK),
                Err("sync.drive.local must not be empty".to_string()),
                "{empty:?} must be rejected"
            );
        }
    }

    #[test]
    fn sync_folder_ids_must_be_frameio_ids() {
        assert_eq!(
            validate_sync(Some(&json!({ "download": { "folder_id": "1_DRIVE" } })), LINK),
            Err("sync.download.folder_id must be a Frame.io folder id".to_string())
        );
        assert_eq!(
            validate_sync(Some(&json!({ "exports": { "local": "0_EXPORTS", "folder_id": "" } })), LINK),
            Err("sync.exports.folder_id must be a Frame.io folder id".to_string())
        );
    }

    #[test]
    fn sync_shape_errors_name_the_field() {
        assert_eq!(validate_sync(Some(&json!("add_only")), LINK), Err("sync must be an object".to_string()));
        assert_eq!(
            validate_sync(Some(&json!({ "upload": "OUT" })), LINK),
            Err("sync.upload must be an object".to_string())
        );
        assert_eq!(validate_sync(Some(&json!({ "name": 7 })), LINK), Err("sync.name must be a string".to_string()));
    }

    #[test]
    fn the_return_folder_is_not_where_in_comes_from() {
        // IN defaults to the link's own folder.
        assert_eq!(
            validate_sync(Some(&json!({ "upload": { "folder_id": LINK } })), LINK),
            Err("sync.upload.folder_id must be a different folder from the one IN comes from".to_string())
        );
        let raw = json!({ "download": { "folder_id": RETURN }, "upload": { "folder_id": RETURN } });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Err("sync.upload.folder_id must be a different folder from the one IN comes from".to_string())
        );
        // IN moved elsewhere frees the link's own folder for OUT.
        let raw = json!({ "download": { "folder_id": OTHER }, "upload": { "folder_id": LINK } });
        assert!(validate_sync(Some(&raw), LINK).is_ok());
    }

    #[test]
    fn local_names_mirror_the_frameio_folders() {
        let mut sync = json!({ "upload": { "folder_id": "r" } });
        fill_local_names(&mut sync, "RUSHES", Some("RENDER"));
        assert_eq!(sync, json!({ "download": { "local": "RUSHES" }, "upload": { "folder_id": "r", "local": "RENDER" } }));
        // The switch alone: only the client's uploads come down.
        let mut sync = json!({});
        fill_local_names(&mut sync, "RUSHES", None);
        assert_eq!(sync, json!({ "download": { "local": "RUSHES" } }));
        // Names already there are kept, older lanes included.
        let mut sync = json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" }, "exports": { "local": "0_EXPORTS", "folder_id": "r" } });
        fill_local_names(&mut sync, "RUSHES", Some("RENDER"));
        assert_eq!(sync, json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" }, "exports": { "local": "0_EXPORTS", "folder_id": "r" } }));
        // A lane bringing down another Frame.io folder isn't named after the link's.
        let mut sync = json!({ "download": { "folder_id": "d" } });
        fill_local_names(&mut sync, "RUSHES", None);
        assert_eq!(sync, json!({ "download": { "folder_id": "d" } }));
        // Same name on both sides: the return one is left to FioTransfer.
        let mut sync = json!({ "upload": { "folder_id": "r" } });
        fill_local_names(&mut sync, "MEDIA", Some("MEDIA"));
        assert_eq!(sync, json!({ "download": { "local": "MEDIA" }, "upload": { "folder_id": "r" } }));
    }

    #[test]
    fn disk_names_are_one_folder() {
        assert_eq!(disk_name(" RENDER "), Some("RENDER".to_string()));
        assert_eq!(disk_name("Grade 50/50 v2"), Some("Grade 50-50 v2".to_string()));
        assert_eq!(disk_name("100%\\done"), Some("100--done".to_string()));
        assert_eq!(disk_name("a\nb"), Some("ab".to_string()));
        assert_eq!(disk_name(".."), None);
        assert_eq!(disk_name("  "), None);
        assert_eq!(disk_name(&"é".repeat(200)).map(|n| n.len()), Some(254));
    }

    #[test]
    fn sync_folders_names_in_and_out() {
        let link = "link";
        assert_eq!(sync_folders(&json!({}), link), ("link", None));
        assert_eq!(sync_folders(&json!({ "upload": { "folder_id": "r" } }), link), ("link", Some("r")));
        assert_eq!(sync_folders(&json!({ "download": { "folder_id": "d" }, "upload": { "folder_id": "r" } }), link), ("d", Some("r")));
        // Older lanes count the same way.
        assert_eq!(sync_folders(&json!({ "drive": { "folder_id": "d" }, "exports": { "local": "0_EXPORTS", "folder_id": "r" } }), link), ("d", Some("r")));
        assert_eq!(sync_folders(&json!({ "exports": { "local": "0_EXPORTS" } }), link), ("link", None));
    }

    #[test]
    fn sync_sides_must_name_different_local_folders() {
        let raw = json!({ "drive": { "local": "1_DRIVE" }, "exports": { "local": " 1_DRIVE " } });
        assert_eq!(
            validate_sync(Some(&raw), LINK),
            Err("sync.drive.local and sync.exports.local must be different folders".to_string())
        );
        // The rule only bites when both sides name one: one side may be named anything.
        let raw = json!({ "upload": { "local": "IN" } });
        assert_eq!(validate_sync(Some(&raw), LINK), Ok(Some(json!({ "upload": { "local": "IN" } }))));
    }
}
