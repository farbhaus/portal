
//! `GET /api/settings/email` and `GET /api/settings/frameio-config` — the read halves of the
//! Settings page (`portal.routes.settings` + `portal.services.runtime_config`).

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::Admin;
use crate::state::AppState;

pub async fn email_status(State(state): State<AppState>, _: Admin) -> Response {
    let row = sqlx::query(
        "SELECT smtp_host, smtp_port, smtp_username, smtp_password_encrypted, smtp_from,
                smtp_use_tls, smtp_starttls, notify_email
           FROM app_settings LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await;
    let row = match row {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("settings.email_status failed: {e}");
            return crate::error::detail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    // Defaults mirror EmailConfig: port 587, starttls on, tls off when unset.
    let (host, port, user, from, use_tls, starttls, notify, has_password) = match &row {
        Some(r) => (
            r.get::<Option<String>, _>("smtp_host").unwrap_or_default(),
            r.get::<Option<i32>, _>("smtp_port").unwrap_or(587),
            r.get::<Option<String>, _>("smtp_username").unwrap_or_default(),
            r.get::<Option<String>, _>("smtp_from").unwrap_or_default(),
            r.get::<Option<bool>, _>("smtp_use_tls").unwrap_or(false),
            r.get::<Option<bool>, _>("smtp_starttls").unwrap_or(true),
            r.get::<Option<String>, _>("notify_email").unwrap_or_default(),
            r.get::<Option<String>, _>("smtp_password_encrypted").is_some(),
        ),
        None => (String::new(), 587, String::new(), String::new(), false, true, String::new(), false),
    };
    let configured = !host.is_empty() && !from.is_empty();
    let notify = if notify.is_empty() { state.settings.admin_email.clone() } else { notify };
    Json(json!({
        "configured": configured,
        "smtp_host": if host.is_empty() { serde_json::Value::Null } else { json!(host) },
        "smtp_port": port,
        "smtp_username": if user.is_empty() { serde_json::Value::Null } else { json!(user) },
        "smtp_from": if from.is_empty() { serde_json::Value::Null } else { json!(from) },
        "smtp_use_tls": use_tls,
        "smtp_starttls": starttls,
        "notify_email": notify,
        "has_password": has_password,
    }))
    .into_response()
}

pub async fn frameio_config_status(State(state): State<AppState>, _: Admin) -> Response {
    let row = sqlx::query(
        "SELECT frameio_client_id, frameio_client_secret_encrypted FROM app_settings LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await;
    let (client_id, has_secret) = match &row {
        Ok(Some(r)) => (
            r.get::<Option<String>, _>("frameio_client_id").unwrap_or_default(),
            r.get::<Option<String>, _>("frameio_client_secret_encrypted").is_some(),
        ),
        _ => (String::new(), false),
    };
    let configured = !client_id.is_empty() && has_secret;
    Json(json!({
        "configured": configured,
        "client_id": if client_id.is_empty() { serde_json::Value::Null } else { json!(client_id) },
        "has_secret": has_secret,
        "redirect_uri": state.settings.resolved_frameio_redirect_uri(),
    }))
    .into_response()
}

async fn ensure_app_settings(state: &AppState) -> Option<uuid::Uuid> {
    let id: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM app_settings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten();
    if id.is_some() {
        return id;
    }
    sqlx::query_scalar("INSERT INTO app_settings (config_seeded) VALUES (false) RETURNING id")
        .fetch_optional(&state.db).await.ok().flatten()
}

pub async fn update_email(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let Some(id) = ensure_app_settings(&state).await else {
        return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let cleartext = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let clear_password = body.get("clear_password").and_then(|v| v.as_bool()).unwrap_or(false);
    let new_password = body.get("smtp_password").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let encrypted = new_password.map(|p| crate::services::runtime_config::encrypt_secret(&state.settings, p));
    let _ = sqlx::query(
        "UPDATE app_settings SET smtp_host=$1, smtp_port=$2, smtp_username=$3, smtp_from=$4,
            smtp_use_tls=$5, smtp_starttls=$6, notify_email=$7,
            smtp_password_encrypted = CASE WHEN $8 THEN NULL ELSE COALESCE($9, smtp_password_encrypted) END,
            updated_at=now() WHERE id=$10",
    )
    .bind(opt_str(cleartext("smtp_host")))
    .bind(body.get("smtp_port").and_then(|v| v.as_i64()).unwrap_or(587) as i32)
    .bind(opt_str(cleartext("smtp_username")))
    .bind(opt_str(cleartext("smtp_from")))
    .bind(body.get("smtp_use_tls").and_then(|v| v.as_bool()).unwrap_or(false))
    .bind(body.get("smtp_starttls").and_then(|v| v.as_bool()).unwrap_or(true))
    .bind(opt_str(cleartext("notify_email")))
    .bind(clear_password)
    .bind(encrypted)
    .bind(id)
    .execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'settings.email_updated')")
        .bind(admin.id).execute(&state.db).await;
    email_status(State(state), admin).await
}

fn opt_str(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

pub async fn send_email_test(State(state): State<AppState>, _: Admin) -> Response {
    let cfg = crate::services::runtime_config::get_email_config(&state.db, &state.settings).await;
    if !cfg.configured() {
        return crate::error::detail(StatusCode::BAD_REQUEST, "Email is not configured");
    }
    let brand = crate::services::runtime_config::email_brand(&state.db, &state.settings).await;
    match crate::notify::send_test_email(&cfg, &brand).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => crate::error::detail(StatusCode::BAD_GATEWAY, &format!("Send failed: {e}")),
    }
}

pub async fn update_frameio_config(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let Some(id) = ensure_app_settings(&state).await else {
        return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let clear_secret = body.get("clear_secret").and_then(|v| v.as_bool()).unwrap_or(false);
    let secret = body.get("client_secret").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let encrypted = secret.map(|p| crate::services::runtime_config::encrypt_secret(&state.settings, p));
    let _ = sqlx::query(
        "UPDATE app_settings SET frameio_client_id=$1,
            frameio_client_secret_encrypted = CASE WHEN $2 THEN NULL ELSE COALESCE($3, frameio_client_secret_encrypted) END,
            updated_at=now() WHERE id=$4",
    )
    .bind(opt_str(client_id))
    .bind(clear_secret)
    .bind(encrypted)
    .bind(id)
    .execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'settings.frameio_config_updated')")
        .bind(admin.id).execute(&state.db).await;
    super::settings::frameio_config_status(State(state), admin).await
}

pub async fn change_password(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let current = body.get("current_password").and_then(|v| v.as_str()).unwrap_or("");
    let new_password = body.get("new_password").and_then(|v| v.as_str()).unwrap_or("");
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(admin.id).fetch_optional(&state.db).await.ok().flatten();
    let Some(hash) = hash else {
        return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    if !crate::auth::verify_password(current, &hash) {
        return crate::error::detail(StatusCode::FORBIDDEN, "Current password is incorrect");
    }
    if new_password.chars().count() < 8 {
        return crate::error::detail(StatusCode::UNPROCESSABLE_ENTITY, "String should have at least 8 characters");
    }
    let Ok(new_hash) = bcrypt::hash(new_password, bcrypt::DEFAULT_COST) else {
        return crate::error::detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let _ = sqlx::query("UPDATE users SET password_hash=$1, updated_at=now() WHERE id=$2")
        .bind(new_hash).bind(admin.id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'admin.password_changed')")
        .bind(admin.id).execute(&state.db).await;
    Json(json!({ "ok": true })).into_response()
}
