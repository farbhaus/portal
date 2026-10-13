
//! Admin security + branding — `portal.routes.security` (TOTP setup, branding status/logo/brand).

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::totp as totp_svc;
use crate::auth::Admin;
use crate::crypto::TokenCipher;
use crate::error::detail;
use crate::state::AppState;

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn cipher(state: &AppState) -> TokenCipher {
    TokenCipher::new(&state.settings.token_encryption_key)
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

fn branding_json(row: Option<&sqlx::postgres::PgRow>) -> Value {
    match row {
        Some(r) => {
            let has_logo = r.get::<Option<Vec<u8>>, _>("logo_png").is_some();
            json!({
                "has_logo": has_logo,
                "logo_updated_at": r.get::<Option<DateTime<Utc>>, _>("logo_updated_at").map(|d| crate::pyfmt::iso(&d)),
                "brand_display_name": r.get::<Option<String>, _>("brand_display_name"),
                "brand_accent_color": r.get::<Option<String>, _>("brand_accent_color"),
            })
        }
        None => json!({ "has_logo": false, "logo_updated_at": null, "brand_display_name": null, "brand_accent_color": null }),
    }
}

async fn branding_row(state: &AppState) -> Option<sqlx::postgres::PgRow> {
    sqlx::query("SELECT id, logo_png, logo_updated_at, brand_display_name, brand_accent_color FROM app_settings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten()
}

pub async fn branding_status(State(state): State<AppState>, _: Admin) -> Response {
    let row = branding_row(&state).await;
    Json(branding_json(row.as_ref())).into_response()
}

pub async fn upload_logo(State(state): State<AppState>, admin: Admin, mut multipart: Multipart) -> Response {
    let mut data: Option<Vec<u8>> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            data = field.bytes().await.ok().map(|b| b.to_vec());
            break;
        }
    }
    let Some(data) = data else {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "Field required");
    };
    let max_bytes = 2 * 1024 * 1024usize; // BRANDING_LOGO_MAX_BYTES default
    if data.len() > max_bytes {
        return detail(StatusCode::BAD_REQUEST, &format!("Logo exceeds the maximum size ({} KB)", max_bytes / 1024));
    }
    if !data.starts_with(&PNG_MAGIC) {
        return detail(StatusCode::BAD_REQUEST, "Logo must be a PNG file");
    }
    let Some(id) = ensure_app_settings(&state).await else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let _ = sqlx::query("UPDATE app_settings SET logo_png=$1, logo_content_type='image/png', logo_updated_at=now(), updated_at=now() WHERE id=$2")
        .bind(&data).bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'branding.logo_updated')")
        .bind(admin.id).execute(&state.db).await;
    let row = branding_row(&state).await;
    Json(branding_json(row.as_ref())).into_response()
}

pub async fn delete_logo(State(state): State<AppState>, admin: Admin) -> Response {
    if let Some(id) = sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM app_settings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten()
    {
        let _ = sqlx::query("UPDATE app_settings SET logo_png=NULL, logo_content_type=NULL, logo_updated_at=NULL, updated_at=now() WHERE id=$1")
            .bind(id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'branding.logo_removed')")
            .bind(admin.id).execute(&state.db).await;
    }
    let row = branding_row(&state).await;
    Json(branding_json(row.as_ref())).into_response()
}

fn valid_hex_color(v: &str) -> bool {
    let v = v.trim();
    let bytes = v.as_bytes();
    if !v.starts_with('#') {
        return false;
    }
    let hex = &bytes[1..];
    (hex.len() == 3 || hex.len() == 6) && hex.iter().all(|c| c.is_ascii_hexdigit())
}

pub async fn update_branding(State(state): State<AppState>, _: Admin, Json(body): Json<Value>) -> Response {
    let name = body.get("brand_display_name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let accent_raw = body.get("brand_accent_color").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if !accent_raw.is_empty() && !valid_hex_color(&accent_raw) {
        return detail(StatusCode::UNPROCESSABLE_ENTITY, "must be a hex colour like #f59e0b");
    }
    let Some(id) = ensure_app_settings(&state).await else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    let _ = sqlx::query("UPDATE app_settings SET brand_display_name=$1, brand_accent_color=$2, updated_at=now() WHERE id=$3")
        .bind(if name.is_empty() { None } else { Some(name) })
        .bind(if accent_raw.is_empty() { None } else { Some(accent_raw) })
        .bind(id).execute(&state.db).await;
    let row = branding_row(&state).await;
    Json(branding_json(row.as_ref())).into_response()
}

