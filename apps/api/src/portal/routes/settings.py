"""Admin settings — email (SMTP) status + test send.

SMTP is configured via env (lib.config), so this exposes status (never secrets) and a test
button. The fuller settings page (branding defaults, base URL, password change) is build step 12.
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

log = get_logger("routes.settings")
router = APIRouter(prefix="/settings", tags=["settings"])


class EmailStatus(BaseModel):
    configured: bool
    smtp_host: str | None
    smtp_from: str | None
    notify_email: str


@router.get("/email", response_model=EmailStatus)
async def email_status(_: User = Depends(require_admin)) -> EmailStatus:
    s = get_settings()
    return EmailStatus(
        configured=s.email_configured,
        smtp_host=s.smtp_host or None,
        smtp_from=s.smtp_from or None,
        notify_email=s.resolved_notify_email,
    )


@router.post("/email/test")
async def send_email_test(_: User = Depends(require_admin)) -> dict[str, bool]:
    if not get_settings().email_configured:
        raise HTTPException(status_code=400, detail="Email is not configured (set SMTP_* in .env)")
    try:
        await send_test_email()
    except EmailError as exc:
        raise HTTPException(status_code=502, detail=f"Send failed: {exc}") from exc
    return {"ok": True}


class GeneralOut(BaseModel):
    base_url: str


@router.get("/general", response_model=GeneralOut)
async def general(_: User = Depends(require_admin)) -> GeneralOut:
    return GeneralOut(base_url=get_settings().base_url)


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
