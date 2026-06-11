"""Admin CRUD for download links.

A download link is a branded, shareable token that lets recipients (no Frame.io account) fetch
files out of Frame.io — a single file, a folder, or a curated selection — via short-lived signed
URLs. The recipient page and signed-URL minting live in routes/public.py (build step 9 for the
page). Lifecycle gates: expiry, password, revocation, and a total download cap.
"""

import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Depends
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.passwords import hash_password
from portal.auth.session import require_admin
from portal.db.models import AuditLog, DownloadLink, User
from portal.db.session import get_session
from portal.downloads.links import validate_source
from portal.lib.config import get_settings
from portal.lib.errors import NotFoundError, PortalError
from portal.lib.logging import get_logger
from portal.uploads.links import generate_token, link_state

log = get_logger("routes.download_links")
router = APIRouter(prefix="/download-links", tags=["download-links"])


class BadRequestError(PortalError):
    status_code = 400
    public_message = "Bad request"


class ViewerFields(BaseModel):
    name: bool = False
    email: bool = False


class DownloadLinkCreate(BaseModel):
    source: dict[str, Any]
    expires_at: datetime | None = None
    password: str | None = None
    max_downloads: int | None = Field(default=None, ge=1)
    viewer_fields_required: ViewerFields = ViewerFields()
    verify_email: bool = False
    allow_preview: bool = True
    brand_logo_url: str | None = None
    brand_accent_color: str | None = Field(default=None, max_length=32)
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_subtitle: str | None = None


class DownloadLinkUpdate(BaseModel):
    expires_at: datetime | None = None
    password: str | None = None
    max_downloads: int | None = Field(default=None, ge=1)
    viewer_fields_required: ViewerFields | None = None
    verify_email: bool | None = None
    allow_preview: bool | None = None
    brand_logo_url: str | None = None
    brand_accent_color: str | None = Field(default=None, max_length=32)
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_subtitle: str | None = None


class DownloadLinkOut(BaseModel):
    id: str
    token: str
    public_url: str
    state: str
    source: dict[str, Any]
    expires_at: str | None
    password_protected: bool
    max_downloads: int | None
    downloads_count: int
    viewer_fields_required: dict[str, bool]
    verify_email: bool
    allow_preview: bool
    brand_logo_url: str | None
    brand_accent_color: str | None
    brand_display_name: str | None
    brand_subtitle: str | None
    revoked_at: str | None
    created_at: str


def _public_url(token: str) -> str:
    return f"{get_settings().base_url.rstrip('/')}/d/{token}"


def _to_out(link: DownloadLink) -> DownloadLinkOut:
    fields = link.viewer_fields_required or {}
    return DownloadLinkOut(
        id=str(link.id),
        token=link.token,
        public_url=_public_url(link.token),
        state=link_state(link),
        source=link.source,
        expires_at=link.expires_at.isoformat() if link.expires_at else None,
        password_protected=link.password_hash is not None,
        max_downloads=link.max_downloads,
        downloads_count=link.downloads_count,
        viewer_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
        },
        verify_email=link.verify_email,
        allow_preview=link.allow_preview,
        brand_logo_url=link.brand_logo_url,
        brand_accent_color=link.brand_accent_color,
        brand_display_name=link.brand_display_name,
        brand_subtitle=link.brand_subtitle,
        revoked_at=link.revoked_at.isoformat() if link.revoked_at else None,
        created_at=link.created_at.isoformat(),
    )


async def _get_or_404(db: AsyncSession, link_id: uuid.UUID) -> DownloadLink:
    link = await db.get(DownloadLink, link_id)
    if link is None:
        raise NotFoundError("Download link not found")
    return link


@router.get("", response_model=list[DownloadLinkOut])
async def list_links(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[DownloadLinkOut]:
    result = await db.execute(select(DownloadLink).order_by(DownloadLink.created_at.desc()))
    return [_to_out(link) for link in result.scalars().all()]


@router.post("", response_model=DownloadLinkOut, status_code=201)
async def create_link(
    body: DownloadLinkCreate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadLinkOut:
    try:
        validate_source(body.source)
    except ValueError as exc:
        raise BadRequestError(str(exc)) from exc

    fields = body.viewer_fields_required.model_dump()
    if body.verify_email:
        fields["email"] = True  # verification implies the email field is required
    link = DownloadLink(
        token=generate_token(),
        source=body.source,
        expires_at=body.expires_at,
        password_hash=hash_password(body.password) if body.password else None,
        max_downloads=body.max_downloads,
        viewer_fields_required=fields,
        verify_email=body.verify_email,
        allow_preview=body.allow_preview,
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
            action="download_link.created",
            detail={"download_link_id": str(link.id), "source_type": body.source.get("type")},
        )
    )
    await db.commit()
    await db.refresh(link)
    log.info("download_link.created", download_link_id=str(link.id))
    return _to_out(link)


@router.get("/{link_id}", response_model=DownloadLinkOut)
async def get_link(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadLinkOut:
    return _to_out(await _get_or_404(db, link_id))


@router.patch("/{link_id}", response_model=DownloadLinkOut)
async def update_link(
    link_id: uuid.UUID,
    body: DownloadLinkUpdate,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadLinkOut:
    link = await _get_or_404(db, link_id)
    fields = body.model_dump(exclude_unset=True)
    if "password" in fields:
        pw = fields.pop("password")
        link.password_hash = hash_password(pw) if pw else None
    if "viewer_fields_required" in fields:
        link.viewer_fields_required = fields.pop("viewer_fields_required")
    for key, value in fields.items():
        setattr(link, key, value)
    if link.verify_email:  # verification implies the email field is required
        vfr = dict(link.viewer_fields_required or {})
        if not vfr.get("email"):
            vfr["email"] = True
            link.viewer_fields_required = vfr
    await db.commit()
    await db.refresh(link)
    return _to_out(link)


@router.post("/{link_id}/revoke", response_model=DownloadLinkOut)
async def revoke_link(
    link_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadLinkOut:
    link = await _get_or_404(db, link_id)
    if link.revoked_at is None:
        link.revoked_at = datetime.now(UTC)
        db.add(
            AuditLog(
                user_id=user.id,
                action="download_link.revoked",
                detail={"download_link_id": str(link_id)},
            )
        )
        await db.commit()
        await db.refresh(link)
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
            user_id=user.id,
            action="download_link.deleted",
            detail={"download_link_id": str(link_id)},
        )
    )
    await db.commit()
