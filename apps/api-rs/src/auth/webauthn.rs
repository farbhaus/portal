
//! Passkeys (WebAuthn) — the port of `portal.auth.webauthn` + the passkey routes in
//! `portal.routes.auth`.
//!
//! Python used py_webauthn and stored the raw COSE public key on the row. This uses
//! `webauthn-rs`, which stores a serialized `Passkey`; the column is Text, so the JSON goes there
//! and `credential_id` stays the base64url credential id. Passkeys registered by the Python
//! service must be re-registered once (the stored key format differs).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use serde_json::{json, Value};
use sqlx::Row;
use url::Url;
use webauthn_rs::prelude::*;

use crate::auth::{Admin, SessionExt};
use crate::error::detail;
use crate::state::AppState;

const REG_CHALLENGE: &str = "webauthn_reg_challenge";
const AUTH_CHALLENGE: &str = "webauthn_auth_challenge";

fn rp_id(state: &AppState) -> String {
    if !state.settings.webauthn_rp_id.is_empty() {
        return state.settings.webauthn_rp_id.clone();
    }
    Url::parse(&state.settings.base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}

fn webauthn(state: &AppState) -> Option<Webauthn> {
    let origin = Url::parse(state.settings.base_url.trim_end_matches('/')).ok()?;
    WebauthnBuilder::new(&rp_id(state), &origin)
        .ok()?
        .rp_name(&state.settings.webauthn_rp_name)
        .build()
        .ok()
}

pub async fn register_begin(State(state): State<AppState>, admin: Admin, SessionExt(session): SessionExt) -> Response {
    let Some(wan) = webauthn(&state) else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "WebAuthn is misconfigured");
    };
    let rows = sqlx::query("SELECT credential_id FROM webauthn_credentials WHERE user_id=$1")
        .bind(admin.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let exclude: Vec<CredentialID> = rows
        .iter()
        .filter_map(|r| {
            let cid: String = r.get("credential_id");
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cid).ok()
        })
        .map(CredentialID::from)
        .collect();

    match wan.start_passkey_registration(admin.id, &admin.email, &admin.email, Some(exclude)) {
        Ok((ccr, reg_state)) => {
            if let Ok(serialized) = serde_json::to_string(&reg_state) {
                session.lock().unwrap().set(REG_CHALLENGE, json!(serialized));
            }
            Json(ccr).into_response()
        }
        Err(e) => detail(StatusCode::BAD_REQUEST, &format!("{e}")),
    }
}

pub async fn register_complete(State(state): State<AppState>, admin: Admin, SessionExt(session): SessionExt, Json(body): Json<Value>) -> Response {
    let Some(wan) = webauthn(&state) else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "WebAuthn is misconfigured");
    };
    let serialized = {
        let mut s = session.lock().unwrap();
        s.remove(REG_CHALLENGE).and_then(|v| v.as_str().map(|x| x.to_string()))
    };
    let Some(serialized) = serialized else {
        return detail(StatusCode::BAD_REQUEST, "No registration in progress");
    };
    let Ok(reg_state) = serde_json::from_str::<PasskeyRegistration>(&serialized) else {
        return detail(StatusCode::BAD_REQUEST, "No registration in progress");
    };
    let credential: RegisterPublicKeyCredential = match serde_json::from_value(body.get("credential").cloned().unwrap_or(Value::Null)) {
        Ok(c) => c,
        Err(_) => return detail(StatusCode::BAD_REQUEST, "Passkey registration failed"),
    };
    let sk = match wan.finish_passkey_registration(&credential, &reg_state) {
        Ok(sk) => sk,
        Err(e) => {
            tracing::warn!("passkey.register_failed: {e}");
            return detail(StatusCode::BAD_REQUEST, "Passkey registration failed");
        }
    };
    let cred_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sk.cred_id());
    let public_key = serde_json::to_string(&sk).unwrap_or_default();
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let name = if name.is_empty() { "Passkey".to_string() } else { name };
    let inserted: Option<uuid::Uuid> = sqlx::query_scalar(
        "INSERT INTO webauthn_credentials (user_id, credential_id, public_key, sign_count, name, updated_at)
         VALUES ($1,$2,$3,0,$4,now()) RETURNING id",
    )
    .bind(admin.id).bind(&cred_id).bind(&public_key).bind(&name)
    .fetch_optional(&state.db).await.ok().flatten();
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action) VALUES ($1,'passkey.registered')")
        .bind(admin.id).execute(&state.db).await;
    match inserted {
        Some(id) => Json(json!({ "id": id.to_string() })).into_response(),
        None => detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

pub async fn login_begin(State(state): State<AppState>, SessionExt(session): SessionExt) -> Response {
    let Some(wan) = webauthn(&state) else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "WebAuthn is misconfigured");
    };
    let rows = sqlx::query("SELECT public_key FROM webauthn_credentials")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let passkeys: Vec<Passkey> = rows
        .iter()
        .filter_map(|r| serde_json::from_str::<Passkey>(&r.get::<String, _>("public_key")).ok())
        .collect();
    if passkeys.is_empty() {
        return detail(StatusCode::BAD_REQUEST, "No passkeys registered");
    }
    match wan.start_passkey_authentication(&passkeys) {
        Ok((acr, auth_state)) => {
            if let Ok(serialized) = serde_json::to_string(&auth_state) {
                session.lock().unwrap().set(AUTH_CHALLENGE, json!(serialized));
            }
            Json(acr).into_response()
        }
        Err(e) => detail(StatusCode::BAD_REQUEST, &format!("{e}")),
    }
}

