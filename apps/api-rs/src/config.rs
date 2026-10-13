
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
    /// Public base URL (`BASE_URL`), used to build link `public_url`s.
    pub base_url: String,
    /// `TOKEN_ENCRYPTION_KEY` — AES key material for secrets at rest.
    pub token_encryption_key: String,
    /// `FRAMEIO_SCOPES` — echoed back by `GET /api/frameio/status`.
    pub frameio_scopes: String,
    /// `ADMIN_EMAIL` — the fallback notify address.
    pub admin_email: String,
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

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Settings {
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
        let session_cookie_secure = std::env::var("SESSION_COOKIE_SECURE")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(true);
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
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18080".to_string()),
            token_encryption_key: std::env::var("TOKEN_ENCRYPTION_KEY")
                .unwrap_or_else(|_| "change-me-token-encryption-key".to_string()),
            frameio_scopes: std::env::var("FRAMEIO_SCOPES").unwrap_or_else(|_| {
                "openid,AdobeID,email,profile,offline_access,additional_info.roles".to_string()
            }),
            admin_email: std::env::var("ADMIN_EMAIL")
                .unwrap_or_else(|_| "admin@example.com".to_string()),
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
