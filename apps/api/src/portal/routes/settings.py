"""Admin settings — email (SMTP) and Frame.io linking config + account password.

Email and Frame.io linking config live in the app_settings row (managed here), not in .env.
Secrets are never returned in GET responses — status shows only whether each is set. Writing a
secret as an empty string leaves the stored value unchanged; sending an explicit clear flag wipes
it. The fuller branding/security settings live in routes.security.
"""

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.passwords import hash_password, verify_password
from portal.auth.session import require_admin
from portal.db.models import AuditLog, User
from portal.db.session import get_session
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.notify.email import EmailError, send_test_email
from portal.services.app_settings import email_brand, get_app_settings, get_or_create_app_settings
from portal.services.runtime_config import encrypt_secret, get_email_config, get_oauth_config

log = get_logger("routes.settings")
router = APIRouter(prefix="/settings", tags=["settings"])


# ── Email (SMTP) ──────────────────────────────────────────────────────────────


class EmailStatus(BaseModel):
    configured: bool
    smtp_host: str | None
    smtp_port: int
    smtp_username: str | None
    smtp_from: str | None
    smtp_use_tls: bool
    smtp_starttls: bool
    notify_email: str
    has_password: bool


class EmailUpdate(BaseModel):
    smtp_host: str = ""
    smtp_port: int = Field(default=587, ge=1, le=65535)
    smtp_username: str = ""
    # Blank leaves the stored password unchanged; use clear_password to remove it.
    smtp_password: str | None = None
    clear_password: bool = False
    smtp_from: str = ""
    smtp_use_tls: bool = False
    smtp_starttls: bool = True
    notify_email: str = ""


@router.get("/email", response_model=EmailStatus)
async def email_status(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> EmailStatus:
    cfg = await get_email_config(db)
    row = await get_app_settings(db)
    return EmailStatus(
        configured=cfg.configured,
        smtp_host=cfg.smtp_host or None,
        smtp_port=cfg.smtp_port,
        smtp_username=cfg.smtp_username or None,
        smtp_from=cfg.smtp_from or None,
        smtp_use_tls=cfg.smtp_use_tls,
        smtp_starttls=cfg.smtp_starttls,
        notify_email=cfg.resolved_notify_email,
        has_password=bool(row and row.smtp_password_encrypted),
    )


@router.post("/email", response_model=EmailStatus)
async def update_email(
    body: EmailUpdate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> EmailStatus:
    row = await get_or_create_app_settings(db)
    row.smtp_host = body.smtp_host.strip() or None
    row.smtp_port = body.smtp_port
    row.smtp_username = body.smtp_username.strip() or None
    row.smtp_from = body.smtp_from.strip() or None
    row.smtp_use_tls = body.smtp_use_tls
    row.smtp_starttls = body.smtp_starttls
    row.notify_email = body.notify_email.strip() or None
    if body.clear_password:
        row.smtp_password_encrypted = None
    elif body.smtp_password:
        row.smtp_password_encrypted = encrypt_secret(body.smtp_password)
    db.add(AuditLog(user_id=user.id, action="settings.email_updated"))
    await db.commit()
    log.info("settings.email_updated", host=row.smtp_host)
    return await email_status(user, db)


@router.post("/email/test")
async def send_email_test(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> dict[str, bool]:
    cfg = await get_email_config(db)
    if not cfg.configured:
        raise HTTPException(status_code=400, detail="Email is not configured")
    brand = await email_brand(db, get_settings().base_url)
    try:
        await send_test_email(cfg, brand)
    except EmailError as exc:
        raise HTTPException(status_code=502, detail=f"Send failed: {exc}") from exc
    return {"ok": True}


# ── Frame.io linking (Adobe IMS OAuth client credentials) ─────────────────────


class FrameioConfigStatus(BaseModel):
    configured: bool
    client_id: str | None
    has_secret: bool
    redirect_uri: str


class FrameioConfigUpdate(BaseModel):
    client_id: str = ""
    # Blank leaves the stored secret unchanged; use clear_secret to remove it.
    client_secret: str | None = None
    clear_secret: bool = False


@router.get("/frameio-config", response_model=FrameioConfigStatus)
async def frameio_config_status(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> FrameioConfigStatus:
    cfg = await get_oauth_config(db)
    row = await get_app_settings(db)
    return FrameioConfigStatus(
        configured=cfg.configured,
        client_id=cfg.client_id or None,
        has_secret=bool(row and row.frameio_client_secret_encrypted),
        redirect_uri=cfg.redirect_uri,
    )


@router.post("/frameio-config", response_model=FrameioConfigStatus)
async def update_frameio_config(
    body: FrameioConfigUpdate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> FrameioConfigStatus:
    row = await get_or_create_app_settings(db)
    row.frameio_client_id = body.client_id.strip() or None
    if body.clear_secret:
        row.frameio_client_secret_encrypted = None
    elif body.client_secret:
        row.frameio_client_secret_encrypted = encrypt_secret(body.client_secret)
    db.add(AuditLog(user_id=user.id, action="settings.frameio_config_updated"))
    await db.commit()
    log.info("settings.frameio_config_updated", client_id=row.frameio_client_id)
    return await frameio_config_status(user, db)


# ── Account password ──────────────────────────────────────────────────────────


class PasswordChange(BaseModel):
    current_password: str
    new_password: str = Field(min_length=8)


@router.post("/password")
async def change_password(
    body: PasswordChange,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, bool]:
    if not verify_password(body.current_password, user.password_hash):
        raise HTTPException(status_code=403, detail="Current password is incorrect")
    # require_admin and this endpoint share the request-scoped session, so `user` is attached here.
    user.password_hash = hash_password(body.new_password)
    db.add(AuditLog(user_id=user.id, action="admin.password_changed"))
    await db.commit()
    log.info("settings.password_changed", user_id=str(user.id))
    return {"ok": True}
