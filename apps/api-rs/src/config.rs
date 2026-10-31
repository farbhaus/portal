
//! Runtime configuration, mirroring `portal.lib.config.Settings` for the fields the HTTP
//! service needs. Values come from the same environment variables the Python service reads.

#[derive(Clone, Debug)]
pub struct Settings {
    pub database_url: String,
    pub redis_url: String,
    pub host: String,
    pub port: u16,
    /// Signing key for the session cookie (`SESSION_SECRET`).
    pub session_secret: String,
    /// `SESSION_COOKIE_SECURE` — adds `secure` to the Set-Cookie flags (default true).
    pub session_cookie_secure: bool,
    /// `SESSION_COOKIE_NAME` — the admin session cookie's name.
    pub session_cookie_name: String,
    /// `BRANDING_LOGO_MAX_BYTES` — the largest app logo an admin may upload.
    pub branding_logo_max_bytes: usize,
    /// `LOG_LEVEL` — Python's level name (DEBUG/INFO/WARNING/ERROR); `RUST_LOG` wins when set.
    pub log_level: String,
    /// Public base URL (`BASE_URL`), used to build link `public_url`s.
    pub base_url: String,
    /// `TOKEN_ENCRYPTION_KEY` — AES key material for secrets at rest.
    pub token_encryption_key: String,
    /// `FRAMEIO_SCOPES` — echoed back by `GET /api/frameio/status`.
    pub frameio_scopes: String,
    /// `ADMIN_EMAIL` — the fallback notify address.
    pub admin_email: String,
    /// `ADMIN_PASSWORD` — the first-boot admin's password (seed only).
    pub admin_password: String,
    /// `PORTAL_API_TOKEN` — optional bearer token for machine access to the admin API.
    pub api_token: Option<String>,
    /// `ALLOW_INSECURE_DEFAULTS` — permit the `.env.example` placeholder secrets (local-only runs).
    pub allow_insecure_defaults: bool,
    /// `FRAMEIO_REDIRECT_URI`, defaulting to `{base_url}/api/frameio/oauth/callback`.
    pub frameio_redirect_uri: String,
    // --- sync engine / retention tuning (portal.lib.config defaults) ---
    pub sync_max_retries: i32,
    pub sync_retry_base_seconds: i64,
    pub sync_download_concurrency: usize,
    pub sync_reconcile_interval_minutes: i64,
    pub sync_source_poll_seconds: i64,
    pub sync_source_ready_timeout_seconds: i64,
    pub sync_pending_stale_seconds: i64,
    pub sync_job_timeout_seconds: i64,
    pub sync_max_concurrent_jobs: usize,
    pub webhook_signature_tolerance_seconds: i64,
    pub retention_webhook_events_days: i64,
    pub retention_sessions_days: i64,
    pub retention_audit_log_days: i64,
    pub upload_stale_after_minutes: i64,
    // --- public-endpoint rate limiting, lockout and ingress ---
    pub rate_limit_enabled: bool,
    pub rate_limit_default_per_minute: i64,
    pub rate_limit_strict_per_minute: i64,
    pub password_lockout_max_failures: i64,
    pub password_lockout_window_seconds: i64,
    pub trusted_proxy_hops: usize,
    // --- email verification ---
    pub verify_code_ttl_seconds: i64,
    pub verify_max_attempts: i32,
    pub verify_resend_cooldown_seconds: i64,
    pub verify_trust_days: i64,
    pub verify_check_mx: bool,
    pub webauthn_rp_id: String,
    pub webauthn_rp_name: String,
}

/// Parse a boolean the way pydantic does (`1/true/t/yes/y/on` and `0/false/f/no/n/off`, any
/// case), so a value the Python service accepts means the same thing here.
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "t" | "yes" | "y" | "on" => Some(true),
        "0" | "false" | "f" | "no" | "n" | "off" => Some(false),
        _ => None,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key).ok().and_then(|v| parse_bool(&v)).unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The placeholder values shipped in `.env.example`. Booting with any of these means the deployer
