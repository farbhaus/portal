"""Admin settings — email (SMTP) status + test send.

SMTP is configured via env (lib.config), so this exposes status (never secrets) and a test
button. The fuller settings page (branding defaults, base URL, password change) is build step 12.
"""

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from portal.auth.session import require_admin
from portal.db.models import User
from portal.lib.config import get_settings
from portal.notify.email import EmailError, send_test_email

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
