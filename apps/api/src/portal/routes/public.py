"""Public (unauthenticated) upload-link endpoints.

These back the branded uploader page at /u/{token}. They expose only what an anonymous
uploader needs — branding and upload constraints — and never Frame.io internals (account /
workspace / folder ids) or stack traces. The uploader page and the actual upload mechanism
land in build step 6; this is the token-resolution and password-gate core.
"""

import uuid
from datetime import UTC, datetime

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException, Request, Response
from pydantic import BaseModel, Field
from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from portal import verify
from portal.auth.passwords import verify_password
from portal.db.models import Destination, UploadLink, UploadSession
from portal.db.session import get_session
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
from portal.lib.config import get_settings
from portal.lib.errors import NotFoundError
from portal.lib.lockout import check_password_lockout, register_password_failure
from portal.lib.logging import get_logger
from portal.lib.ratelimit import limit
from portal.notify.email import UploadedFile, send_upload_completion
from portal.services.app_settings import email_brand, resolve_branding
from portal.services.runtime_config import get_email_config
from portal.storage.base import DestinationConfig, UploadNotReady
from portal.storage.base import UploadSession as StorageUploadSession
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.sync.events import trigger_sync_for_upload
from portal.sync.paths import render_session_prefix
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
    verify_email: bool


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


@router.get("/{token}", response_model=PublicLinkOut, dependencies=[limit("default")])
async def resolve_link(token: str, db: AsyncSession = Depends(get_session)) -> PublicLinkOut:
    link = await _get_link(db, token)
    branding = merged_branding(link, link.destination)
    app_logo, app_name, app_accent = await resolve_branding(db, get_settings().base_url)
    fields = link.uploader_fields_required or {}
    return PublicLinkOut(
        state=link_state(link),
        display_name=branding["display_name"] or app_name or "Upload",
        subtitle=branding["subtitle"],
        logo_url=app_logo,
        accent_color=app_accent,
        password_required=link.password_hash is not None,
        max_file_size=link.max_file_size,
        allowed_extensions=link.allowed_extensions,
        uploader_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
            "message": bool(fields.get("message")),
        },
        verify_email=link.verify_email,
    )


class RequestCodeRequest(BaseModel):
    email: str


class RequestCodeResult(BaseModel):
    trusted: bool
    sent: bool


class VerifyCodeRequest(BaseModel):
    email: str
    code: str


@router.post(
    "/{token}/request-code", response_model=RequestCodeResult, dependencies=[limit("strict")]
)
async def request_code(
    token: str,
    body: RequestCodeRequest,
    request: Request,
    db: AsyncSession = Depends(get_session),
) -> RequestCodeResult:
    link = await _get_link(db, token)
    _require_usable(link)
    if not link.verify_email:
        raise HTTPException(status_code=400, detail="This link does not require verification")
    email = body.email.strip()
    if not email:
        raise HTTPException(status_code=422, detail="Email is required")
    if verify.is_trusted(request, email):
        return RequestCodeResult(trusted=True, sent=False)
    try:
        await verify.request_code(db, email)
    except verify.VerificationError as exc:
        raise HTTPException(status_code=exc.status_code, detail=exc.detail) from exc
    return RequestCodeResult(trusted=False, sent=True)


@router.post(
    "/{token}/verify-code", response_model=PasswordVerifyResult, dependencies=[limit("strict")]
)
async def verify_link_code(
    token: str,
    body: VerifyCodeRequest,
    request: Request,
    response: Response,
    db: AsyncSession = Depends(get_session),
) -> PasswordVerifyResult:
    """Validate an OTP up front and issue the device-trust cookie, so the UI can unlock the
    uploader as soon as the code is entered instead of validating only at session start."""
    link = await _get_link(db, token)
    _require_usable(link)
    if not link.verify_email:
        return PasswordVerifyResult(ok=True)
    email = body.email.strip()
    if not email:
        raise HTTPException(status_code=422, detail="Email is required")
    if verify.is_trusted(request, email):
        return PasswordVerifyResult(ok=True)
    if not await verify.verify_code(db, email, body.code):
        raise HTTPException(status_code=403, detail="That code is incorrect or expired")
    verify.issue_trust(response, email)
    return PasswordVerifyResult(ok=True)