/// never set a real secret.
const INSECURE_DEFAULTS: [(&str, &str); 3] = [
    ("admin_password", "change-me"),
    ("session_secret", "change-me-session-secret"),
    ("token_encryption_key", "change-me-token-encryption-key"),
];

impl Settings {
    /// Refuse to start if any secret is still the `.env.example` placeholder — port of
    /// `require_secure_secrets`. Left at its default, `session_secret` makes session cookies
    /// forgeable (full admin takeover) and `token_encryption_key` weakly keys every secret at rest.
    pub fn require_secure_secrets(&self) -> anyhow::Result<()> {
        if self.allow_insecure_defaults {
            return Ok(());
        }
        let offenders: Vec<&str> = INSECURE_DEFAULTS
            .iter()
            .filter(|(name, default)| {
                let value = match *name {
                    "admin_password" => &self.admin_password,
                    "session_secret" => &self.session_secret,
                    _ => &self.token_encryption_key,
                };
                value == default
            })
            .map(|(name, _)| *name)
            .collect();
        if offenders.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "Refusing to start with default secret(s): {}. Set strong values in .env (see \
             deploy/.env.example), or set ALLOW_INSECURE_DEFAULTS=true for local-only use.",
            offenders.join(", ")
        )
    }

    /// The tracing filter for `LOG_LEVEL`, mapping Python's level names onto tracing's.
    pub fn log_filter(&self) -> &'static str {
        match self.log_level.trim().to_ascii_uppercase().as_str() {
            "DEBUG" => "debug",
            "WARNING" | "WARN" => "warn",
            "ERROR" | "CRITICAL" => "error",
            _ => "info",
        }
    }

    pub fn resolved_frameio_redirect_uri(&self) -> String {
        if self.frameio_redirect_uri.is_empty() {
            format!("{}/api/frameio/oauth/callback", self.base_url.trim_end_matches('/'))
        } else {
            self.frameio_redirect_uri.clone()
        }
    }
}

