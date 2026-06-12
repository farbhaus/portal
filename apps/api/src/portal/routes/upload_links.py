"""Admin CRUD for upload links.

A link is a branded, shareable token bound to a destination, with optional gating: expiry,
password, max file size, allowed extensions, and which uploader fields are required. The public
side (token resolution, uploader page, the actual upload) is in routes/public.py and build
step 6. Branding can be overridden per link, otherwise it falls back to the destination's.
"""

import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Depends
from pydantic import BaseModel, Field
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from portal.auth.passwords import hash_password
from portal.auth.session import require_admin
from portal.db.models import AuditLog, Destination, UploadLink, UploadSession, User
from portal.db.session import get_session
from portal.lib.config import get_settings
from portal.lib.errors import NotFoundError, PortalError
from portal.lib.logging import get_logger
from portal.uploads.links import (
    MEDIA_EXTENSIONS_PRESET,
    generate_token,
    link_state,
    normalize_extensions,
)

log = get_logger("routes.upload_links")
router = APIRouter(prefix="/upload-links", tags=["upload-links"])


class BadRequestError(PortalError):
    status_code = 400
    public_message = "Bad request"


class UploaderFields(BaseModel):
    name: bool = False
    email: bool = False
    message: bool = False


class UploadLinkCreate(BaseModel):
    destination_id: uuid.UUID
    expires_at: datetime | None = None
    password: str | None = None
    max_file_size: int | None = Field(default=None, ge=1)
    allowed_extensions: list[str] | None = None
    uploader_fields_required: UploaderFields = UploaderFields()
    verify_email: bool = False
    brand_logo_url: str | None = None
    brand_accent_color: str | None = Field(default=None, max_length=32)
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_subtitle: str | None = None


class UploadLinkUpdate(BaseModel):
    destination_id: uuid.UUID | None = None
    expires_at: datetime | None = None
    # "" or null clears the password; a non-empty value sets it; omitting leaves it unchanged.
    password: str | None = None
    max_file_size: int | None = Field(default=None, ge=1)
    allowed_extensions: list[str] | None = None
    uploader_fields_required: UploaderFields | None = None
    verify_email: bool | None = None
    brand_logo_url: str | None = None
    brand_accent_color: str | None = Field(default=None, max_length=32)
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_subtitle: str | None = None


class UploadLinkOut(BaseModel):
    id: str
    token: str
    public_url: str
    state: str
    destination_id: str
    destination_name: str
    expires_at: str | None
    password_protected: bool
    max_file_size: int | None
    allowed_extensions: list[str] | None
    uploader_fields_required: dict[str, bool]
    verify_email: bool
    brand_logo_url: str | None
    brand_accent_color: str | None
    brand_display_name: str | None
    brand_subtitle: str | None
    revoked_at: str | None
    created_at: str


def _public_url(token: str) -> str:
    return f"{get_settings().base_url.rstrip('/')}/u/{token}"


def _with_email_forced(fields: dict[str, Any], verify_email: bool) -> dict[str, Any]:
    """Verification implies the email field is required."""
    fields = dict(fields)
    if verify_email:
        fields["email"] = True
    return fields


def _to_out(link: UploadLink) -> UploadLinkOut:
    fields = link.uploader_fields_required or {}
    return UploadLinkOut(
        id=str(link.id),
        token=link.token,
        public_url=_public_url(link.token),
        state=link_state(link),
        destination_id=str(link.destination_id),
        destination_name=link.destination.display_name,
        expires_at=link.expires_at.isoformat() if link.expires_at else None,
        password_protected=link.password_hash is not None,
        max_file_size=link.max_file_size,
        allowed_extensions=link.allowed_extensions,
        uploader_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
            "message": bool(fields.get("message")),
        },
        verify_email=link.verify_email,
        brand_logo_url=link.brand_logo_url,
        brand_accent_color=link.brand_accent_color,
        brand_display_name=link.brand_display_name,
        brand_subtitle=link.brand_subtitle,
        revoked_at=link.revoked_at.isoformat() if link.revoked_at else None,
        created_at=link.created_at.isoformat(),
    )


async def _get_or_404(db: AsyncSession, link_id: uuid.UUID) -> UploadLink:
    result = await db.execute(
        select(UploadLink)
        .where(UploadLink.id == link_id)
        .options(selectinload(UploadLink.destination))
    )
    link = result.scalar_one_or_none()
    if link is None:
        raise NotFoundError("Upload link not found")
    return link


@router.get("/extension-preset", response_model=list[str])
async def extension_preset(_: User = Depends(require_admin)) -> list[str]:
    return MEDIA_EXTENSIONS_PRESET


