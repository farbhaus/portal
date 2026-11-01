
//! `GET /api/links` — every upload and download link in one list, newest first, with what the
//! Links page shows on each row: where an upload link delivers, what has happened on the link, and
//! any upload arriving right now.
//!
//! DB-only (never calls Frame.io): a handful of grouped queries, whatever the number of links.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::routes::{download_links, upload_links};
use crate::state::AppState;

/// An upload is "arriving now" while its page keeps sending heartbeats (every 20s).
const LIVE_WINDOW_SECONDS: i64 = 90;

fn iso(d: Option<DateTime<Utc>>) -> Value {
    d.map(|d| json!(crate::pyfmt::iso(&d))).unwrap_or(Value::Null)
}

fn who(name: Option<String>, email: Option<String>) -> Option<String> {
    name.filter(|s| !s.trim().is_empty()).or(email)
}

/// Sum of the per-file sizes an in-flight session has announced so far.
fn planned_bytes(ids: &Option<Value>) -> Option<i64> {
    let total: i64 = ids
        .as_ref()?
        .as_array()?
        .iter()
        .filter_map(|e| e.get("size").and_then(|s| s.as_i64()))
        .sum();
    if total > 0 { Some(total) } else { None }
}

/// Average speed so far, in bytes per second.
pub fn average_bps(uploaded: Option<i64>, started: Option<DateTime<Utc>>, last: Option<DateTime<Utc>>) -> Option<i64> {
    let (bytes, start, end) = (uploaded?, started?, last?);
    let secs = (end - start).num_milliseconds() as f64 / 1000.0;
    if bytes <= 0 || secs < 1.0 {
        return None;
    }
    Some((bytes as f64 / secs) as i64)
}

pub async fn list_links(State(state): State<AppState>, _: Admin) -> Response {
    let rows = match sqlx::query("SELECT * FROM links ORDER BY created_at DESC").fetch_all(&state.db).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("links.list_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    // ── upload activity ─────────────────────────────────────────────────────
    let mut up_stats: HashMap<Uuid, (i64, i64, Option<DateTime<Utc>>)> = HashMap::new();
    for r in sqlx::query(
        "SELECT upload_link_id,
                count(*) FILTER (WHERE status = 'completed') AS uploads,
                COALESCE(sum(total_bytes) FILTER (WHERE status = 'completed'), 0)::bigint AS bytes,
                max(completed_at) AS last_at
           FROM upload_sessions GROUP BY upload_link_id",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    {
        up_stats.insert(r.get("upload_link_id"), (r.get("uploads"), r.get("bytes"), r.get("last_at")));
    }
    let mut up_last_who: HashMap<Uuid, Option<String>> = HashMap::new();
    for r in sqlx::query(
        "SELECT DISTINCT ON (upload_link_id) upload_link_id, uploader_name, uploader_email
           FROM upload_sessions WHERE status = 'completed'
          ORDER BY upload_link_id, completed_at DESC NULLS LAST",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    {
        up_last_who.insert(r.get("upload_link_id"), who(r.get("uploader_name"), r.get("uploader_email")));
    }
    let mut live: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for r in sqlx::query(
        "SELECT id, upload_link_id, uploader_name, uploader_email, uploaded_bytes, total_bytes,
                frameio_asset_ids, started_at, last_activity_at
           FROM upload_sessions
          WHERE status = 'in_progress'
            AND COALESCE(last_activity_at, started_at, created_at) >= now() - make_interval(secs => $1)
          ORDER BY started_at",
    )
    .bind(LIVE_WINDOW_SECONDS as f64)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    {
        let ids: Option<Value> = r.get("frameio_asset_ids");
        let uploaded: Option<i64> = r.get("uploaded_bytes");
        let started: Option<DateTime<Utc>> = r.get("started_at");
        let last: Option<DateTime<Utc>> = r.get("last_activity_at");
        live.entry(r.get("upload_link_id")).or_default().push(json!({
            "id": r.get::<Uuid, _>("id").to_string(),
            "who": who(r.get("uploader_name"), r.get("uploader_email")),
            "uploaded_bytes": uploaded,
            "total_bytes": r.get::<Option<i64>, _>("total_bytes").or_else(|| planned_bytes(&ids)),
            "files": ids.as_ref().and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
            "started_at": iso(started),
            "last_activity_at": iso(last),
            "avg_bps": average_bps(uploaded, started, last),
        }));
    }

    // ── download activity ───────────────────────────────────────────────────
    let mut down_stats: HashMap<Uuid, (i64, Option<DateTime<Utc>>)> = HashMap::new();
    for r in sqlx::query(
        "SELECT s.download_link_id, count(e.id) AS downloads, max(e.started_at) AS last_at
           FROM download_sessions s JOIN download_events e ON e.download_session_id = s.id
          GROUP BY s.download_link_id",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    {
        down_stats.insert(r.get("download_link_id"), (r.get("downloads"), r.get("last_at")));
    }
    let mut down_last_who: HashMap<Uuid, Option<String>> = HashMap::new();
    for r in sqlx::query(
        "SELECT DISTINCT ON (s.download_link_id) s.download_link_id, s.viewer_name, s.viewer_email
           FROM download_sessions s JOIN download_events e ON e.download_session_id = s.id
          ORDER BY s.download_link_id, e.started_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    {
        down_last_who.insert(r.get("download_link_id"), who(r.get("viewer_name"), r.get("viewer_email")));
    }

    // ── render, newest first ────────────────────────────────────────────────
    let mut items: Vec<Value> = Vec::with_capacity(rows.len());
    for row in rows.iter().filter(|r| r.get::<String, _>("kind") == "upload") {
        let id: Uuid = row.get("id");
        let mut v = upload_links::render_row(&state, row);
        let (count, bytes, last_at) = up_stats.get(&id).cloned().unwrap_or((0, 0, None));
        v["activity"] = json!({
            "count": count,
            "bytes": bytes,
            "last_at": iso(last_at),
            "last_who": up_last_who.get(&id).cloned().flatten(),
            "live": live.remove(&id).unwrap_or_default(),
        });
        items.push(v);
    }
    for row in rows.iter().filter(|r| r.get::<String, _>("kind") == "download") {
        let id: Uuid = row.get("id");
        let mut v = download_links::render_row(&state, row);
        let (count, last_at) = down_stats.get(&id).cloned().unwrap_or((0, None));
        v["activity"] = json!({
            "count": count,
            "last_at": iso(last_at),
            "last_who": down_last_who.get(&id).cloned().flatten(),
            "live": [],
        });
        items.push(v);
    }
    // Both kinds come from one table ordered newest first; keep that order across kinds.
    let order: HashMap<String, usize> = rows.iter().enumerate().map(|(i, r)| (r.get::<Uuid, _>("id").to_string(), i)).collect();
    items.sort_by_key(|v| v["id"].as_str().and_then(|id| order.get(id)).copied().unwrap_or(usize::MAX));
    Json(Value::Array(items)).into_response()
}

/// `POST /api/links/email` — send a link to a client by email, from the sender address in
/// Settings. Body: `{kind: "upload"|"download", id, to, note?}`. One recipient per message.
pub async fn email_link(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let Some(id) = body.get("id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok()) else {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Field required");
    };
    let to = body.get("to").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if !to.contains('@') || to.contains(',') || to.contains(';') {
        return detail(StatusCode::BAD_REQUEST, "Enter one email address");
    }
    let note = body.get("note").and_then(|v| v.as_str()).map(|s| s.chars().take(2000).collect::<String>());
    let path = match kind {
        "upload" => "u",
        "download" => "d",
        _ => return detail(StatusCode::BAD_REQUEST, "kind must be upload or download"),
    };
    let row = sqlx::query(
        "SELECT token, name, brand_display_name, revoked_at, expires_at, brand_profile_id FROM links WHERE kind = $1 AND id = $2",
    )
    .bind(kind)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return detail(StatusCode::NOT_FOUND, "Link not found") };
    if upload_links::link_state(row.get("revoked_at"), row.get("expires_at")) != "ok" {
        return detail(StatusCode::CONFLICT, "This link is no longer active");
    }
    let config = crate::services::runtime_config::get_email_config(&state.db, &state.settings).await;
    if !config.configured() {
        return detail(StatusCode::CONFLICT, "Email isn't set up yet. Add an SMTP server in Settings.");
    }
    let brand = crate::services::runtime_config::email_brand_for(&state.db, &state.settings, row.get("brand_profile_id")).await;
    let token: String = row.get("token");
    let url = format!("{}/{path}/{token}", state.settings.base_url.trim_end_matches('/'));
    let title = row
        .get::<Option<String>, _>("brand_display_name")
        .filter(|s| !s.trim().is_empty())
        .or_else(|| row.get::<Option<String>, _>("name"))
        .unwrap_or_else(|| "Files".to_string());
    let (subject, text, html) = crate::notify::render_link_invite(kind, &title, &url, note.as_deref(), &brand);
    if let Err(e) = crate::notify::send_email(&config, &to, &subject, &text, Some(&html)).await {
        return detail(StatusCode::BAD_GATEWAY, &e.0);
    }
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'link.emailed',$2)")
        .bind(admin.id)
        .bind(json!({ "kind": kind, "link_id": id.to_string() }))
        .execute(&state.db)
        .await;
    Json(json!({ "ok": true })).into_response()
}

