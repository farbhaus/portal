
//! Admin authentication: the `require_admin` extractor and the password login, mirroring
//! `portal.auth.session` and `portal.routes.auth.login`.

pub mod totp;
pub mod webauthn;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::detail;
use crate::session::SessionHandle;
use crate::state::AppState;

/// Fixed bcrypt hash used when the email doesn't match an account, so login pays one verify
/// either way (constant-time account enumeration guard, mirroring `verify_dummy`).
const DUMMY_HASH: &str = "$2b$12$EJusDFY5AD5/1C0Xe5Fk9.IQPOlhkjmNkDD.uHSVZSg1k77I.mWnq";

pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

fn session_handle(parts: &Parts) -> SessionHandle {
    parts
        .extensions
        .get::<SessionHandle>()
        .cloned()
        .expect("session middleware must run before auth extractors")
}

pub fn session_user_id(parts: &Parts) -> Option<String> {
    let handle = session_handle(parts);
    let mut s = handle.lock().unwrap();
    s.get("user_id")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

#[derive(Clone)]
pub struct SessionExt(pub SessionHandle);

impl<S> axum::extract::FromRequestParts<S> for SessionExt
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(SessionExt(session_handle(parts)))
    }
}

pub struct Admin {
    #[allow(dead_code)]
    pub id: Uuid,
    pub email: String,
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(uid) = session_user_id(parts) else {
            return Err(detail(StatusCode::UNAUTHORIZED, "Authentication required"));
        };
        let Ok(id) = Uuid::parse_str(&uid) else {
            return Err(detail(StatusCode::UNAUTHORIZED, "Authentication required"));
        };
        let row = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, email FROM users WHERE id = $1 AND is_active",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| detail(StatusCode::UNAUTHORIZED, "Authentication required"))?;
        match row {
            Some((id, email)) => Ok(Admin { id, email }),
            None => Err(detail(StatusCode::UNAUTHORIZED, "Authentication required")),
        }
    }
}

pub fn invalid_credentials() -> Response {
    detail(StatusCode::UNAUTHORIZED, "Invalid email or password")
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    SessionExt(session): SessionExt,
    axum::extract::Json(body): axum::extract::Json<LoginRequest>,
) -> Response {
    let row = sqlx::query_as::<_, (Uuid, String, bool, bool)>(
        "SELECT id, password_hash, is_active, totp_enabled FROM users WHERE email = $1",
    )
    .bind(&body.email)
    .fetch_optional(&state.db)
    .await;

    let Ok(Some((id, password_hash, is_active, totp_enabled))) = row else {
        verify_password(&body.password, DUMMY_HASH);
        return invalid_credentials();
    };
    if !is_active {
        verify_password(&body.password, DUMMY_HASH);
        return invalid_credentials();
    }
    if !verify_password(&body.password, &password_hash) {
        return invalid_credentials();
    }
    if totp_enabled {
        let mut s = session.lock().unwrap();
        s.set("totp_pending_uid", json!(id.to_string()));
        s.set(
            "totp_pending_ts",
            json!(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)),
        );
        s.set("totp_pending_attempts", json!(0));
        return (
            StatusCode::OK,
            axum::Json(json!({ "totp_required": true, "id": null, "email": null })),
        )
            .into_response();
    }
    {
        let mut s = session.lock().unwrap();
        s.set("user_id", json!(id.to_string()));
    }
    (
        StatusCode::OK,
        axum::Json(json!({ "totp_required": false, "id": id.to_string(), "email": body.email })),
    )
        .into_response()
}

const TOTP_PENDING_TTL: i64 = 300;
const TOTP_MAX_ATTEMPTS: i64 = 5;

fn clear_totp_pending(session: &SessionHandle) {
    let mut s = session.lock().unwrap();
    s.remove("totp_pending_uid");
    s.remove("totp_pending_ts");
    s.remove("totp_pending_attempts");
}

/// `POST /api/auth/login/totp` — the second factor after a password step.
pub async fn login_totp(
    State(state): State<AppState>,
    SessionExt(session): SessionExt,
    axum::extract::Json(body): axum::extract::Json<LoginRequest>,
) -> Response {
    let (uid, ts) = {
        let mut s = session.lock().unwrap();
        (
            s.get("totp_pending_uid").and_then(|v| v.as_str()).map(|x| x.to_string()),
            s.get("totp_pending_ts").and_then(|v| v.as_i64()),
        )
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(uid) = uid else {
        clear_totp_pending(&session);
        return detail(StatusCode::UNAUTHORIZED, "Your login session expired — start again");
    };
    let Some(ts) = ts else {
        clear_totp_pending(&session);
        return detail(StatusCode::UNAUTHORIZED, "Your login session expired — start again");
    };
    if now - ts > TOTP_PENDING_TTL {
        clear_totp_pending(&session);
        return detail(StatusCode::UNAUTHORIZED, "Your login session expired — start again");
    }
    let Ok(id) = Uuid::parse_str(&uid) else {
        clear_totp_pending(&session);
        return detail(StatusCode::UNAUTHORIZED, "Invalid login");
    };
    let row: Option<(bool, Option<String>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT totp_enabled, totp_secret_encrypted, totp_recovery_codes FROM users WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((totp_enabled, secret_enc, recovery)) = row else {
        return detail(StatusCode::UNAUTHORIZED, "Invalid login");
    };
    if !totp_enabled || secret_enc.is_none() {
        return detail(StatusCode::UNAUTHORIZED, "Invalid login");
    }
    let secret = crate::crypto::TokenCipher::new(&state.settings.token_encryption_key)
        .decrypt(secret_enc.as_deref().unwrap_or(""))
        .unwrap_or_default();
    let code = body.password.clone();
    let mut ok = crate::auth::totp::verify_code(&secret, &code);
    if !ok {
        let hashes: Vec<String> = recovery
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if let Some(remaining) = crate::auth::totp::consume_recovery_code(&code, &hashes) {
            let _ = sqlx::query("UPDATE users SET totp_recovery_codes=$1, updated_at=now() WHERE id=$2")
                .bind(serde_json::json!(remaining))
                .bind(id)
                .execute(&state.db)
                .await;
            ok = true;
        }
    }
    if !ok {
        let attempts = {
            let mut s = session.lock().unwrap();
            let a = s.get("totp_pending_attempts").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
            s.set("totp_pending_attempts", serde_json::json!(a));
            a
        };
        if attempts >= TOTP_MAX_ATTEMPTS {
            clear_totp_pending(&session);
            return detail(StatusCode::UNAUTHORIZED, "Too many incorrect codes — sign in again");
        }
        return detail(StatusCode::UNAUTHORIZED, "Invalid code");
    }
    clear_totp_pending(&session);
    {
        let mut s = session.lock().unwrap();
        s.set("user_id", serde_json::json!(id.to_string()));
    }
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(id).fetch_one(&state.db).await.unwrap_or_default();
    axum::Json(serde_json::json!({ "id": id.to_string(), "email": email })).into_response()
}
