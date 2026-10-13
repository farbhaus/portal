
//! Email verification (2FA-by-code) — port of `portal.verify.*`.

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::config::Settings;
use crate::crypto::TokenCipher;
use crate::services::runtime_config::{email_brand, get_email_config};

pub const TRUST_COOKIE: &str = "portal_verified";

#[derive(Debug)]
pub struct VerificationError {
    pub status: u16,
    pub detail: String,
}
impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}
impl std::error::Error for VerificationError {}

fn norm(email: &str) -> String {
    email.trim().to_lowercase()
}
fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.trim().as_bytes()))
}

/// Syntax + (optionally) a DNS A/AAAA plausibility check; DNS errors fail open.
pub async fn is_plausible_email(email: &str, check_mx: bool) -> bool {
    let email = email.trim();
    let ok_syntax = {
        let parts: Vec<&str> = email.split('@').collect();
        parts.len() == 2
            && !parts[0].is_empty()
            && parts[1].contains('.')
            && !parts[1].starts_with('.')
            && !parts[1].ends_with('.')
    };
    if !ok_syntax {
        return false;
    }
    if !check_mx {
        return true;
    }
    let domain = email.rsplit('@').next().unwrap_or("").to_string();
    tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        match (domain.as_str(), 0u16).to_socket_addrs() {
            Ok(mut it) => it.next().is_some(),
            Err(_) => true, // transient/resolver error — fail open; the code send is the real gate
        }
    })
    .await
    .unwrap_or(true)
}

/// Issue a fresh code for `email` (cooldown enforced). Err(detail) on failure.
pub async fn request_code(db: &PgPool, settings: &Settings, email: &str) -> Result<(), VerificationError> {
    let email_config = get_email_config(db, settings).await;
    if !email_config.configured() {
        return Err(VerificationError { status: 503, detail: "Email verification is temporarily unavailable".into() });
    }
    let email = norm(email);
    if !is_plausible_email(&email, settings.verify_check_mx).await {
        return Err(VerificationError { status: 400, detail: "That email address looks invalid".into() });
    }
    let existing = sqlx::query("SELECT last_sent_at FROM email_verifications WHERE email = $1")
        .bind(&email)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
    let now = Utc::now();
    if let Some(row) = &existing {
        let last: chrono::DateTime<Utc> = row.get("last_sent_at");
        if (now - last).num_seconds() < settings.verify_resend_cooldown_seconds {
            return Err(VerificationError { status: 429, detail: "Please wait a moment before requesting another code".into() });
        }
    }
    let code = format!("{:06}", fastrand::u32(0..1_000_000));
    let expires = now + Duration::seconds(settings.verify_code_ttl_seconds);
    let brand = email_brand(db, settings).await;
    // Send first; only persist if the email actually went out.
    crate::notify::send_verification_code(&email_config, &email, &code, settings.verify_code_ttl_seconds / 60, &brand)
        .await
        .map_err(|_| VerificationError { status: 503, detail: "Email verification is temporarily unavailable".into() })?;

    if existing.is_none() {
        // attempts has no DB default (SQLAlchemy sets it).
        let _ = sqlx::query(
            "INSERT INTO email_verifications (email, code_hash, expires_at, attempts, last_sent_at)
             VALUES ($1,$2,$3,0,$4)",
        )
        .bind(&email).bind(hash_code(&code)).bind(expires).bind(now)
        .execute(db).await;
    } else {
        let _ = sqlx::query(
            "UPDATE email_verifications SET code_hash=$1, expires_at=$2, attempts=0, last_sent_at=$3 WHERE email=$4",
        )
        .bind(hash_code(&code)).bind(expires).bind(now).bind(&email).execute(db).await;
    }
    Ok(())
}

/// True iff `code` matches the active, unexpired code for `email` (consumed on success).
pub async fn verify_code(db: &PgPool, settings: &Settings, email: &str, code: &str) -> bool {
    let email = norm(email);
    let row = sqlx::query("SELECT id, code_hash, expires_at, attempts FROM email_verifications WHERE email = $1")
        .bind(&email)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
    let Some(row) = row else { return false };
    let now = Utc::now();
    let expires: chrono::DateTime<Utc> = row.get("expires_at");
    let attempts: i32 = row.get("attempts");
    if expires < now || attempts >= settings.verify_max_attempts {
        return false;
    }
    let _ = sqlx::query("UPDATE email_verifications SET attempts = attempts + 1 WHERE email = $1")
        .bind(&email).execute(db).await;
    if row.get::<String, _>("code_hash") == hash_code(code) {
        let _ = sqlx::query("DELETE FROM email_verifications WHERE email = $1").bind(&email).execute(db).await;
        return true;
    }
    false
}

fn cipher(settings: &Settings) -> TokenCipher {
    TokenCipher::new(&settings.token_encryption_key)
}

/// Is this device already trusted for `email` (encrypted cookie)?
pub fn is_trusted(settings: &Settings, cookies: &str, email: &str) -> bool {
    let raw = cookies.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == TRUST_COOKIE).map(|(_, v)| v.to_string());
    let Some(raw) = raw else { return false };
    let Ok(data) = cipher(settings).decrypt(&raw) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    if v.get("email").and_then(|e| e.as_str()).map(|e| e.to_lowercase()) != Some(norm(email)) {
        return false;
    }
    v.get("exp").and_then(|e| e.as_f64()).map(|e| e > Utc::now().timestamp() as f64).unwrap_or(false)
}

/// Build the `Set-Cookie` value for the device-trust cookie.
pub fn trust_cookie(settings: &Settings, email: &str) -> String {
    let days = settings.verify_trust_days;
    let exp = (Utc::now() + Duration::days(days)).timestamp();
    let payload = serde_json::json!({ "email": norm(email), "exp": exp }).to_string();
    let token = cipher(settings).encrypt(&payload);
    let mut flags = "httponly; samesite=lax".to_string();
    if settings.session_cookie_secure {
        flags.push_str("; secure");
    }
    format!("{TRUST_COOKIE}={token}; Max-Age={}; path=/; {flags}", days * 86400)
}
