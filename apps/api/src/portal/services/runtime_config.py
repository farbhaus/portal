"""DB-backed runtime configuration for email (SMTP) and Frame.io linking.

These settings live in the single app_settings row (managed from the Settings page), not in .env.
Secrets are encrypted at rest with TokenCipher. On first startup `bootstrap_config` imports any
pre-existing .env values once (so an already-deployed instance keeps working after the migration),
after which .env is ignored for these settings.

Static, infra-level config (BASE_URL, DATABASE_URL, secrets, admin seed, sync tuning) stays in
lib.config / .env — only the user-managed email + Frame.io linking config moved here.
"""

from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache

from sqlalchemy.ext.asyncio import AsyncSession

from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher
from portal.lib.logging import get_logger
from portal.services.app_settings import get_app_settings, get_or_create_app_settings

log = get_logger("services.runtime_config")


@lru_cache
def _cipher() -> TokenCipher:
    return TokenCipher(get_settings().token_encryption_key)


@dataclass(frozen=True)
class EmailConfig:
    smtp_host: str
    smtp_port: int
    smtp_username: str
    smtp_password: str
    smtp_from: str
    smtp_use_tls: bool
    smtp_starttls: bool
    notify_email: str

    @property
    def configured(self) -> bool:
        return bool(self.smtp_host and self.smtp_from)

    @property
    def resolved_notify_email(self) -> str:
        return self.notify_email or get_settings().admin_email


@dataclass(frozen=True)
class OAuthConfig:
    client_id: str
    client_secret: str
    scopes: str
    redirect_uri: str

    @property
    def configured(self) -> bool:
        return bool(self.client_id and self.client_secret)


def _decrypt(value: str | None) -> str:
    if not value:
        return ""
    try:
        return _cipher().decrypt(value)
    except Exception:  # corrupt/rotated key — treat as unset rather than crash a send/connect
        log.warning("runtime_config.decrypt_failed")
        return ""


def encrypt_secret(plaintext: str) -> str:
    return _cipher().encrypt(plaintext)


async def get_email_config(db: AsyncSession) -> EmailConfig:
    row = await get_app_settings(db)
    return EmailConfig(
        smtp_host=(row.smtp_host if row else None) or "",
        smtp_port=(row.smtp_port if row and row.smtp_port else 587),
        smtp_username=(row.smtp_username if row else None) or "",
        smtp_password=_decrypt(row.smtp_password_encrypted if row else None),
        smtp_from=(row.smtp_from if row else None) or "",
        smtp_use_tls=bool(row.smtp_use_tls) if row and row.smtp_use_tls is not None else False,
        smtp_starttls=bool(row.smtp_starttls) if row and row.smtp_starttls is not None else True,
        notify_email=(row.notify_email if row else None) or "",
    )


async def get_oauth_config(db: AsyncSession) -> OAuthConfig:
    settings = get_settings()
    row = await get_app_settings(db)
    return OAuthConfig(
        client_id=(row.frameio_client_id if row else None) or "",
        client_secret=_decrypt(row.frameio_client_secret_encrypted if row else None),
        # Scopes and redirect URI remain infra-derived: AdobeID scope is mandatory and the
        # redirect URI follows BASE_URL, so neither is something the admin should hand-edit.
        scopes=settings.frameio_scopes,
        redirect_uri=settings.resolved_frameio_redirect_uri,
    )


async def bootstrap_config(db: AsyncSession) -> None:
    """One-time import of legacy .env email/Frame.io values into the app_settings row.

    Runs once (guarded by config_seeded). Without this, switching to DB-only config would drop a
    live instance's existing .env-based email + Frame.io connection until re-entered in the UI.
    """
    row = await get_or_create_app_settings(db)
    if row.config_seeded:
        return
    settings = get_settings()

    if settings.smtp_host and row.smtp_host is None:
        row.smtp_host = settings.smtp_host
        row.smtp_port = settings.smtp_port
        row.smtp_username = settings.smtp_username or None
        if settings.smtp_password:
            row.smtp_password_encrypted = encrypt_secret(settings.smtp_password)
        row.smtp_from = settings.smtp_from or None
        row.smtp_use_tls = settings.smtp_use_tls
        row.smtp_starttls = settings.smtp_starttls
        row.notify_email = settings.notify_email or None

    if settings.frameio_client_id and row.frameio_client_id is None:
        row.frameio_client_id = settings.frameio_client_id
        if settings.frameio_client_secret:
            row.frameio_client_secret_encrypted = encrypt_secret(settings.frameio_client_secret)

    row.config_seeded = True
    await db.commit()
    log.info("runtime_config.bootstrapped")