/// `GET /api/nav` — the counts the navigation shows: active links and uploads arriving now. One
/// query; pages fetch it again on every live change.
pub async fn nav_summary(State(state): State<AppState>, _: Admin) -> Response {
    let row = sqlx::query(
        "SELECT
            (SELECT count(*) FROM links WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now()))
              AS links_active,
            (SELECT count(*) FROM upload_sessions WHERE status = 'in_progress'
                AND COALESCE(last_activity_at, started_at, created_at) >= now() - make_interval(secs => $1))
              AS uploads_live",
    )
    .bind(LIVE_WINDOW_SECONDS as f64)
    .fetch_one(&state.db)
    .await;
    match row {
        Ok(r) => Json(json!({
            "links_active": r.get::<i64, _>("links_active"),
            "uploads_live": r.get::<i64, _>("uploads_live"),
        }))
        .into_response(),
        Err(e) => {
            tracing::error!("links.nav_failed: {e}");
            detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn average_speed_needs_bytes_and_time() {
        let t0 = Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap();
        let t1 = t0 + chrono::Duration::seconds(100);
        assert_eq!(average_bps(Some(10_400_000_000), Some(t0), Some(t1)), Some(104_000_000));
        assert_eq!(average_bps(Some(0), Some(t0), Some(t1)), None);
        assert_eq!(average_bps(Some(5), Some(t0), Some(t0)), None);
        assert_eq!(average_bps(None, Some(t0), Some(t1)), None);
    }

    #[test]
    fn planned_bytes_sums_announced_files() {
        assert_eq!(planned_bytes(&Some(json!([{ "size": 10 }, { "size": 5 }, {}]))), Some(15));
        assert_eq!(planned_bytes(&Some(json!([]))), None);
        assert_eq!(planned_bytes(&None), None);
    }
}
