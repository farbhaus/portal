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
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.passwords import hash_password
from portal.auth.session import require_admin
from portal.db.models import (
    AuditLog,
    DownloadEvent,
    DownloadLink,
    DownloadSession,
    User,
)
from portal.db.session import get_session
from portal.downloads.links import resolve_source, validate_source
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
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
    brand_display_name: str | None = Field(default=None, max_length=255)
    brand_subtitle: str | None = None


class DownloadLinkUpdate(BaseModel):
    source: dict[str, Any] | None = None
    expires_at: datetime | None = None
    password: str | None = None
    max_downloads: int | None = Field(default=None, ge=1)
    viewer_fields_required: ViewerFields | None = None
    verify_email: bool | None = None
    allow_preview: bool | None = None
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
    brand_display_name: str | None
    brand_subtitle: str | None
    revoked_at: str | None
    created_at: str


def _public_url(token: str) -> str:
    return f"{get_settings().base_url.rstrip('/')}/d/{token}"


def _generic_label(source: dict[str, Any]) -> str:
    """Type-only summary used when Frame.io can't be reached to resolve names."""
    stype = source.get("type")
    if stype == "file":
        return "Single file"
    if stype == "folder":
        return "Folder (recursive)" if source.get("recursive") else "Folder"
    if stype == "selection":
        return f"{len(source.get('file_ids') or [])} files"
    return "—"


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
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadLinkOut:
    link = await _get_or_404(db, link_id)
    fields = body.model_dump(exclude_unset=True)
    # The source (which Frame.io file/folder/selection this link serves) can be re-pointed on an
    # existing link — the token/URL stays the same, so a shared link can be corrected or reused for
    # a new delivery. Validate like creation and record who changed it.
    new_source = fields.pop("source", None)
    if new_source is not None and new_source != link.source:
        try:
            validate_source(new_source)
        except ValueError as exc:
            raise BadRequestError(str(exc)) from exc
        db.add(
            AuditLog(
                user_id=user.id,
                action="download_link.source_changed",
                detail={
                    "download_link_id": str(link_id),
                    "from_type": (link.source or {}).get("type"),
                    "to_type": new_source.get("type"),
                },
            )
        )
        link.source = new_source
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


class DownloadEventOut(BaseModel):
    frameio_file_id: str
    file_name: str | None
    viewer_name: str | None
    viewer_email: str | None
    ip: str | None
    completed: bool
    started_at: str


class DownloadStats(BaseModel):
    downloads: int
    unique_viewers: int
    last_activity: str | None


@router.get("/{link_id}/events", response_model=list[DownloadEventOut])
async def list_events(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    limit: int = 100,
) -> list[DownloadEventOut]:
    await _get_or_404(db, link_id)
    limit = max(1, min(limit, 500))
    result = await db.execute(
        select(DownloadEvent, DownloadSession)
        .join(DownloadSession, DownloadEvent.download_session_id == DownloadSession.id)
        .where(DownloadSession.download_link_id == link_id)
        .order_by(DownloadEvent.started_at.desc())
        .limit(limit)
    )
    return [
        DownloadEventOut(
            frameio_file_id=ev.frameio_file_id,
            file_name=ev.file_name,
            viewer_name=sess.viewer_name,
            viewer_email=sess.viewer_email,
            ip=sess.ip,
            completed=ev.completed,
            started_at=ev.started_at.isoformat(),
        )
        for ev, sess in result.all()
    ]


@router.get("/{link_id}/stats", response_model=DownloadStats)
async def stats(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DownloadStats:
    await _get_or_404(db, link_id)
    sessions = select(DownloadSession.id).where(DownloadSession.download_link_id == link_id)
    downloads = (
        await db.scalar(select(func.count()).where(DownloadEvent.download_session_id.in_(sessions)))
        or 0
    )
    viewer_key = func.coalesce(DownloadSession.viewer_email, DownloadSession.ip)
    unique_viewers = (
        await db.scalar(
            select(func.count(func.distinct(viewer_key))).where(
                DownloadSession.download_link_id == link_id
            )
        )
        or 0
    )
    last = await db.scalar(
        select(func.max(DownloadSession.started_at)).where(
            DownloadSession.download_link_id == link_id
        )
    )
    return DownloadStats(
        downloads=downloads,
        unique_viewers=unique_viewers,
        last_activity=last.isoformat() if last else None,
    )


class PathItem(BaseModel):
    id: str
    name: str


class SourceInfo(BaseModel):
    type: str
    label: str  # human summary (folder path / file name when resolved, else a type-only fallback)
    account_id: str | None = None
    workspace_id: str | None = (
        None  # resolved from the folder's project, to pre-select the explorer
    )
    project_id: str | None = None
    project_name: str | None = None
    folder_id: str | None = None
    folder_name: str | None = None  # leaf folder name
    folder_path: str | None = None  # full path string, top-most ancestor → shared folder
    path: list[PathItem] | None = None  # breadcrumb items for reopening the explorer in place
    recursive: bool | None = None
    files: list[str] | None = None
    resolved: bool  # False when Frame.io couldn't be reached — label falls back to the generic form


@router.get("/{link_id}/source", response_model=SourceInfo)
async def source_info(
    link_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> SourceInfo:
    """Resolve a link's Frame.io source to human-readable names for the editor.

    Best-effort: on any Frame.io error it returns a type-only label with resolved=False so the
    edit page still renders.
    """
    link = await _get_or_404(db, link_id)
    source = link.source or {}
    stype = str(source.get("type") or "")
    account_id = str(source.get("account_id") or "")
    client = get_frameio_client()
    try:
        if stype == "folder":
            folder_id = str(source["folder_id"])
            folder = await client.get_folder(account_id, folder_id)
            ancestry = await client.folder_ancestry(account_id, folder_id)
            project = (
                await client.get_project(account_id, folder.project_id)
                if folder.project_id
                else None
            )
            path = [PathItem(id=i.id, name=i.name) for i in ancestry]
            # The top-most ancestor is the project's root folder; label it like the create flow.
            if project and project.root_folder_id and path and path[0].id == project.root_folder_id:
                path[0] = PathItem(id=path[0].id, name=f"{project.name} (root)")
            names = [i.name for i in path] or [folder_id]
            full = "/".join(names)
            return SourceInfo(
                type="folder",
                label=full,
                account_id=account_id,
                workspace_id=project.workspace_id if project else None,
                project_id=folder.project_id,
                project_name=project.name if project else None,
                folder_id=folder_id,
                folder_name=(ancestry[-1].name if ancestry else folder_id),
                folder_path=full,
                path=path,
                recursive=bool(source.get("recursive")),
                resolved=True,
            )
        if stype == "file":
            f = await client.get_file(account_id, str(source["file_id"]))
            return SourceInfo(type="file", label=f.name, files=[f.name], resolved=True)
        if stype == "selection":
            resolved = await resolve_source(client, source)
            names = [f.name for f in resolved.files]
            return SourceInfo(
                type="selection", label=f"{len(names)} files", files=names, resolved=True
            )
    except (FrameioNotConnected, FrameioError) as exc:
        log.warning(
            "download_link.source_info_failed", download_link_id=str(link_id), error=str(exc)
        )
    return SourceInfo(type=stype or "unknown", label=_generic_label(source), resolved=False)
