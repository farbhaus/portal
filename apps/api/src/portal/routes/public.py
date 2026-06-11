"""Public (unauthenticated) upload-link endpoints.

These back the branded uploader page at /u/{token}. They expose only what an anonymous
uploader needs — branding and upload constraints — and never Frame.io internals (account /
workspace / folder ids) or stack traces. The uploader page and the actual upload mechanism
land in build step 6; this is the token-resolution and password-gate core.
"""

import uuid
from datetime import UTC, datetime

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from portal.auth.passwords import verify_password
from portal.db.models import Destination, UploadLink, UploadSession
from portal.db.session import get_session
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
from portal.lib.errors import NotFoundError
from portal.lib.logging import get_logger
from portal.notify.email import UploadedFile, send_upload_completion
from portal.storage.base import DestinationConfig, UploadNotReady
from portal.storage.base import UploadSession as StorageUploadSession
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.sync.events import trigger_sync_for_upload
from portal.uploads.links import link_state, merged_branding

log = get_logger("routes.public")
router = APIRouter(prefix="/public/links", tags=["public"])


class PublicLinkOut(BaseModel):
    state: str  # "ok" | "expired" | "revoked"
    display_name: str
    subtitle: str | None
    logo_url: str | None
    accent_color: str | None
    password_required: bool
    # Only meaningful when state == "ok"; still returned so the page can render constraints.
    max_file_size: int | None
    allowed_extensions: list[str] | None
    uploader_fields_required: dict[str, bool]


class PasswordVerifyRequest(BaseModel):
    password: str


class PasswordVerifyResult(BaseModel):
    ok: bool


async def _get_link(db: AsyncSession, token: str) -> UploadLink:
    result = await db.execute(
        select(UploadLink)
        .where(UploadLink.token == token)
        .options(selectinload(UploadLink.destination))
    )
    link = result.scalar_one_or_none()
    if link is None:
        # Same response as any unknown token — don't reveal whether a token ever existed.
        raise NotFoundError("This upload link does not exist")
    return link


@router.get("/{token}", response_model=PublicLinkOut)
async def resolve_link(token: str, db: AsyncSession = Depends(get_session)) -> PublicLinkOut:
    link = await _get_link(db, token)
    branding = merged_branding(link, link.destination)
    fields = link.uploader_fields_required or {}
    return PublicLinkOut(
        state=link_state(link),
        display_name=branding["display_name"] or "Upload",
        subtitle=branding["subtitle"],
        logo_url=branding["logo_url"],
        accent_color=branding["accent_color"],
        password_required=link.password_hash is not None,
        max_file_size=link.max_file_size,
        allowed_extensions=link.allowed_extensions,
        uploader_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
            "message": bool(fields.get("message")),
        },
    )


@router.post("/{token}/verify-password", response_model=PasswordVerifyResult)
async def verify_link_password(
    token: str, body: PasswordVerifyRequest, db: AsyncSession = Depends(get_session)
) -> PasswordVerifyResult:
    link = await _get_link(db, token)
    # An open (no-password) link verifies trivially; a usable link must also be in "ok" state.
    if link.password_hash is None:
        return PasswordVerifyResult(ok=link_state(link) == "ok")
    ok = link_state(link) == "ok" and verify_password(body.password, link.password_hash)
    return PasswordVerifyResult(ok=ok)


# ── uploader session lifecycle ────────────────────────────────────────────────


class StartSessionRequest(BaseModel):
    name: str | None = None
    email: str | None = None
    message: str | None = None
    password: str | None = None


class StartSessionResult(BaseModel):
    session_id: str


class FileRequest(BaseModel):
    # Relative path including any folders to preserve, e.g. "A-CAM/clip001.mov".
    path: str = Field(min_length=1)
    size: int = Field(ge=1)


class ChunkOut(BaseModel):
    part: int
    size: int
    url: str


class FileResult(BaseModel):
    file_id: str
    chunks: list[ChunkOut]
    headers: dict[str, str]


class CompleteFileResult(BaseModel):
    complete: bool
    failed: bool


class CompleteSessionRequest(BaseModel):
    total_bytes: int | None = None


def _require_usable(link: UploadLink) -> None:
    state = link_state(link)
    if state != "ok":
        # 410 Gone: the link is real but no longer accepting uploads.
        raise HTTPException(status_code=410, detail=f"This link is {state}")


def _ext_allowed(path: str, allowed: list[str] | None) -> bool:
    if not allowed:
        return True
    ext = path.rsplit(".", 1)[-1].lower() if "." in path else ""
    return ext in allowed


def _destination_config(destination: Destination) -> DestinationConfig:
    config = dict(destination.config)
    return DestinationConfig(type=str(config.get("type", "frameio")), data=config)


async def _load_session(db: AsyncSession, link: UploadLink, session_id: str) -> UploadSession:
    try:
        sid = uuid.UUID(session_id)
    except ValueError as exc:
        raise NotFoundError("Upload session not found") from exc
    session = await db.get(UploadSession, sid)
    if session is None or session.upload_link_id != link.id:
        raise NotFoundError("Upload session not found")
    return session


