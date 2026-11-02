
//! `GET /api/dashboard/transfers` — the operations view, mirroring `portal.routes.dashboard`.
//!
//! The recent-transfer feed merges completed uploads and file downloads newest-first. The sort
//! is stable and uploads are appended first, so ties resolve the same way every time.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::Admin;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct TransfersQuery {
    limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    kind: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// `GET /api/dashboard/history` — paginated, newest-first merge of finished uploads and file
/// downloads. `kind` = all | upload | download; `q` searches names and people.
pub async fn history(State(state): State<AppState>, _: Admin, Query(q): Query<HistoryQuery>) -> Response {
    let limit = q.limit.unwrap_or(50).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);
    let kind = q.kind.unwrap_or_else(|| "all".to_string());
    let want = |k: &str| kind == "all" || kind == k;
    // ILIKE pattern, with the user's % and _ taken literally.
    let pattern = q
        .q
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let take = offset + limit + 1;
    let mut pool: Vec<(DateTime<Utc>, Value)> = Vec::new();

    if want("upload") {
        let rows = sqlx::query(
            "SELECT s.id, s.uploader_name, s.uploader_email, s.uploaded_bytes, s.total_bytes,
                    s.status, s.started_at, s.completed_at, s.last_activity_at, s.created_at,
                    s.frameio_asset_ids, l.id AS link_id,
                    COALESCE(NULLIF(l.name, ''), l.brand_display_name) AS brand_display_name
               FROM upload_sessions s JOIN links l ON s.upload_link_id = l.id
              WHERE (s.status = 'completed'
                     OR (s.status = 'abandoned' AND s.frameio_asset_ids IS NOT NULL
                         AND jsonb_array_length(s.frameio_asset_ids) > 0))
                AND ($2::text IS NULL OR concat_ws(' ', l.name, l.brand_display_name, s.uploader_name, s.uploader_email) ILIKE $2)
              ORDER BY COALESCE(s.completed_at, s.last_activity_at, s.started_at, s.created_at) DESC
              LIMIT $1",
        )
        .bind(take)
        .bind(&pattern)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        for row in &rows {
            let ids: Option<Value> = row.get("frameio_asset_ids");
            let status: String = row.get("status");
            let completed = status == "completed";
            let total: Option<i64> = row.get("total_bytes");
            let uploaded: Option<i64> = row.get("uploaded_bytes");
            let at = row
                .get::<Option<DateTime<Utc>>, _>("completed_at")
                .or_else(|| row.get::<Option<DateTime<Utc>>, _>("last_activity_at"))
                .or_else(|| row.get::<Option<DateTime<Utc>>, _>("started_at"))
                .unwrap_or_else(|| at_of(row, "created_at"));
            let size = if completed { total } else { uploaded.or_else(|| planned_bytes(&ids)) };
            let brand: Option<String> = row.get("brand_display_name");
            let files = ids.as_ref().and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            pool.push((at, json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "upload",
                "label": brand.unwrap_or_else(|| "Upload".to_string()),
                "who": who(row.get("uploader_name"), row.get("uploader_email")),
                "bytes": size,
                "files": files,
                "status": if completed { "completed" } else { "incomplete" },
                "at": crate::pyfmt::iso(&at),
                "link_id": row.get::<uuid::Uuid, _>("link_id").to_string(),
            })));
        }
    }
    if want("download") {
        let rows = sqlx::query(
            "SELECT e.id, e.file_name, e.bytes_served, e.completed, e.started_at, ds.viewer_email,
                    ds.viewer_name, ds.download_link_id, COALESCE(NULLIF(l.name, ''), l.brand_display_name) AS link_name
               FROM download_events e
               JOIN download_sessions ds ON e.download_session_id = ds.id
               JOIN links l ON l.id = ds.download_link_id
              WHERE ($2::text IS NULL OR concat_ws(' ', e.file_name, ds.viewer_email, ds.viewer_name, l.name) ILIKE $2)
              ORDER BY e.started_at DESC LIMIT $1",
        )
        .bind(take)
        .bind(&pattern)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        for row in &rows {
            let at: DateTime<Utc> = row.get("started_at");
            let completed: bool = row.get("completed");
            pool.push((at, json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "download",
                "label": row.get::<Option<String>, _>("file_name").unwrap_or_else(|| "Download".to_string()),
                "who": who(row.get("viewer_name"), row.get("viewer_email")),
                "bytes": row.get::<Option<i64>, _>("bytes_served"),
                "files": 1,
                "status": if completed { "completed" } else { "in_progress" },
                "at": crate::pyfmt::iso(&at),
                "link_id": row.get::<uuid::Uuid, _>("download_link_id").to_string(),
                "via": row.get::<Option<String>, _>("link_name"),
            })));
        }
    }
    pool.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let window: Vec<Value> = pool
        .into_iter()
        .skip(offset as usize)
        .take((limit + 1) as usize)
        .map(|(_, v)| v)
        .collect();
    let has_more = window.len() as i64 > limit;
    let items: Vec<Value> = window.into_iter().take(limit as usize).collect();
    Json(json!({ "items": items, "has_more": has_more })).into_response()
}

