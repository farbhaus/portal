
//! Admin activity log — `portal.routes.activity`.

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
pub struct ActivityQuery {
    action: Option<String>,
    limit: Option<i64>,
}

pub async fn list_activity(
    State(state): State<AppState>,
    _: Admin,
    Query(q): Query<ActivityQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query(
        "SELECT a.id, a.action, a.detail, a.created_at, u.email
           FROM audit_log a LEFT JOIN users u ON a.user_id = u.id
          WHERE ($1::text IS NULL OR a.action = $1)
          ORDER BY a.created_at DESC LIMIT $2",
    )
    .bind(q.action.as_deref())
    .bind(limit)
    .fetch_all(&state.db)
    .await;
    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("activity.list_failed: {e}");
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<uuid::Uuid, _>("id").to_string(),
                "action": r.get::<String, _>("action"),
                "user_email": r.get::<Option<String>, _>("email"),
                "detail": r.get::<Option<Value>, _>("detail"),
                "created_at": crate::pyfmt::iso(&r.get::<DateTime<Utc>, _>("created_at")),
            })
        })
        .collect();
    Json(Value::Array(out)).into_response()
}

pub async fn list_actions(State(state): State<AppState>, _: Admin) -> Response {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT action FROM audit_log ORDER BY action",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    Json(json!(rows)).into_response()
}

pub async fn summary(State(state): State<AppState>, _: Admin) -> Response {
    let mut counts = [0i64; 4];
    let queries = [
        "SELECT count(*) FROM destinations",
        "SELECT count(*) FROM upload_links",
        "SELECT count(*) FROM download_links",
        "SELECT count(*) FROM sync_rules",
    ];
    for (i, q) in queries.iter().enumerate() {
        counts[i] = sqlx::query_scalar(*q).fetch_one(&state.db).await.unwrap_or(0);
    }
    Json(json!({
        "destinations": counts[0],
        "upload_links": counts[1],
        "download_links": counts[2],
        "sync_rules": counts[3],
    }))
    .into_response()
}
