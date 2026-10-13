
//! Admin data-subject tools (GDPR access + erasure) — `portal.routes.privacy`.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::state::AppState;

fn iso(d: Option<DateTime<Utc>>) -> Value {
    d.map(|d| json!(crate::pyfmt::iso(&d))).unwrap_or(Value::Null)
}

pub async fn export_subject(
    State(state): State<AppState>,
    admin: Admin,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let email = q.get("email").cloned().unwrap_or_default().trim().to_lowercase();
    let uploads = sqlx::query(
        "SELECT * FROM upload_sessions WHERE lower(uploader_email) = $1 ORDER BY created_at DESC",
    )
    .bind(&email).fetch_all(&state.db).await.unwrap_or_default();
    let downloads = sqlx::query(
        "SELECT * FROM download_sessions WHERE lower(viewer_email) = $1 ORDER BY started_at DESC NULLS LAST",
    )
    .bind(&email).fetch_all(&state.db).await.unwrap_or_default();

    let mut events_by_session: HashMap<Uuid, Vec<Value>> = HashMap::new();
    let ids: Vec<Uuid> = downloads.iter().map(|d| d.get("id")).collect();
    if !ids.is_empty() {
        let events = sqlx::query(
            "SELECT * FROM download_events WHERE download_session_id = ANY($1) ORDER BY started_at DESC",
        )
        .bind(&ids).fetch_all(&state.db).await.unwrap_or_default();
        for e in events {
            events_by_session.entry(e.get("download_session_id")).or_default().push(json!({
                "file_name": e.get::<Option<String>, _>("file_name"),
                "frameio_file_id": e.get::<String, _>("frameio_file_id"),
                "completed": e.get::<bool, _>("completed"),
                "bytes_served": e.get::<Option<i64>, _>("bytes_served"),
                "ip": e.get::<Option<String>, _>("ip"),
                "user_agent": e.get::<Option<String>, _>("user_agent"),
                "started_at": iso(Some(e.get::<DateTime<Utc>, _>("started_at"))),
            }));
        }
    }
    let verifications = sqlx::query("SELECT * FROM email_verifications WHERE lower(email) = $1")
        .bind(&email).fetch_all(&state.db).await.unwrap_or_default();

    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'privacy.subject_exported',$2)")
        .bind(admin.id)
        .bind(json!({ "email": email, "upload_sessions": uploads.len(), "download_sessions": downloads.len() }))
        .execute(&state.db).await;

    let upload_out: Vec<Value> = uploads.iter().map(|s| json!({
        "id": s.get::<Uuid, _>("id").to_string(),
        "upload_link_id": s.get::<Uuid, _>("upload_link_id").to_string(),
        "uploader_name": s.get::<Option<String>, _>("uploader_name"),
        "uploader_email": s.get::<Option<String>, _>("uploader_email"),
        "uploader_message": s.get::<Option<String>, _>("uploader_message"),
        "status": s.get::<String, _>("status"),
        "total_bytes": s.get::<Option<i64>, _>("total_bytes"),
        "started_at": iso(s.get("started_at")),
        "completed_at": iso(s.get("completed_at")),
        "created_at": iso(Some(s.get::<DateTime<Utc>, _>("created_at"))),
    })).collect();
    let download_out: Vec<Value> = downloads.iter().map(|s| json!({
        "id": s.get::<Uuid, _>("id").to_string(),
        "download_link_id": s.get::<Uuid, _>("download_link_id").to_string(),
        "viewer_name": s.get::<Option<String>, _>("viewer_name"),
        "viewer_email": s.get::<Option<String>, _>("viewer_email"),
        "ip": s.get::<Option<String>, _>("ip"),
        "user_agent": s.get::<Option<String>, _>("user_agent"),
        "started_at": iso(s.get("started_at")),
        "created_at": iso(Some(s.get::<DateTime<Utc>, _>("created_at"))),
        "events": events_by_session.get(&s.get::<Uuid, _>("id")).cloned().unwrap_or_default(),
    })).collect();
    let verify_out: Vec<Value> = verifications.iter().map(|v| json!({
        "email": v.get::<String, _>("email"),
        "expires_at": iso(Some(v.get::<DateTime<Utc>, _>("expires_at"))),
        "attempts": v.get::<i32, _>("attempts"),
        "last_sent_at": iso(Some(v.get::<DateTime<Utc>, _>("last_sent_at"))),
    })).collect();

    Json(json!({
        "email": email,
        "generated_at": crate::pyfmt::iso(&Utc::now()),
        "upload_sessions": upload_out,
        "download_sessions": download_out,
        "email_verifications": verify_out,
    }))
    .into_response()
}

pub async fn erase_subject(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("").trim().to_lowercase();
    let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("anonymize").to_string();

    let up_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM upload_sessions WHERE lower(uploader_email) = $1")
        .bind(&email).fetch_all(&state.db).await.unwrap_or_default();
    let dl_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM download_sessions WHERE lower(viewer_email) = $1")
        .bind(&email).fetch_all(&state.db).await.unwrap_or_default();
    let ver_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM email_verifications WHERE lower(email) = $1")
        .bind(&email).fetch_all(&state.db).await.unwrap_or_default();

    if mode == "anonymize" {
        if !up_ids.is_empty() {
            let _ = sqlx::query("UPDATE upload_sessions SET uploader_name=NULL, uploader_email=NULL, uploader_message=NULL, updated_at=now() WHERE id = ANY($1)")
                .bind(&up_ids).execute(&state.db).await;
        }
        if !dl_ids.is_empty() {
            let _ = sqlx::query("UPDATE download_sessions SET viewer_name=NULL, viewer_email=NULL, ip=NULL, user_agent=NULL, updated_at=now() WHERE id = ANY($1)")
                .bind(&dl_ids).execute(&state.db).await;
            let _ = sqlx::query("UPDATE download_events SET ip=NULL, user_agent=NULL WHERE download_session_id = ANY($1)")
                .bind(&dl_ids).execute(&state.db).await;
        }
    } else {
        if !up_ids.is_empty() {
            let _ = sqlx::query("DELETE FROM upload_sessions WHERE id = ANY($1)").bind(&up_ids).execute(&state.db).await;
        }
        if !dl_ids.is_empty() {
            let _ = sqlx::query("DELETE FROM download_sessions WHERE id = ANY($1)").bind(&dl_ids).execute(&state.db).await;
        }
    }
    if !ver_ids.is_empty() {
        let _ = sqlx::query("DELETE FROM email_verifications WHERE id = ANY($1)").bind(&ver_ids).execute(&state.db).await;
    }
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'privacy.subject_erased',$2)")
        .bind(admin.id)
        .bind(json!({ "mode": mode, "upload_sessions": up_ids.len(), "download_sessions": dl_ids.len() }))
        .execute(&state.db).await;

    Json(json!({
        "mode": mode,
        "upload_sessions": up_ids.len(),
        "download_sessions": dl_ids.len(),
        "email_verifications": ver_ids.len(),
    }))
    .into_response()
}
