
//! Persistence and token lifecycle for the Frame.io connection — port of
//! `portal.frameio.connection`. Tokens are AES-GCM encrypted at rest.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::Settings;
use crate::crypto::TokenCipher;
use crate::frameio::oauth::{self, FrameioOAuthError, TokenSet};

pub const REFRESH_SKEW_SECONDS: i64 = 300;

pub struct ConnectionRow {
    pub user_id: Uuid,
    pub access_token_encrypted: Option<String>,
    pub refresh_token_encrypted: Option<String>,
    pub token_expires_at: Option<DateTime<Utc>>,
}

pub async fn get_connection(db: &PgPool, user_id: Uuid) -> sqlx::Result<Option<ConnectionRow>> {
    let row = sqlx::query(
        "SELECT user_id, access_token_encrypted, refresh_token_encrypted, token_expires_at
           FROM frameio_connections WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| ConnectionRow {
        user_id: r.get("user_id"),
        access_token_encrypted: r.get("access_token_encrypted"),
        refresh_token_encrypted: r.get("refresh_token_encrypted"),
        token_expires_at: r.get("token_expires_at"),
    }))
}

/// The single connection used for API access (v1: one admin, one connection).
pub async fn get_primary_connection(db: &PgPool) -> sqlx::Result<Option<ConnectionRow>> {
    let row = sqlx::query(
        "SELECT user_id, access_token_encrypted, refresh_token_encrypted, token_expires_at
           FROM frameio_connections ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| ConnectionRow {
        user_id: r.get("user_id"),
        access_token_encrypted: r.get("access_token_encrypted"),
        refresh_token_encrypted: r.get("refresh_token_encrypted"),
        token_expires_at: r.get("token_expires_at"),
    }))
}

pub fn needs_refresh(expires_at: Option<DateTime<Utc>>, skew: i64) -> bool {
    match expires_at {
        None => true,
        Some(exp) => (exp - Utc::now()).num_seconds() <= skew,
    }
}

/// Client id/secret from `app_settings`, scopes/redirect from env — as `get_oauth_config` does.
pub async fn oauth_config(db: &PgPool, settings: &Settings) -> Result<oauth::OAuthConfig, FrameioOAuthError> {
    let row = sqlx::query(
        "SELECT frameio_client_id, frameio_client_secret_encrypted FROM app_settings LIMIT 1",
    )
    .fetch_optional(db)
    .await
    .map_err(|e| FrameioOAuthError(format!("config read failed: {e}")))?;
    let (client_id, secret_enc) = match row {
        Some(r) => (
            r.get::<Option<String>, _>("frameio_client_id").unwrap_or_default(),
            r.get::<Option<String>, _>("frameio_client_secret_encrypted"),
        ),
        None => (String::new(), None),
    };
    let cipher = TokenCipher::new(&settings.token_encryption_key);
    let client_secret = secret_enc
        .and_then(|s| cipher.decrypt(&s).ok())
        .unwrap_or_default();
    Ok(oauth::OAuthConfig {
        client_id,
        client_secret,
        scopes: settings.frameio_scopes.clone(),
        redirect_uri: settings.resolved_frameio_redirect_uri(),
    })
}

/// Return a usable access token, refreshing via IMS first if expired/near-expiry.
pub async fn get_valid_access_token(
    db: &PgPool,
    settings: &Settings,
    http: &reqwest::Client,
    conn: &ConnectionRow,
) -> Result<String, FrameioOAuthError> {
    let cipher = TokenCipher::new(&settings.token_encryption_key);
    if !needs_refresh(conn.token_expires_at, REFRESH_SKEW_SECONDS) {
        let Some(enc) = &conn.access_token_encrypted else {
            return Err(FrameioOAuthError("No access token stored".into()));
        };
        return cipher
            .decrypt(enc)
            .map_err(|_| FrameioOAuthError("No access token stored".into()));
    }
    let Some(refresh_enc) = &conn.refresh_token_encrypted else {
        return Err(FrameioOAuthError("No refresh token; reconnect required".into()));
    };
    let refresh_token = cipher
        .decrypt(refresh_enc)
        .map_err(|_| FrameioOAuthError("No refresh token; reconnect required".into()))?;
    let config = oauth_config(db, settings).await?;
    let tokens: TokenSet = oauth::refresh_tokens(http, &config, &refresh_token).await?;
    sqlx::query(
        "UPDATE frameio_connections SET access_token_encrypted = $1,
            refresh_token_encrypted = COALESCE($2, refresh_token_encrypted),
            token_expires_at = $3, updated_at = now() WHERE user_id = $4",
    )
    .bind(cipher.encrypt(&tokens.access_token))
    .bind(tokens.refresh_token.as_deref().map(|t| cipher.encrypt(t)))
    .bind(tokens.expires_at)
    .bind(conn.user_id)
    .execute(db)
    .await
    .map_err(|e| FrameioOAuthError(format!("token persist failed: {e}")))?;
    Ok(tokens.access_token)
}

/// Insert-or-update the connection after an OAuth exchange.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_connection(
    db: &PgPool,
    settings: &Settings,
    user_id: Uuid,
    tokens: &TokenSet,
    identity: &oauth::Identity,
) -> sqlx::Result<()> {
    let cipher = TokenCipher::new(&settings.token_encryption_key);
    let existing = get_connection(db, user_id).await?;
    if existing.is_some() {
        sqlx::query(
            "UPDATE frameio_connections SET access_token_encrypted = $1,
                refresh_token_encrypted = COALESCE($2, refresh_token_encrypted),
                token_expires_at = $3, adobe_ims_user_id = $4, adobe_email = $5,
                updated_at = now() WHERE user_id = $6",
        )
        .bind(cipher.encrypt(&tokens.access_token))
        .bind(tokens.refresh_token.as_deref().map(|t| cipher.encrypt(t)))
        .bind(tokens.expires_at)
        .bind(identity.frameio_user_id.as_deref())
        .bind(identity.email.as_deref())
        .bind(user_id)
        .execute(db)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO frameio_connections (user_id, access_token_encrypted,
                refresh_token_encrypted, token_expires_at, adobe_ims_user_id, adobe_email)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(cipher.encrypt(&tokens.access_token))
        .bind(tokens.refresh_token.as_deref().map(|t| cipher.encrypt(t)))
        .bind(tokens.expires_at)
        .bind(identity.frameio_user_id.as_deref())
        .bind(identity.email.as_deref())
        .execute(db)
        .await?;
    }
    Ok(())
}
