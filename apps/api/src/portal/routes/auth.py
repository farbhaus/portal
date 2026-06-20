import json
import time
import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, EmailStr
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth import totp as totp_svc
from portal.auth import webauthn as webauthn_svc
from portal.auth.passwords import verify_password
from portal.auth.session import (
    clear_session,
    login_session,
    require_admin,
)
from portal.db.models import AuditLog, User, WebauthnCredential
from portal.db.session import get_session
from portal.lib.errors import AuthError
from portal.lib.logging import get_logger

log = get_logger("routes.auth")
router = APIRouter(prefix="/auth", tags=["auth"])

# Session keys for the WebAuthn challenge handoff and the TOTP password-verified-awaiting-code gate.
_REG_CHALLENGE = "webauthn_reg_challenge"
_AUTH_CHALLENGE = "webauthn_auth_challenge"
_TOTP_PENDING_UID = "totp_pending_uid"
_TOTP_PENDING_TS = "totp_pending_ts"
_TOTP_PENDING_TTL = 300  # seconds the password step stays valid before the code is required again


class LoginRequest(BaseModel):
    email: EmailStr
    password: str


class UserOut(BaseModel):
    id: str
    email: str


class LoginResult(BaseModel):
    # When totp_required is true, no session is set yet — the client must POST /auth/login/totp.
    totp_required: bool = False
    id: str | None = None
    email: str | None = None


@router.post("/login", response_model=LoginResult)
async def login(
    body: LoginRequest, request: Request, db: AsyncSession = Depends(get_session)
) -> LoginResult:
    result = await db.execute(select(User).where(User.email == body.email))
    user = result.scalar_one_or_none()
    if user is None or not user.is_active or not verify_password(body.password, user.password_hash):
        raise AuthError("Invalid email or password")
    if user.totp_enabled:
        request.session[_TOTP_PENDING_UID] = str(user.id)
        request.session[_TOTP_PENDING_TS] = int(time.time())
        return LoginResult(totp_required=True)
    login_session(request, user)
    return LoginResult(id=str(user.id), email=user.email)


class TotpLoginRequest(BaseModel):
    code: str


@router.post("/login/totp", response_model=UserOut)
async def login_totp(
    body: TotpLoginRequest, request: Request, db: AsyncSession = Depends(get_session)
) -> UserOut:
    uid = request.session.get(_TOTP_PENDING_UID)
    ts = request.session.get(_TOTP_PENDING_TS)
    if not uid or not ts or (time.time() - int(ts)) > _TOTP_PENDING_TTL:
        request.session.pop(_TOTP_PENDING_UID, None)
        request.session.pop(_TOTP_PENDING_TS, None)
        raise AuthError("Your login session expired — start again")
    user = await db.get(User, uuid.UUID(uid))
    if user is None or not user.totp_enabled or not user.totp_secret_encrypted:
        raise AuthError("Invalid login")

    from portal.lib.config import get_settings
    from portal.lib.crypto import TokenCipher

    secret = TokenCipher(get_settings().token_encryption_key).decrypt(user.totp_secret_encrypted)
    ok = totp_svc.verify_code(secret, body.code)
    if not ok:
        remaining = totp_svc.consume_recovery_code(body.code, user.totp_recovery_codes or [])
        if remaining is not None:
            user.totp_recovery_codes = remaining
            ok = True
    if not ok:
        raise AuthError("Invalid code")

    request.session.pop(_TOTP_PENDING_UID, None)
    request.session.pop(_TOTP_PENDING_TS, None)
    login_session(request, user)
    await db.commit()
    return UserOut(id=str(user.id), email=user.email)


@router.post("/logout")
async def logout(request: Request) -> dict[str, bool]:
    clear_session(request)
    return {"ok": True}


@router.get("/me", response_model=UserOut)
async def me(user: User = Depends(require_admin)) -> UserOut:
    return UserOut(id=str(user.id), email=user.email)


# ── passkeys (WebAuthn) ─────────────────────────────────────────────────────────


async def _user_credentials(db: AsyncSession, user_id: uuid.UUID) -> list[WebauthnCredential]:
    return list(
        (
            await db.execute(
                select(WebauthnCredential).where(WebauthnCredential.user_id == user_id)
            )
        )
        .scalars()
        .all()
    )