fn planned_bytes(ids: &Option<Value>) -> Option<i64> {
    let arr = ids.as_ref()?.as_array()?;
    let total: i64 = arr
        .iter()
        .filter_map(|e| e.as_object())
        .filter_map(|e| e.get("size"))
        .filter_map(|s| s.as_i64().or_else(|| s.as_str().and_then(|x| x.parse().ok())))
        .sum();
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

fn who(name: Option<String>, email: Option<String>) -> Option<String> {
    name.or(email)
}

fn at_of(row: &sqlx::postgres::PgRow, which: &str) -> DateTime<Utc> {
    row.get::<DateTime<Utc>, _>(which)
}

pub async fn transfers(State(state): State<AppState>, _: Admin, Query(q): Query<TransfersQuery>) -> Response {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);

    let mut feed: Vec<(DateTime<Utc>, Value)> = Vec::new();

    // --- live uploads in flight ------------------------------------------------
    let active = sqlx::query(
        "SELECT s.id, s.uploader_name, s.uploader_email, s.uploaded_bytes, s.total_bytes,
                s.started_at, s.last_activity_at, s.frameio_asset_ids, l.id AS link_id,
                COALESCE(NULLIF(l.name, ''), l.brand_display_name) AS brand_display_name
           FROM upload_sessions s JOIN links l ON s.upload_link_id = l.id
          WHERE s.status = 'in_progress'
            AND COALESCE(s.last_activity_at, s.started_at, s.created_at) >= now() - interval '24 hours'
          ORDER BY s.started_at DESC NULLS LAST
          LIMIT 50",
    )
    .fetch_all(&state.db)
    .await;

    // --- completed / abandoned uploads -----------------------------------------
    let uploads = sqlx::query(
        "SELECT s.id, s.uploader_name, s.uploader_email, s.uploaded_bytes, s.total_bytes,
                s.status, s.started_at, s.completed_at, s.last_activity_at, s.created_at,
                s.frameio_asset_ids, COALESCE(NULLIF(l.name, ''), l.brand_display_name) AS brand_display_name
           FROM upload_sessions s JOIN links l ON s.upload_link_id = l.id
          WHERE s.status = 'completed'
             OR (s.status = 'abandoned' AND s.frameio_asset_ids IS NOT NULL
                 AND jsonb_array_length(s.frameio_asset_ids) > 0)
          ORDER BY COALESCE(s.completed_at, s.last_activity_at, s.started_at, s.created_at) DESC
          LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await;

    let downloads = sqlx::query(
        "SELECT e.id, e.file_name, e.bytes_served, e.completed, e.started_at, ds.viewer_email
           FROM download_events e JOIN download_sessions ds ON e.download_session_id = ds.id
          ORDER BY e.started_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await;

    let (active, uploads, downloads) = match (active, uploads, downloads) {
        (Ok(a), Ok(u), Ok(d)) => (a, u, d),
        (a, u, d) => {
            tracing::error!("dashboard.transfers query failed: {:?} {:?} {:?}", a.err(), u.err(), d.err());
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    let mut active_uploads: Vec<Value> = Vec::with_capacity(active.len());
    for row in &active {
        let ids: Option<Value> = row.get("frameio_asset_ids");
        let total: Option<i64> = row.get("total_bytes");
        active_uploads.push(json!({
            "id": row.get::<uuid::Uuid, _>("id").to_string(),
            "who": who(row.get("uploader_name"), row.get("uploader_email")),
            "brand": row.get::<Option<String>, _>("brand_display_name"),
            "uploaded_bytes": row.get::<Option<i64>, _>("uploaded_bytes"),
            "total_bytes": total.or_else(|| planned_bytes(&ids)),
            "started_at": row.get::<Option<DateTime<Utc>>, _>("started_at").map(|d| crate::pyfmt::iso(&d)),
            "link_id": row.get::<uuid::Uuid, _>("link_id").to_string(),
            "last_activity_at": row.get::<Option<DateTime<Utc>>, _>("last_activity_at").map(|d| crate::pyfmt::iso(&d)),
        }));
    }

    for row in &uploads {
        let ids: Option<Value> = row.get("frameio_asset_ids");
        let status: String = row.get("status");
        let completed = status == "completed";
        let total: Option<i64> = row.get("total_bytes");
        let uploaded: Option<i64> = row.get("uploaded_bytes");
        let at = row
            .get::<Option<DateTime<Utc>>, _>("completed_at")
            .or_else(|| row.get::<Option<DateTime<Utc>>, _>("last_activity_at"))
            .or_else(|| row.get::<Option<DateTime<Utc>>, _>("started_at"))
            .unwrap_or_else(|| at_of(row, "created_at"));
        let size = if completed {
            total
        } else {
            uploaded.or_else(|| planned_bytes(&ids))
        };
        let brand: Option<String> = row.get("brand_display_name");
        feed.push((
            at,
            json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "upload",
                "label": brand.unwrap_or_else(|| "Upload".to_string()),
                "who": who(row.get("uploader_name"), row.get("uploader_email")),
                "bytes": size,
                "status": if completed { "completed" } else { "incomplete" },
                "at": crate::pyfmt::iso(&at),
            }),
        ));
    }

    for row in &downloads {
        let at: DateTime<Utc> = row.get("started_at");
        let completed: bool = row.get("completed");
        feed.push((
            at,
            json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "download",
                "label": row.get::<Option<String>, _>("file_name").unwrap_or_else(|| "Download".to_string()),
                "who": row.get::<Option<String>, _>("viewer_email"),
                "bytes": row.get::<Option<i64>, _>("bytes_served"),
                "status": if completed { "completed" } else { "in_progress" },
                "at": crate::pyfmt::iso(&at),
            }),
        ));
    }

    feed.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let recent: Vec<Value> = feed.into_iter().take(limit as usize).map(|(_, v)| v).collect();

    Json(json!({ "active_uploads": active_uploads, "recent": recent })).into_response()
}