@router.post("/{token}/sessions", response_model=StartSessionResult)
async def start_session(
    token: str, body: StartSessionRequest, db: AsyncSession = Depends(get_session)
) -> StartSessionResult:
    link = await _get_link(db, token)
    _require_usable(link)
    if link.password_hash is not None and not verify_password(
        body.password or "", link.password_hash
    ):
        raise HTTPException(status_code=403, detail="Incorrect password")

    required = link.uploader_fields_required or {}
    provided = {"name": body.name, "email": body.email, "message": body.message}
    missing = [f for f in ("name", "email", "message") if required.get(f) and not provided[f]]
    if missing:
        raise HTTPException(
            status_code=422, detail=f"Missing required field(s): {', '.join(missing)}"
        )

    session = UploadSession(
        upload_link_id=link.id,
        uploader_name=body.name,
        uploader_email=body.email,
        uploader_message=body.message,
        status="in_progress",
        started_at=datetime.now(UTC),
    )
    db.add(session)
    await db.commit()
    await db.refresh(session)
    return StartSessionResult(session_id=str(session.id))


@router.post("/{token}/sessions/{session_id}/files", response_model=FileResult)
async def request_file_upload(
    token: str,
    session_id: str,
    body: FileRequest,
    db: AsyncSession = Depends(get_session),
) -> FileResult:
    link = await _get_link(db, token)
    _require_usable(link)
    session = await _load_session(db, link, session_id)

    if not _ext_allowed(body.path, link.allowed_extensions):
        raise HTTPException(status_code=400, detail="This file type is not allowed")
    if link.max_file_size is not None and body.size > link.max_file_size:
        raise HTTPException(status_code=400, detail="File exceeds the maximum allowed size")

    backend = FrameioStorageBackend(client=get_frameio_client())
    try:
        upload = await backend.create_upload_session(
            _destination_config(link.destination), filename=body.path, size=body.size
        )
        creds = await backend.get_upload_credentials(upload)
    except FrameioNotConnected as exc:
        raise HTTPException(status_code=503, detail="Uploads are temporarily unavailable") from exc
    except FrameioError as exc:
        log.warning("public.upload.create_failed", error=str(exc))
        raise HTTPException(status_code=502, detail="Could not start the upload") from exc

    # Store per-file metadata (not just ids) so the completion email can list names + sizes
    # without extra Frame.io calls.
    entry = {"file_id": upload.backend_data["file_id"], "name": body.path, "size": body.size}
    session.frameio_asset_ids = (session.frameio_asset_ids or []) + [entry]
    await db.commit()

    return FileResult(
        file_id=str(upload.backend_data["file_id"]),
        chunks=[ChunkOut(part=c.part, size=c.size, url=c.url) for c in creds.chunks],
        headers=creds.headers,
    )


@router.post(
    "/{token}/sessions/{session_id}/files/{file_id}/complete", response_model=CompleteFileResult
)
async def complete_file(
    token: str,
    session_id: str,
    file_id: str,
    db: AsyncSession = Depends(get_session),
) -> CompleteFileResult:
    link = await _get_link(db, token)
    await _load_session(db, link, session_id)
    backend = FrameioStorageBackend(client=get_frameio_client())
    # complete_upload only needs the destination + file id to poll Frame.io's assembly status.
    storage_session = StorageUploadSession(
        id=file_id,
        destination=_destination_config(link.destination),
        filename="",
        size=0,
        backend_data={"file_id": file_id},
    )
    try:
        await backend.complete_upload(storage_session)
        return CompleteFileResult(complete=True, failed=False)
    except UploadNotReady:
        return CompleteFileResult(complete=False, failed=False)
    except FrameioError:
        return CompleteFileResult(complete=False, failed=True)


@router.post("/{token}/sessions/{session_id}/complete", response_model=PasswordVerifyResult)
async def complete_session(
    token: str,
    session_id: str,
    body: CompleteSessionRequest,
    background: BackgroundTasks,
    db: AsyncSession = Depends(get_session),
) -> PasswordVerifyResult:
    link = await _get_link(db, token)
    session = await _load_session(db, link, session_id)
    now = datetime.now(UTC)
    session.status = "completed"
    session.completed_at = now
    session.total_bytes = body.total_bytes

    # Snapshot what the notification + sync trigger need BEFORE commit (attrs expire on commit).
    entries = session.frameio_asset_ids or []
    files = [UploadedFile(name=str(e.get("name", "")), size=e.get("size")) for e in entries]
    total_bytes = body.total_bytes or sum(f.size or 0 for f in files)
    started_at = session.started_at
    duration = (now - started_at).total_seconds() if started_at else 0.0
    link_name = link.brand_display_name or link.destination.display_name
    uploader = (session.uploader_name, session.uploader_email, session.uploader_message)
    dest_cfg = dict(link.destination.config)
    sync_account = str(dest_cfg.get("account_id") or "")
    sync_folder = str(dest_cfg.get("folder_id") or "")
    uploaded_file_ids = [str(e["file_id"]) for e in entries if e.get("file_id")]

    await db.commit()
    log.info("public.upload.session_completed", session_id=session_id)

    background.add_task(
        send_upload_completion,
        link_name=link_name,
        uploader_name=uploader[0],
        uploader_email=uploader[1],
        uploader_message=uploader[2],
        files=files,
        total_bytes=total_bytes,
        duration_seconds=duration,
    )
    # Portal's own sync trigger: mirror client-uploaded files to any matching sync rule's NAS
    # destination immediately, instead of waiting on a Frame.io webhook (which self-serve plans
    # don't deliver) or the reconcile cron.
    if sync_account and sync_folder and uploaded_file_ids:
        background.add_task(
            trigger_sync_for_upload, sync_account, sync_folder, uploaded_file_ids
        )
    return PasswordVerifyResult(ok=True)
