
use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

use crate::state::AppState;

/// The `/api/health` payload. Field order is part of the contract: the Python pydantic/JSON
/// encoder emits keys in this order and the byte-for-byte diff compares raw bodies.
#[derive(Serialize)]
struct Health {
    status: &'static str,
    db: bool,
    redis: bool,
    version: &'static str,
}

async fn check_db(state: &AppState) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok()
}

async fn check_redis(state: &AppState) -> bool {
    let mut conn = state.redis.clone();
    redis::cmd("PING")
        .query_async::<String>(&mut conn)
        .await
        .map(|pong| !pong.is_empty())
        .unwrap_or(false)
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let (db_ok, redis_ok) = tokio::join!(check_db(&state), check_redis(&state));
    let healthy = db_ok && redis_ok;
    let body = Health {
        status: if healthy { "ok" } else { "degraded" },
        db: db_ok,
        redis: redis_ok,
        version: env!("CARGO_PKG_VERSION"),
    };
    let status = if healthy { 200 } else { 503 };
    (axum::http::StatusCode::from_u16(status).unwrap(), Json(body))
}
