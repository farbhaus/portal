import secrets

from fastapi import APIRouter, Depends, Request
from fastapi.responses import RedirectResponse
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import get_current_user, require_admin
from portal.db.models import AuditLog, User
from portal.db.session import get_session
from portal.frameio import connection as conn_svc
from portal.frameio import oauth
from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("routes.frameio")
router = APIRouter(prefix="/frameio", tags=["frameio"])

_STATE_SESSION_KEY = "frameio_oauth_state"


class ConnectionStatus(BaseModel):
    connected: bool
    adobe_email: str | None = None
    expires_at: str | None = None
    needs_refresh: bool | None = None
    scopes: str | None = None


def _web_redirect(suffix: str) -> RedirectResponse:
    base = get_settings().base_url.rstrip("/")
    return RedirectResponse(url=f"{base}{suffix}", status_code=302)


@router.get("/oauth/connect")
async def connect(request: Request, _: User = Depends(require_admin)) -> RedirectResponse:
    state = secrets.token_urlsafe(32)
    request.session[_STATE_SESSION_KEY] = state
    return RedirectResponse(url=oauth.build_authorize_url(state), status_code=302)


@router.get("/oauth/callback")
async def callback(
    request: Request,
    state: str | None = None,
    code: str | None = None,
    error: str | None = None,
    db: AsyncSession = Depends(get_session),
    user: User | None = Depends(get_current_user),
) -> RedirectResponse:
    expected = request.session.pop(_STATE_SESSION_KEY, None)
    if user is None:
        return _web_redirect("/login")
    if error or not code or not state or state != expected:
        log.warning("frameio.callback.rejected", has_code=bool(code), state_ok=state == expected)
        return _web_redirect("/connections?frameio=error")
    try:
        tokens = await oauth.exchange_code(code)
        identity = await oauth.fetch_identity(tokens.access_token)
        await conn_svc.upsert_connection(db, user.id, tokens, identity)
    except oauth.FrameioOAuthError:
        return _web_redirect("/connections?frameio=error")

    db.add(AuditLog(user_id=user.id, action="frameio.connected", detail={"email": identity.email}))
    await db.commit()
    return _web_redirect("/connections?frameio=connected")


@router.get("/status", response_model=ConnectionStatus)
async def status(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> ConnectionStatus:
    conn = await conn_svc.get_connection(db, user.id)
    if conn is None:
        return ConnectionStatus(connected=False)
    return ConnectionStatus(
        connected=True,
        adobe_email=conn.adobe_email,
        expires_at=conn.token_expires_at.isoformat() if conn.token_expires_at else None,
        needs_refresh=conn_svc.needs_refresh(conn),
        scopes=get_settings().frameio_scopes,
    )


@router.post("/disconnect")
async def disconnect(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> dict[str, bool]:
    conn = await conn_svc.get_connection(db, user.id)
    if conn is not None:
        await db.delete(conn)
        db.add(AuditLog(user_id=user.id, action="frameio.disconnected"))
        await db.commit()
    return {"ok": True}
