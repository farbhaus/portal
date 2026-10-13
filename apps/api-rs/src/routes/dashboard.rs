
//! `GET /api/dashboard/transfers` — the operations view, mirroring `portal.routes.dashboard`.
//!
//! The recent-transfer feed merges completed uploads, file downloads and sync jobs
//! newest-first. Python's `list.sort` is stable; Rust's `sort_by` is too, and the feeds are
//! appended in the same order (uploads, downloads, syncs), so ties resolve identically.

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
    limit: Option<i64>,
    offset: Option<i64>,
}

/// `GET /api/dashboard/history` — paginated merge of completed uploads and file downloads.
pub async fn history(State(state): State<AppState>, _: Admin, Query(q): Query<HistoryQuery>) -> Response {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let kind = q.kind.unwrap_or_else(|| "all".to_string());
    let want_upload = kind == "all" || kind == "upload";
    let want_download = kind == "all" || kind == "download";
    let take = offset + limit + 1;
    let mut pool: Vec<(DateTime<Utc>, Value)> = Vec::new();

    if want_upload {
        let rows = sqlx::query(
            "SELECT s.id, s.uploader_name, s.uploader_email, s.uploaded_bytes, s.total_bytes,
                    s.status, s.started_at, s.completed_at, s.last_activity_at, s.created_at,
                    s.frameio_asset_ids, l.brand_display_name
               FROM upload_sessions s JOIN upload_links l ON s.upload_link_id = l.id
              WHERE s.status = 'completed'
                 OR (s.status = 'abandoned' AND s.frameio_asset_ids IS NOT NULL
                     AND jsonb_array_length(s.frameio_asset_ids) > 0)
              ORDER BY COALESCE(s.completed_at, s.last_activity_at, s.started_at, s.created_at) DESC
              LIMIT $1",
        )
        .bind(take)
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
            pool.push((at, json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "upload",
                "label": brand.unwrap_or_else(|| "Upload".to_string()),
                "who": who(row.get("uploader_name"), row.get("uploader_email")),
                "bytes": size,
                "status": if completed { "completed" } else { "incomplete" },
                "at": crate::pyfmt::iso(&at),
            })));
        }
    }
    if want_download {
        let rows = sqlx::query(
            "SELECT e.id, e.file_name, e.bytes_served, e.completed, e.started_at, ds.viewer_email
               FROM download_events e JOIN download_sessions ds ON e.download_session_id = ds.id
              ORDER BY e.started_at DESC LIMIT $1",
        )
        .bind(take)
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
                "who": row.get::<Option<String>, _>("viewer_email"),
                "bytes": row.get::<Option<i64>, _>("bytes_served"),
                "status": if completed { "completed" } else { "in_progress" },
                "at": crate::pyfmt::iso(&at),
            })));
        }
    }

    pool.sort_by(|a, b| b.0.cmp(&a.0));
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
                s.started_at, s.frameio_asset_ids, l.brand_display_name
           FROM upload_sessions s JOIN upload_links l ON s.upload_link_id = l.id
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
                s.frameio_asset_ids, l.brand_display_name
           FROM upload_sessions s JOIN upload_links l ON s.upload_link_id = l.id
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

    let syncs = sqlx::query(
        "SELECT j.id, j.status, j.bytes, j.started_at, j.completed_at, j.created_at, r.name
           FROM sync_jobs j JOIN sync_rules r ON j.sync_rule_id = r.id
          ORDER BY j.created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await;

    let (active, uploads, downloads, syncs) = match (active, uploads, downloads, syncs) {
        (Ok(a), Ok(u), Ok(d), Ok(s)) => (a, u, d, s),
        (a, u, d, s) => {
            tracing::error!("dashboard.transfers query failed: {:?} {:?} {:?} {:?}", a.err(), u.err(), d.err(), s.err());
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

    for row in &syncs {
        let at = row
            .get::<Option<DateTime<Utc>>, _>("completed_at")
            .or_else(|| row.get::<Option<DateTime<Utc>>, _>("started_at"))
            .unwrap_or_else(|| at_of(row, "created_at"));
        feed.push((
            at,
            json!({
                "id": row.get::<uuid::Uuid, _>("id").to_string(),
                "kind": "sync",
                "label": row.get::<String, _>("name"),
                "who": null,
                "bytes": row.get::<Option<i64>, _>("bytes"),
                "status": row.get::<String, _>("status"),
                "at": crate::pyfmt::iso(&at),
            }),
        ));
    }

    feed.sort_by(|a, b| b.0.cmp(&a.0));
    let recent: Vec<Value> = feed.into_iter().take(limit as usize).map(|(_, v)| v).collect();

    // --- sync health -----------------------------------------------------------
    let status_rows = sqlx::query("SELECT status, count(*) AS n FROM sync_jobs GROUP BY status")
        .fetch_all(&state.db)
        .await;
    let status_rows = match status_rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("dashboard.sync_health failed: {e}");
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let mut counts = std::collections::HashMap::new();
    for row in status_rows {
        counts.insert(row.get::<String, _>("status"), row.get::<i64, _>("n"));
    }
    let done_24h: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sync_jobs WHERE status = 'done' AND completed_at >= now() - interval '24 hours'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let dead_letter_rule_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT sync_rule_id FROM sync_jobs WHERE status = 'dead_letter' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let body = json!({
        "active_uploads": active_uploads,
        "recent": recent,
        "sync_health": {
            "waiting": counts.get("waiting").copied().unwrap_or(0),
            "running": counts.get("running").copied().unwrap_or(0),
            "dead_letter": counts.get("dead_letter").copied().unwrap_or(0),
            "done_24h": done_24h,
            "dead_letter_rule_id": dead_letter_rule_id.map(|u| u.to_string()),
        },
    });
    Json(body).into_response()
}