// ── TOTP setup ───────────────────────────────────────────────────────────────

pub async fn totp_begin(State(state): State<AppState>, admin: Admin) -> Response {
    let enabled: Option<bool> = sqlx::query_scalar("SELECT totp_enabled FROM users WHERE id=$1")
        .bind(admin.id).fetch_optional(&state.db).await.ok().flatten();
    if enabled == Some(true) {
        return detail(StatusCode::BAD_REQUEST, "Two-factor is already enabled");
    }
    let secret = totp_svc::generate_secret();
    let encrypted = cipher(&state).encrypt(&secret);
    let _ = sqlx::query("UPDATE users SET totp_secret_encrypted=$1, updated_at=now() WHERE id=$2")
        .bind(encrypted).bind(admin.id).execute(&state.db).await;
    let uri = totp_svc::provisioning_uri(&secret, &admin.email);
    let qr = totp_svc::qr_svg(&uri);
    Json(json!({ "secret": secret, "otpauth_uri": uri, "qr_svg": qr })).into_response()
}

pub async fn totp_enable(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let row: Option<(bool, Option<String>)> =
        sqlx::query_as("SELECT totp_enabled, totp_secret_encrypted FROM users WHERE id=$1")
            .bind(admin.id).fetch_optional(&state.db).await.ok().flatten();
    let Some((enabled, secret_enc)) = row else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    if enabled {
        return detail(StatusCode::BAD_REQUEST, "Two-factor is already enabled");
    }
    let Some(secret_enc) = secret_enc else {
        return detail(StatusCode::BAD_REQUEST, "Start two-factor setup first");
    };
    let Ok(secret) = cipher(&state).decrypt(&secret_enc) else {
        return detail(StatusCode::BAD_REQUEST, "Start two-factor setup first");
    };
    let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if !totp_svc::verify_code(&secret, code) {
        return detail(StatusCode::BAD_REQUEST, "That code didn't match — try again");
    }
    let (codes, hashes) = totp_svc::generate_recovery_codes();
    let _ = sqlx::query("UPDATE users SET totp_enabled=true, totp_recovery_codes=$1, updated_at=now() WHERE id=$2")
        .bind(json!(hashes)).bind(admin.id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'totp.enabled')")
        .bind(admin.id).execute(&state.db).await;
    Json(json!({ "recovery_codes": codes })).into_response()
}

pub async fn totp_disable(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let row: Option<(bool, Option<String>, Option<Value>, String)> =
        sqlx::query_as("SELECT totp_enabled, totp_secret_encrypted, totp_recovery_codes, password_hash FROM users WHERE id=$1")
            .bind(admin.id).fetch_optional(&state.db).await.ok().flatten();
    let Some((enabled, secret_enc, _recovery, password_hash)) = row else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    if !enabled {
        return Json(json!({ "ok": true })).into_response();
    }
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let mut ok = !password.is_empty() && crate::auth::verify_password(password, &password_hash);
    if !ok {
        if let (Some(code), Some(enc)) = (body.get("code").and_then(|v| v.as_str()), secret_enc.as_ref()) {
            if let Ok(secret) = cipher(&state).decrypt(enc) {
                ok = totp_svc::verify_code(&secret, code);
            }
        }
    }
    if !ok {
        return detail(StatusCode::FORBIDDEN, "Provide your current password or a valid code");
    }
    let _ = sqlx::query("UPDATE users SET totp_enabled=false, totp_secret_encrypted=NULL, totp_recovery_codes=NULL, updated_at=now() WHERE id=$1")
        .bind(admin.id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'totp.disabled')")
        .bind(admin.id).execute(&state.db).await;
    Json(json!({ "ok": true })).into_response()
}