impl Settings {
    pub fn from_env() -> Self {
        // Python's DSN carries a SQLAlchemy driver suffix (`+asyncpg`); sqlx wants the bare
        // scheme. Translate rather than require the operator to run two different values.
        let raw_db = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://portal@127.0.0.1:5432/portal".to_string());
        let database_url = raw_db.replace("postgresql+asyncpg://", "postgresql://");
        let session_cookie_secure = env_bool("SESSION_COOKIE_SECURE", true);
        Self {
            database_url,
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379/0".to_string()),
            host: std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000),
            session_secret: std::env::var("SESSION_SECRET")
                .unwrap_or_else(|_| "change-me-session-secret".to_string()),
            session_cookie_secure,
            session_cookie_name: std::env::var("SESSION_COOKIE_NAME").unwrap_or_else(|_| "portal_session".to_string()),
            branding_logo_max_bytes: env_i64("BRANDING_LOGO_MAX_BYTES", 2 * 1024 * 1024).max(0) as usize,
            log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "INFO".to_string()),
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18080".to_string()),
            token_encryption_key: std::env::var("TOKEN_ENCRYPTION_KEY")
                .unwrap_or_else(|_| "change-me-token-encryption-key".to_string()),
            frameio_scopes: std::env::var("FRAMEIO_SCOPES").unwrap_or_else(|_| {
                "openid,AdobeID,email,profile,offline_access,additional_info.roles".to_string()
            }),
            admin_email: std::env::var("ADMIN_EMAIL")
                .unwrap_or_else(|_| "admin@example.com".to_string()),
            admin_password: std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "change-me".to_string()),
            api_token: std::env::var("PORTAL_API_TOKEN")
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
            allow_insecure_defaults: env_bool("ALLOW_INSECURE_DEFAULTS", false),
            frameio_redirect_uri: std::env::var("FRAMEIO_REDIRECT_URI").unwrap_or_default(),
            sync_max_retries: env_i64("SYNC_MAX_RETRIES", 5) as i32,
            sync_retry_base_seconds: env_i64("SYNC_RETRY_BASE_SECONDS", 30),
            sync_download_concurrency: env_i64("SYNC_DOWNLOAD_CONCURRENCY", 4).max(1) as usize,
            sync_reconcile_interval_minutes: env_i64("SYNC_RECONCILE_INTERVAL_MINUTES", 15),
            sync_source_poll_seconds: env_i64("SYNC_SOURCE_POLL_SECONDS", 120),
            sync_source_ready_timeout_seconds: env_i64("SYNC_SOURCE_READY_TIMEOUT_SECONDS", 21600),
            sync_pending_stale_seconds: env_i64("SYNC_PENDING_STALE_SECONDS", 1800),
            sync_job_timeout_seconds: env_i64("SYNC_JOB_TIMEOUT_SECONDS", 86400),
            sync_max_concurrent_jobs: env_i64("SYNC_MAX_CONCURRENT_JOBS", 3).max(1) as usize,
            webhook_signature_tolerance_seconds: env_i64("WEBHOOK_SIGNATURE_TOLERANCE_SECONDS", 300),
            retention_webhook_events_days: env_i64("RETENTION_WEBHOOK_EVENTS_DAYS", 90),
            retention_sessions_days: env_i64("RETENTION_SESSIONS_DAYS", 365),
            retention_audit_log_days: env_i64("RETENTION_AUDIT_LOG_DAYS", 0),
            upload_stale_after_minutes: env_i64("UPLOAD_STALE_AFTER_MINUTES", 30),
            rate_limit_enabled: env_bool("RATE_LIMIT_ENABLED", true),
            rate_limit_default_per_minute: env_i64("RATE_LIMIT_DEFAULT_PER_MINUTE", 120),
            rate_limit_strict_per_minute: env_i64("RATE_LIMIT_STRICT_PER_MINUTE", 12),
            password_lockout_max_failures: env_i64("PASSWORD_LOCKOUT_MAX_FAILURES", 30),
            password_lockout_window_seconds: env_i64("PASSWORD_LOCKOUT_WINDOW_SECONDS", 900),
            trusted_proxy_hops: env_i64("TRUSTED_PROXY_HOPS", 2).max(0) as usize,
            verify_code_ttl_seconds: env_i64("VERIFY_CODE_TTL_SECONDS", 600),
            verify_max_attempts: env_i64("VERIFY_MAX_ATTEMPTS", 5) as i32,
            verify_resend_cooldown_seconds: env_i64("VERIFY_RESEND_COOLDOWN_SECONDS", 30),
            verify_trust_days: env_i64("VERIFY_TRUST_DAYS", 30),
            verify_check_mx: env_bool("VERIFY_CHECK_MX", true),
            webauthn_rp_id: std::env::var("WEBAUTHN_RP_ID").unwrap_or_default(),
            webauthn_rp_name: std::env::var("WEBAUTHN_RP_NAME").unwrap_or_else(|_| "Portal".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_parse_like_pydantic() {
        for t in ["1", "true", "TRUE", "t", "yes", "Y", "on", " true "] {
            assert_eq!(parse_bool(t), Some(true), "{t}");
        }
        for f in ["0", "false", "f", "no", "N", "off"] {
            assert_eq!(parse_bool(f), Some(false), "{f}");
        }
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn refuses_placeholder_secrets() {
        let mut s = Settings::from_env();
        s.allow_insecure_defaults = false;
        s.admin_password = "a-real-password".into();
        s.session_secret = "change-me-session-secret".into();
        s.token_encryption_key = "change-me-token-encryption-key".into();
        let err = s.require_secure_secrets().unwrap_err().to_string();
        assert!(err.starts_with("Refusing to start with default secret(s): session_secret, token_encryption_key."), "{err}");
        s.allow_insecure_defaults = true;
        assert!(s.require_secure_secrets().is_ok());
        s.allow_insecure_defaults = false;
        s.session_secret = "x".repeat(32);
        s.token_encryption_key = "y".repeat(32);
        assert!(s.require_secure_secrets().is_ok());
        s.admin_password = "change-me".into();
        assert!(s.require_secure_secrets().is_err());
    }
}
