from functools import lru_cache

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_file_encoding="utf-8", extra="ignore")

    # Database / cache
    database_url: str = Field(
        default="postgresql+asyncpg://portal:portal@portal_postgres:5432/portal"
    )
    redis_url: str = Field(default="redis://portal_redis:6379/0")

    # Ingress
    portal_port: int = Field(default=18080)
    base_url: str = Field(default="http://127.0.0.1:18080")

    # Secrets
    session_secret: str = Field(default="change-me-session-secret")
    token_encryption_key: str = Field(default="change-me-token-encryption-key")

    # Admin seed (single admin in v1)
    admin_email: str = Field(default="admin@example.com")
    admin_password: str = Field(default="change-me")

    # Session cookie
    session_cookie_secure: bool = Field(default=True)
    session_cookie_name: str = Field(default="portal_session")

    # Frame.io / Adobe IMS OAuth
    frameio_client_id: str = Field(default="")
    frameio_client_secret: str = Field(default="")
    # AdobeID is required for Frame.io API access. Adobe auto-adds it during interactive
    # consent, but NOT on refresh, so it must be requested explicitly or refreshed tokens
    # get 401'd by the Frame.io API.
    frameio_scopes: str = Field(
        default="openid,AdobeID,email,profile,offline_access,additional_info.roles"
    )
    # Defaults to {base_url}/api/frameio/callback when left blank.
    frameio_redirect_uri: str = Field(default="")

    # Email (SMTP) — notifications on upload completion (and download links in step 9).
    smtp_host: str = Field(default="")
    smtp_port: int = Field(default=587)
    smtp_username: str = Field(default="")
    smtp_password: str = Field(default="")
    smtp_from: str = Field(default="")
    # Implicit TLS (usually port 465). When false, STARTTLS is used if smtp_starttls is set.
    smtp_use_tls: bool = Field(default=False)
    smtp_starttls: bool = Field(default=True)
    # Where notifications are sent; falls back to admin_email when blank.
    notify_email: str = Field(default="")

    # Email verification (2FA-by-code) for links with "verify email" on.
    verify_code_ttl_seconds: int = Field(default=600)
    verify_max_attempts: int = Field(default=5)
    verify_resend_cooldown_seconds: int = Field(default=30)
    verify_trust_days: int = Field(default=30)
    # Pre-filter a typed address with a DNS MX/A lookup before emailing a code. Disable if the
    # worker has no outbound DNS; the code send is still the real gate.
    verify_check_mx: bool = Field(default=True)

    # Webhooks — reject deliveries whose timestamp is further than this from now (replay window).
    webhook_signature_tolerance_seconds: int = Field(default=300)

    # Sync engine — download retries before a job is dead-lettered, and backoff base (seconds;
    # the nth retry defers by sync_retry_base_seconds * 2**(n-1)). Concurrency = parallel byte
    # ranges per file.
    sync_max_retries: int = Field(default=5)
    sync_retry_base_seconds: int = Field(default=30)
    sync_download_concurrency: int = Field(default=4)
    # Reconciliation polling cadence (minutes). This is the safety net that catches files added
    # directly in Frame.io; client uploads through Portal links trigger sync immediately on
    # completion, so they don't wait for this. Each run lists the watched folder (rate-limited
    # endpoint), so don't set it too low.
    sync_reconcile_interval_minutes: int = Field(default=15)
    # A freshly uploaded large file isn't downloadable until Frame.io finalizes it. While it's not
    # ready, re-check every this many seconds (without counting toward the retry/dead-letter cap),
    # giving up only after the timeout. Big originals (100s of GB) can take a while to finalize.
    sync_source_poll_seconds: int = Field(default=120)
    sync_source_ready_timeout_seconds: int = Field(default=21600)  # 6h
    # Max wall-clock for a single download task. MUST exceed the time to pull your largest file —
    # arq cancels a task past this (arq's own default is only 300s, far too low for big DCPs). Also
    # the threshold past which a still-"running" job is treated as orphaned (worker died) and
    # re-picked by reconcile. Default 24h.
    sync_job_timeout_seconds: int = Field(default=86400)
    # Max sync downloads running at once (arq max_jobs). The NAS link is the bottleneck
    # (~18 MB/s over CIFS-on-VPN), so many parallel multi-GB pulls just thrash and leave
    # everything half-done. Keep this small.
    sync_max_concurrent_jobs: int = Field(default=3)

    log_level: str = Field(default="INFO")

    @property
    def resolved_frameio_redirect_uri(self) -> str:
        base = self.base_url.rstrip("/")
        return self.frameio_redirect_uri or f"{base}/api/frameio/oauth/callback"

    @property
    def email_configured(self) -> bool:
        return bool(self.smtp_host and self.smtp_from)

    @property
    def resolved_notify_email(self) -> str:
        return self.notify_email or self.admin_email


@lru_cache
def get_settings() -> Settings:
    return Settings()