@router.post(
    "/{token}/verify-password", response_model=PasswordVerifyResult, dependencies=[limit("strict")]
)
async def verify_link_password(
    token: str, body: PasswordVerifyRequest, db: AsyncSession = Depends(get_session)
) -> PasswordVerifyResult:
    link = await _get_link(db, token)
    # An open (no-password) link verifies trivially; a usable link must also be in "ok" state.
    if link.password_hash is None:
        return PasswordVerifyResult(ok=link_state(link) == "ok")
    await check_password_lockout("upload", token)
    state_ok = link_state(link) == "ok"
    ok = state_ok and verify_password(body.password, link.password_hash)
    if state_ok and not ok:
        await register_password_failure("upload", token)
    return PasswordVerifyResult(ok=ok)


# ── uploader session lifecycle ────────────────────────────────────────────────


class StartSessionRequest(BaseModel):
    name: str | None = None
    email: str | None = None
    message: str | None = None
    password: str | None = None
    code: str | None = None


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


class HeartbeatRequest(BaseModel):
    # Bytes the browser has pushed to storage so far — surfaces live progress on the dashboard.
    uploaded_bytes: int | None = Field(default=None, ge=0)


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


def _upload_destination_config(link: UploadLink) -> DestinationConfig:
    """Destination config for this link's uploads, with the folder overridden to the link's
    base subfolder when one is set (otherwise the destination's root folder)."""
    config = dict(link.destination.config)
    if link.target_folder_id:
        config["folder_id"] = link.target_folder_id
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


