
//! Frame.io webhook ingestion + the admin event log — `portal.routes.webhooks`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::frameio::webhooks::{
    parse_webhook_payload, verify_signature, SIGNATURE_HEADER, TIMESTAMP_HEADER,
};
use crate::state::AppState;
use crate::sync::queue::{self, PROCESS_WEBHOOK_EVENT};

pub async fn ingest_frameio(
    State(state): State<AppState>,
    Path(rule_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let secret: Option<String> =
        sqlx::query_scalar("SELECT webhook_secret FROM sync_rules WHERE id=$1")
            .bind(rule_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some(secret) = secret else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let timestamp = headers
        .get(TIMESTAMP_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid = verify_signature(
        &secret,
        timestamp,
        &body,
        signature,
        None,
        state.settings.webhook_signature_tolerance_seconds,
    );

    let raw: Value = serde_json::from_slice(&body).unwrap_or_else(|_| {
        if body.is_empty() {
            json!({ "_unparseable": true })
        } else {
            json!({ "_raw": String::from_utf8_lossy(&body) })
        }
    });
    let event_type = parse_webhook_payload(&raw).r#type;

    // `processed` has no DB default (SQLAlchemy sets it), so bind it explicitly.
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO webhook_events (source, event_type, raw, signature_valid, processed) VALUES ('frameio',$1,$2,$3,false) RETURNING id",
    )
    .bind(event_type)
    .bind(&raw)
    .bind(valid)
    .fetch_one(&state.db)
    .await;
    let event_id = match inserted {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::error!("webhook.persist_failed: {e}");
            None
        }
    };

    if !valid {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(id) = event_id {
        let mut redis = state.redis.clone();
        let _ = queue::enqueue(&mut redis, PROCESS_WEBHOOK_EVENT, &id.to_string(), 0.0).await;
    }
    StatusCode::OK.into_response()
}

pub async fn list_events(
    State(state): State<AppState>,
    _: Admin,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let limit = q.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query("SELECT * FROM webhook_events ORDER BY received_at DESC LIMIT $1")
        .bind(limit)
        .fetch_all(&state.db)
        .await;
    let Ok(rows) = rows else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let out: Vec<Value> = rows
        .iter()
        .map(|e| {
            json!({
                "id": e.get::<Uuid, _>("id").to_string(),
                "source": e.get::<String, _>("source"),
                "event_type": e.get::<Option<String>, _>("event_type"),
                "signature_valid": e.get::<Option<bool>, _>("signature_valid"),
                "processed": e.get::<bool, _>("processed"),
                "processed_at": e.get::<Option<DateTime<Utc>>, _>("processed_at").map(|d| crate::pyfmt::iso(&d)),
                "error": e.get::<Option<String>, _>("error"),
                "received_at": crate::pyfmt::iso(&e.get::<DateTime<Utc>, _>("received_at")),
            })
        })
        .collect();
    Json(Value::Array(out)).into_response()
}
