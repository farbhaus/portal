
//! Admin actions on individual sync jobs — `portal.routes.sync_jobs`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::state::AppState;
use crate::sync::queue::{self, RUN_SYNC_JOB};

pub async fn retry_job(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM sync_jobs WHERE id=$1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if exists.is_none() {
        return detail(StatusCode::NOT_FOUND, "sync job not found");
    }
    let _ = sqlx::query(
        "UPDATE sync_jobs SET status='pending', retry_count=0, error=NULL, started_at=NULL, completed_at=NULL, source_wait_started_at=NULL, updated_at=now() WHERE id=$1",
    ).bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'sync_job.retried',$2)")
        .bind(admin.id).bind(json!({ "sync_job_id": id.to_string() })).execute(&state.db).await;
    let mut redis = state.redis.clone();
    let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &id.to_string(), 0.0).await;
    Json(json!({ "ok": true, "status": "pending" })).into_response()
}

pub async fn dismiss_job(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM sync_jobs WHERE id=$1").bind(id).fetch_optional(&state.db).await.ok().flatten();
    if exists.is_none() {
        return detail(StatusCode::NOT_FOUND, "sync job not found");
    }
    let _ = sqlx::query("UPDATE sync_jobs SET status='skipped', completed_at=now(), updated_at=now() WHERE id=$1")
        .bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'sync_job.dismissed',$2)")
        .bind(admin.id).bind(json!({ "sync_job_id": id.to_string() })).execute(&state.db).await;
    Json(json!({ "ok": true, "status": "skipped" })).into_response()
}