@router.post(
    "/{token}/sessions", response_model=StartSessionResult, dependencies=[limit("default")]
)
async def start_session(
    token: str,
    body: StartSessionRequest,
    request: Request,
    response: Response,
    db: AsyncSession = Depends(get_session),
) -> StartSessionResult:
    link = await _get_link(db, token)
    _require_usable(link)
    if link.password_hash is not None:
        await check_password_lockout("upload", token)
        if not verify_password(body.password or "", link.password_hash):
            await register_password_failure("upload", token)
            raise HTTPException(status_code=403, detail="Incorrect password")

    required = link.uploader_fields_required or {}
    provided = {"name": body.name, "email": body.email, "message": body.message}
    missing = [f for f in ("name", "email", "message") if required.get(f) and not provided[f]]
    if missing:
        raise HTTPException(
            status_code=422, detail=f"Missing required field(s): {', '.join(missing)}"
        )

    if link.verify_email:
        await verify.enforce_verification(
            db, request, response, (body.email or "").strip(), body.code
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

    # Place the file under the link's base subfolder (folder override) + per-session template
    # prefix; the backend mirrors any remaining browser path beneath that, auto-creating folders.
    prefix = render_session_prefix(
        link.subfolder_template,
        uploader_name=session.uploader_name or "",
        when=session.started_at or datetime.now(UTC),
    )
    effective_path = f"{prefix}/{body.path}" if prefix else body.path

    backend = FrameioStorageBackend(client=get_frameio_client())
    try:
        upload = await backend.create_upload_session(
            _upload_destination_config(link), filename=effective_path, size=body.size
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
    session.last_activity_at = datetime.now(UTC)
    if session.status == "abandoned":  # a live client outranks the stale sweep
        session.status = "in_progress"
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
    background: BackgroundTasks,
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
    except UploadNotReady:
        return CompleteFileResult(complete=False, failed=False)
    except FrameioError:
        return CompleteFileResult(complete=False, failed=True)

    # This file is finalized on Frame.io — start syncing it NOW rather than waiting for the whole
    # session to finish. In a large multi-file upload each file mirrors to the NAS as soon as it
    # lands, overlapping with the remaining uploads. trigger_sync_for_upload dedupes by
    # (rule, file_id), so the session-complete trigger stays a harmless safety net.
    dest_cfg = dict(link.destination.config)
    sync_account = str(dest_cfg.get("account_id") or "")
    sync_folder = str(dest_cfg.get("folder_id") or "")
    if sync_account and sync_folder:
        background.add_task(trigger_sync_for_upload, sync_account, sync_folder, [file_id])
    return CompleteFileResult(complete=True, failed=False)


@router.post("/{token}/sessions/{session_id}/complete", response_model=PasswordVerifyResult)
async def complete_session(
    token: str,
    session_id: str,
    body: CompleteSessionRequest,
    background: BackgroundTasks,
    response: Response,
    db: AsyncSession = Depends(get_session),
) -> PasswordVerifyResult:
    link = await _get_link(db, token)
    session = await _load_session(db, link, session_id)

    # Slide the device trust window on a real upload (verified links only).
    if link.verify_email and session.uploader_email:
        verify.issue_trust(response, session.uploader_email)
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
    # Resolve email config + branding now — the request session is gone by background-task time.
    email_config = await get_email_config(db)
    brand = await email_brand(db, get_settings().base_url)

    await db.commit()
    log.info("public.upload.session_completed", session_id=session_id)

    background.add_task(
        send_upload_completion,
        email_config,
        link_name=link_name,
        uploader_name=uploader[0],
        uploader_email=uploader[1],
        uploader_message=uploader[2],
        files=files,
        total_bytes=total_bytes,
        duration_seconds=duration,
        brand=brand,
    )
    # Portal's own sync trigger: mirror client-uploaded files to any matching sync rule's NAS
    # destination immediately, instead of waiting on a Frame.io webhook (which self-serve plans
    # don't deliver) or the reconcile cron.
    if sync_account and sync_folder and uploaded_file_ids:
        background.add_task(
            trigger_sync_for_upload, sync_account, sync_folder, uploaded_file_ids
        )
    return PasswordVerifyResult(ok=True)


# Like complete_*, heartbeat/abandon skip _require_usable: an upload that outlives its link's
# expiry must still be able to signal liveness and finish cleanly.


@router.post(
    "/{token}/sessions/{session_id}/heartbeat",
    response_model=PasswordVerifyResult,
    dependencies=[limit("default")],
)
async def session_heartbeat(
    token: str,
    session_id: str,
    body: HeartbeatRequest | None = None,
    db: AsyncSession = Depends(get_session),
) -> PasswordVerifyResult:
    """Liveness ping from the uploader page (~20s cadence) while an upload is running. Sessions
    that stop pinging get flagged "abandoned" by the worker sweep; a ping from a still-alive
    client revives one the sweep flagged too eagerly."""
    link = await _get_link(db, token)
    session = await _load_session(db, link, session_id)
    if session.status == "abandoned":
        session.status = "in_progress"
    session.last_activity_at = datetime.now(UTC)
    if body is not None and body.uploaded_bytes is not None:
        session.uploaded_bytes = body.uploaded_bytes
    await db.commit()
    return PasswordVerifyResult(ok=True)


@router.post(
    "/{token}/sessions/{session_id}/abandon",
    response_model=PasswordVerifyResult,
    dependencies=[limit("default")],
)
async def abandon_session(
    token: str,
    session_id: str,
    db: AsyncSession = Depends(get_session),
) -> PasswordVerifyResult:
    """Best-effort "tab is going away" signal (navigator.sendBeacon — hence no request body).
    Conditional so a beacon racing the final complete POST can never clobber "completed"
    (complete_session sets its status unconditionally, so either commit order ends completed)."""
    link = await _get_link(db, token)
    session = await _load_session(db, link, session_id)
    await db.execute(
        update(UploadSession)
        .where(UploadSession.id == session.id, UploadSession.status == "in_progress")
        .values(status="abandoned")
    )
    await db.commit()
    return PasswordVerifyResult(ok=True)