@router.post("/passkeys/register/begin")
async def passkey_register_begin(
    request: Request,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> Any:
    existing = await _user_credentials(db, user.id)
    options_json, challenge = webauthn_svc.registration_options(
        user_id=user.id.bytes,
        user_name=user.email,
        exclude_ids=[c.credential_id for c in existing],
    )
    request.session[_REG_CHALLENGE] = challenge
    return json.loads(options_json)


class PasskeyRegisterComplete(BaseModel):
    credential: dict[str, Any]
    name: str | None = None


@router.post("/passkeys/register/complete")
async def passkey_register_complete(
    body: PasskeyRegisterComplete,
    request: Request,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, str]:
    challenge = request.session.pop(_REG_CHALLENGE, None)
    if not challenge:
        raise HTTPException(status_code=400, detail="No registration in progress")
    try:
        verified = webauthn_svc.verify_registration(
            credential=json.dumps(body.credential), challenge_b64=challenge
        )
    except Exception as exc:  # py_webauthn raises on any verification failure
        log.warning("passkey.register_failed", error=str(exc))
        raise HTTPException(status_code=400, detail="Passkey registration failed") from exc

    cred = WebauthnCredential(
        user_id=user.id,
        credential_id=webauthn_svc.to_b64url(verified.credential_id),
        public_key=webauthn_svc.to_b64url(verified.credential_public_key),
        sign_count=verified.sign_count,
        transports=body.credential.get("response", {}).get("transports"),
        name=(body.name or "").strip() or "Passkey",
    )
    db.add(cred)
    db.add(AuditLog(user_id=user.id, action="passkey.registered"))
    await db.commit()
    return {"id": str(cred.id)}


@router.post("/passkeys/login/begin")
async def passkey_login_begin(request: Request) -> Any:
    options_json, challenge = webauthn_svc.authentication_options()
    request.session[_AUTH_CHALLENGE] = challenge
    return json.loads(options_json)


class PasskeyLoginComplete(BaseModel):
    credential: dict[str, Any]


@router.post("/passkeys/login/complete", response_model=UserOut)
async def passkey_login_complete(
    body: PasskeyLoginComplete, request: Request, db: AsyncSession = Depends(get_session)
) -> UserOut:
    challenge = request.session.pop(_AUTH_CHALLENGE, None)
    if not challenge:
        raise AuthError("No login in progress")
    credential_id = body.credential.get("id")
    if not credential_id:
        raise AuthError("Malformed passkey response")
    cred = (
        await db.execute(
            select(WebauthnCredential).where(WebauthnCredential.credential_id == credential_id)
        )
    ).scalar_one_or_none()
    if cred is None:
        raise AuthError("Unknown passkey")
    try:
        verified = webauthn_svc.verify_authentication(
            credential=json.dumps(body.credential),
            challenge_b64=challenge,
            public_key_b64=cred.public_key,
            sign_count=cred.sign_count,
        )
    except Exception as exc:
        log.warning("passkey.login_failed", error=str(exc))
        raise AuthError("Passkey authentication failed") from exc

    user = await db.get(User, cred.user_id)
    if user is None or not user.is_active:
        raise AuthError("Account is not active")
    cred.sign_count = verified.new_sign_count
    cred.last_used_at = datetime.now(UTC)
    login_session(request, user)
    await db.commit()
    return UserOut(id=str(user.id), email=user.email)


class PasskeyInfo(BaseModel):
    id: str
    name: str | None
    created_at: str
    last_used_at: str | None


@router.get("/passkeys", response_model=list[PasskeyInfo])
async def list_passkeys(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[PasskeyInfo]:
    creds = await _user_credentials(db, user.id)
    creds.sort(key=lambda c: c.created_at)
    return [
        PasskeyInfo(
            id=str(c.id),
            name=c.name,
            created_at=c.created_at.isoformat(),
            last_used_at=c.last_used_at.isoformat() if c.last_used_at else None,
        )
        for c in creds
    ]


@router.delete("/passkeys/{passkey_id}", status_code=204)
async def delete_passkey(
    passkey_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> None:
    cred = await db.get(WebauthnCredential, passkey_id)
    if cred is not None and cred.user_id == user.id:
        await db.delete(cred)
        db.add(AuditLog(user_id=user.id, action="passkey.removed"))
        await db.commit()