pub async fn login_complete(State(state): State<AppState>, SessionExt(session): SessionExt, Json(body): Json<Value>) -> Response {
    let Some(wan) = webauthn(&state) else {
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "WebAuthn is misconfigured");
    };
    let serialized = {
        let mut s = session.lock().unwrap();
        s.remove(AUTH_CHALLENGE).and_then(|v| v.as_str().map(|x| x.to_string()))
    };
    let Some(serialized) = serialized else {
        return detail(StatusCode::UNAUTHORIZED, "No login in progress");
    };
    let Ok(auth_state) = serde_json::from_str::<PasskeyAuthentication>(&serialized) else {
        return detail(StatusCode::UNAUTHORIZED, "No login in progress");
    };
    let credential: PublicKeyCredential = match serde_json::from_value(body.get("credential").cloned().unwrap_or(Value::Null)) {
        Ok(c) => c,
        Err(_) => return detail(StatusCode::UNAUTHORIZED, "Malformed passkey response"),
    };
    let cred_id_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(credential.raw_id.as_ref());
    let row = sqlx::query("SELECT id, user_id, public_key, sign_count FROM webauthn_credentials WHERE credential_id=$1")
        .bind(&cred_id_b64).fetch_optional(&state.db).await.ok().flatten();
    let Some(row) = row else {
        return detail(StatusCode::UNAUTHORIZED, "Unknown passkey");
    };
    let Ok(mut sk) = serde_json::from_str::<Passkey>(&row.get::<String, _>("public_key")) else {
        return detail(StatusCode::UNAUTHORIZED, "Unknown passkey");
    };
    let auth_result = match wan.finish_passkey_authentication(&credential, &auth_state) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("passkey.login_failed: {e}");
            return detail(StatusCode::UNAUTHORIZED, "Passkey authentication failed");
        }
    };
    let new_counter = auth_result.counter();
    if auth_result.needs_update() {
        sk.update_credential(&auth_result);
    }
    let user_id: uuid::Uuid = row.get("user_id");
    let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM users WHERE id=$1")
        .bind(user_id).fetch_optional(&state.db).await.ok().flatten();
    if active != Some(true) {
        return detail(StatusCode::UNAUTHORIZED, "Account is not active");
    }
    let _ = sqlx::query("UPDATE webauthn_credentials SET public_key=$1, sign_count=$2, last_used_at=now(), updated_at=now() WHERE id=$3")
        .bind(serde_json::to_string(&sk).unwrap_or_default())
        .bind(new_counter as i64)
        .bind(row.get::<uuid::Uuid, _>("id"))
        .execute(&state.db).await;
    {
        let mut s = session.lock().unwrap();
        s.set("user_id", json!(user_id.to_string()));
    }
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(user_id).fetch_one(&state.db).await.unwrap_or_default();
    Json(json!({ "id": user_id.to_string(), "email": email })).into_response()
}

pub async fn list_passkeys(State(state): State<AppState>, admin: Admin) -> Response {
    let rows = sqlx::query("SELECT id, name, created_at, last_used_at FROM webauthn_credentials WHERE user_id=$1 ORDER BY created_at")
        .bind(admin.id).fetch_all(&state.db).await.unwrap_or_default();
    let out: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<uuid::Uuid, _>("id").to_string(),
        "name": r.get::<Option<String>, _>("name"),
        "created_at": crate::pyfmt::iso(&r.get::<chrono::DateTime<chrono::Utc>, _>("created_at")),
        "last_used_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at").map(|d| crate::pyfmt::iso(&d)),
    })).collect();
    Json(Value::Array(out)).into_response()
}

pub async fn delete_passkey(State(state): State<AppState>, admin: Admin, axum::extract::Path(passkey_id): axum::extract::Path<uuid::Uuid>) -> Response {
    let _ = sqlx::query("DELETE FROM webauthn_credentials WHERE id=$1 AND user_id=$2")
        .bind(passkey_id).bind(admin.id).execute(&state.db).await;
    StatusCode::NO_CONTENT.into_response()
}
