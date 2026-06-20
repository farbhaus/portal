"""Admin security + branding management: TOTP setup, security status, and the global logo upload.

Passkey registration/management lives in routes/auth.py (it's auth-flow adjacent); this module
holds the second-factor and branding admin endpoints. All require an authenticated admin.
"""

from datetime import UTC, datetime

from fastapi import APIRouter, Depends, File, HTTPException, UploadFile
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth import totp as totp_svc
from portal.auth.passwords import verify_password
from portal.auth.session import require_admin
from portal.db.models import AppSettings, AuditLog, User, WebauthnCredential
from portal.db.session import get_session
from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher
from portal.lib.logging import get_logger
from portal.services.app_settings import get_app_settings, get_or_create_app_settings

log = get_logger("routes.security")
router = APIRouter(prefix="/settings", tags=["security"])

PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def _cipher() -> TokenCipher:
    return TokenCipher(get_settings().token_encryption_key)


# ── security status ────────────────────────────────────────────────────────────


class PasskeyOut(BaseModel):
    id: str
    name: str | None
    created_at: str
    last_used_at: str | None


class SecurityStatus(BaseModel):
    totp_enabled: bool
    passkeys: list[PasskeyOut]


@router.get("/security", response_model=SecurityStatus)
async def security_status(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> SecurityStatus:
    rows = (
        await db.execute(
            select(WebauthnCredential)
            .where(WebauthnCredential.user_id == user.id)
            .order_by(WebauthnCredential.created_at)
        )
    ).scalars().all()
    return SecurityStatus(
        totp_enabled=user.totp_enabled,
        passkeys=[
            PasskeyOut(
                id=str(c.id),
                name=c.name,
                created_at=c.created_at.isoformat(),
                last_used_at=c.last_used_at.isoformat() if c.last_used_at else None,
            )
            for c in rows
        ],
    )


# ── TOTP setup ─────────────────────────────────────────────────────────────────


class TotpBeginOut(BaseModel):
    secret: str
    otpauth_uri: str
    qr_svg: str


@router.post("/totp/begin", response_model=TotpBeginOut)
async def totp_begin(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> TotpBeginOut:
    if user.totp_enabled:
        raise HTTPException(status_code=400, detail="Two-factor is already enabled")
    secret = totp_svc.generate_secret()
    user.totp_secret_encrypted = _cipher().encrypt(secret)  # stored now; enabled only after verify
    await db.commit()
    uri = totp_svc.provisioning_uri(secret, user.email)
    return TotpBeginOut(secret=secret, otpauth_uri=uri, qr_svg=totp_svc.qr_svg(uri))


class TotpEnable(BaseModel):
    code: str


class RecoveryCodesOut(BaseModel):
    recovery_codes: list[str]


@router.post("/totp/enable", response_model=RecoveryCodesOut)
async def totp_enable(
    body: TotpEnable,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> RecoveryCodesOut:
    if user.totp_enabled:
        raise HTTPException(status_code=400, detail="Two-factor is already enabled")
    if not user.totp_secret_encrypted:
        raise HTTPException(status_code=400, detail="Start two-factor setup first")
    secret = _cipher().decrypt(user.totp_secret_encrypted)
    if not totp_svc.verify_code(secret, body.code):
        raise HTTPException(status_code=400, detail="That code didn't match — try again")
    codes, hashes = totp_svc.generate_recovery_codes()
    user.totp_enabled = True
    user.totp_recovery_codes = hashes
    db.add(AuditLog(user_id=user.id, action="totp.enabled"))
    await db.commit()
    log.info("security.totp_enabled", user_id=str(user.id))
    return RecoveryCodesOut(recovery_codes=codes)


class TotpDisable(BaseModel):
    password: str | None = None
    code: str | None = None


@router.post("/totp/disable")
async def totp_disable(
    body: TotpDisable,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, bool]:
    if not user.totp_enabled:
        return {"ok": True}
    ok = bool(body.password and verify_password(body.password, user.password_hash))
    if not ok and body.code and user.totp_secret_encrypted:
        ok = totp_svc.verify_code(_cipher().decrypt(user.totp_secret_encrypted), body.code)
    if not ok:
        raise HTTPException(status_code=403, detail="Provide your current password or a valid code")
    user.totp_enabled = False
    user.totp_secret_encrypted = None
    user.totp_recovery_codes = None
    db.add(AuditLog(user_id=user.id, action="totp.disabled"))
    await db.commit()
    log.info("security.totp_disabled", user_id=str(user.id))
    return {"ok": True}


# ── branding (global logo + brand defaults) ─────────────────────────────────────


class BrandingStatus(BaseModel):
    has_logo: bool
    logo_updated_at: str | None
    brand_display_name: str | None
    brand_accent_color: str | None


def _branding_status(row: AppSettings | None) -> BrandingStatus:
    return BrandingStatus(
        has_logo=bool(row and row.logo_png),
        logo_updated_at=row.logo_updated_at.isoformat() if row and row.logo_updated_at else None,
        brand_display_name=row.brand_display_name if row else None,
        brand_accent_color=row.brand_accent_color if row else None,
    )


@router.get("/branding", response_model=BrandingStatus)
async def branding_status(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> BrandingStatus:
    return _branding_status(await get_app_settings(db))


@router.post("/branding/logo", response_model=BrandingStatus)
async def upload_logo(
    file: UploadFile = File(...),
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> BrandingStatus:
    data = await file.read()
    max_bytes = get_settings().branding_logo_max_bytes
    if len(data) > max_bytes:
        raise HTTPException(
            status_code=400, detail=f"Logo exceeds the maximum size ({max_bytes // 1024} KB)"
        )
    if not data.startswith(PNG_MAGIC):
        raise HTTPException(status_code=400, detail="Logo must be a PNG file")
    row = await get_or_create_app_settings(db)
    row.logo_png = data
    row.logo_content_type = "image/png"
    row.logo_updated_at = datetime.now(UTC)
    db.add(AuditLog(user_id=user.id, action="branding.logo_updated"))
    await db.commit()
    log.info("security.branding_logo_updated", user_id=str(user.id), bytes=len(data))
    return _branding_status(row)


@router.delete("/branding/logo", response_model=BrandingStatus)
async def delete_logo(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> BrandingStatus:
    row = await get_app_settings(db)
    if row is not None:
        row.logo_png = None
        row.logo_content_type = None
        row.logo_updated_at = None
        db.add(AuditLog(user_id=user.id, action="branding.logo_removed"))
        await db.commit()
    return _branding_status(row)


class BrandingUpdate(BaseModel):
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_accent_color: str | None = Field(default=None, max_length=32)


@router.post("/branding", response_model=BrandingStatus)
async def update_branding(
    body: BrandingUpdate,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> BrandingStatus:
    row = await get_or_create_app_settings(db)
    row.brand_display_name = (body.brand_display_name or "").strip() or None
    row.brand_accent_color = (body.brand_accent_color or "").strip() or None
    await db.commit()
    return _branding_status(row)