@router.get("", response_model=list[UploadLinkOut])
async def list_links(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[UploadLinkOut]:
    result = await db.execute(
        select(UploadLink)
        .options(selectinload(UploadLink.destination))
        .order_by(UploadLink.created_at.desc())
    )
    return [_to_out(link) for link in result.scalars().all()]


@router.post("", response_model=UploadLinkOut, status_code=201)
async def create_link(
    body: UploadLinkCreate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> UploadLinkOut:
    destination = await db.get(Destination, body.destination_id)
    if destination is None:
        raise BadRequestError("Destination not found")

    link = UploadLink(
        token=generate_token(),
        destination_id=body.destination_id,
        expires_at=body.expires_at,
        password_hash=hash_password(body.password) if body.password else None,
        max_file_size=body.max_file_size,
        allowed_extensions=normalize_extensions(body.allowed_extensions),
        uploader_fields_required=_with_email_forced(
            body.uploader_fields_required.model_dump(), body.verify_email
        ),
        verify_email=body.verify_email,
        brand_logo_url=body.brand_logo_url,
        brand_accent_color=body.brand_accent_color,
        brand_display_name=body.brand_display_name,
        brand_subtitle=body.brand_subtitle,
    )
    db.add(link)
    await db.flush()
    db.add(
        AuditLog(
            user_id=user.id,
            action="upload_link.created",
            detail={"upload_link_id": str(link.id), "destination_id": str(body.destination_id)},
        )
    )
    await db.commit()
    link = await _get_or_404(db, link.id)
    log.info("upload_link.created", upload_link_id=str(link.id))
    return _to_out(link)


@router.get("/{link_id}", response_model=UploadLinkOut)
async def get_link(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> UploadLinkOut:
    return _to_out(await _get_or_404(db, link_id))


@router.patch("/{link_id}", response_model=UploadLinkOut)
async def update_link(
    link_id: uuid.UUID,
    body: UploadLinkUpdate,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> UploadLinkOut:
    link = await _get_or_404(db, link_id)
    fields = body.model_dump(exclude_unset=True)

    if "destination_id" in fields:
        dest_id = fields.pop("destination_id")
        if await db.get(Destination, dest_id) is None:
            raise BadRequestError("Destination not found")
        link.destination_id = dest_id
    if "password" in fields:
        pw = fields.pop("password")
        link.password_hash = hash_password(pw) if pw else None
    if "allowed_extensions" in fields:
        link.allowed_extensions = normalize_extensions(fields.pop("allowed_extensions"))
    if "uploader_fields_required" in fields:
        ufr: dict[str, Any] | None = fields.pop("uploader_fields_required")
        link.uploader_fields_required = ufr

    for key, value in fields.items():
        setattr(link, key, value)
    if link.verify_email:  # verification implies the email field is required
        link.uploader_fields_required = _with_email_forced(
            link.uploader_fields_required or {}, True
        )

    await db.commit()
    link = await _get_or_404(db, link_id)
    return _to_out(link)


@router.post("/{link_id}/revoke", response_model=UploadLinkOut)
async def revoke_link(
    link_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> UploadLinkOut:
    link = await _get_or_404(db, link_id)
    if link.revoked_at is None:
        link.revoked_at = datetime.now(UTC)
        db.add(
            AuditLog(
                user_id=user.id,
                action="upload_link.revoked",
                detail={"upload_link_id": str(link_id)},
            )
        )
        await db.commit()
        link = await _get_or_404(db, link_id)
    return _to_out(link)


@router.delete("/{link_id}", status_code=204)
async def delete_link(
    link_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> None:
    link = await _get_or_404(db, link_id)
    await db.delete(link)
    db.add(
        AuditLog(
            user_id=user.id, action="upload_link.deleted", detail={"upload_link_id": str(link_id)}
        )
    )
    await db.commit()


class UploadStats(BaseModel):
    uploads: int  # completed sessions
    total_bytes: int
    last_activity: str | None


@router.get("/{link_id}/stats", response_model=UploadStats)
async def stats(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> UploadStats:
    await _get_or_404(db, link_id)
    completed = (UploadSession.upload_link_id == link_id) & (UploadSession.status == "completed")
    uploads = await db.scalar(select(func.count()).where(completed)) or 0
    total_bytes = await db.scalar(
        select(func.coalesce(func.sum(UploadSession.total_bytes), 0)).where(completed)
    ) or 0
    last = await db.scalar(
        select(func.max(UploadSession.completed_at)).where(
            UploadSession.upload_link_id == link_id
        )
    )
    return UploadStats(
        uploads=uploads,
        total_bytes=int(total_bytes),
        last_activity=last.isoformat() if last else None,
    )
