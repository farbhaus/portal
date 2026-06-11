"""Public (unauthenticated) download-link endpoints — back the /d/{token} recipient page.

Flow: GET resolves branding + gates; POST /sessions captures viewer info (after password/fields)
and returns the file listing; POST .../url mints a short-lived Frame.io signed URL for one file,
records a download_event, and enforces the total download cap. Bytes are fetched by the browser
directly from Frame.io (the browser is redirected to the signed URL) — never through Portal.
Frame.io internals are never exposed.
"""

import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request, Response
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal import verify
from portal.auth.passwords import verify_password
from portal.db.models import DownloadEvent, DownloadLink, DownloadSession
from portal.db.session import get_session
from portal.downloads.links import branding, resolve_source
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
from portal.lib.errors import NotFoundError
from portal.lib.logging import get_logger
from portal.storage.base import DestinationConfig
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.uploads.links import link_state

log = get_logger("routes.public_downloads")
router = APIRouter(prefix="/public/downloads", tags=["public"])


class DownloadPageOut(BaseModel):
    state: str  # ok | expired | revoked
    display_name: str
    subtitle: str | None
    logo_url: str | None
    accent_color: str | None
    password_required: bool
    viewer_fields_required: dict[str, bool]
    verify_email: bool
    allow_preview: bool


class StartSessionRequest(BaseModel):
    name: str | None = None
    email: str | None = None
    password: str | None = None
    code: str | None = None


class RequestCodeRequest(BaseModel):
    email: str


class RequestCodeResult(BaseModel):
    trusted: bool
    sent: bool


class FileOut(BaseModel):
    id: str
    name: str
    size: int | None
    media_type: str | None
    thumbnail_url: str | None


class StartSessionResult(BaseModel):
    session_id: str
    files: list[FileOut]


class DownloadUrlResult(BaseModel):
    url: str


async def _get_link(db: AsyncSession, token: str) -> DownloadLink:
    result = await db.execute(select(DownloadLink).where(DownloadLink.token == token))
    link = result.scalar_one_or_none()
    if link is None:
        raise NotFoundError("This download link does not exist")
    return link


def _require_usable(link: DownloadLink) -> None:
    state = link_state(link)
    if state != "ok":
        raise HTTPException(status_code=410, detail=f"This link is {state}")


async def _load_session(db: AsyncSession, link: DownloadLink, session_id: str) -> DownloadSession:
    try:
        sid = uuid.UUID(session_id)
    except ValueError as exc:
        raise NotFoundError("Download session not found") from exc
    session = await db.get(DownloadSession, sid)
    if session is None or session.download_link_id != link.id:
        raise NotFoundError("Download session not found")
    return session


@router.get("/{token}", response_model=DownloadPageOut)
async def resolve_link(token: str, db: AsyncSession = Depends(get_session)) -> DownloadPageOut:
    link = await _get_link(db, token)
    b = branding(link)
    fields = link.viewer_fields_required or {}
    return DownloadPageOut(
        state=link_state(link),
        display_name=b["display_name"] or "Download",
        subtitle=b["subtitle"],
        logo_url=b["logo_url"],
        accent_color=b["accent_color"],
        password_required=link.password_hash is not None,
        viewer_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
        },
        verify_email=link.verify_email,
        allow_preview=link.allow_preview,
    )


@router.post("/{token}/request-code", response_model=RequestCodeResult)
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


@router.post("/{token}/sessions", response_model=StartSessionResult)
async def start_session(
    token: str,
    body: StartSessionRequest,
    request: Request,
    response: Response,
    db: AsyncSession = Depends(get_session),
) -> StartSessionResult:
    link = await _get_link(db, token)
    _require_usable(link)
    if link.password_hash is not None and not verify_password(
        body.password or "", link.password_hash
    ):
        raise HTTPException(status_code=403, detail="Incorrect password")

    required = link.viewer_fields_required or {}
    provided = {"name": body.name, "email": body.email}
    missing = [f for f in ("name", "email") if required.get(f) and not provided[f]]
    if missing:
        raise HTTPException(
            status_code=422, detail=f"Missing required field(s): {', '.join(missing)}"
        )

    if link.verify_email:
        await verify.enforce_verification(
            db, request, response, (body.email or "").strip(), body.code
        )

    try:
        resolved = await resolve_source(get_frameio_client(), link.source)
    except FrameioNotConnected as exc:
        raise HTTPException(
            status_code=503, detail="Downloads are temporarily unavailable"
        ) from exc
    except (FrameioError, ValueError) as exc:
        log.warning("public.download.resolve_failed", error=str(exc))
        raise HTTPException(status_code=502, detail="Could not load the files") from exc

    session = DownloadSession(
        download_link_id=link.id,
        viewer_name=body.name,
        viewer_email=body.email,
        ip=request.client.host if request.client else None,
        user_agent=request.headers.get("user-agent"),
        started_at=datetime.now(UTC),
    )
    db.add(session)
    await db.commit()
    await db.refresh(session)

    files = [
        FileOut(
            id=f.id,
            name=f.name,
            size=f.file_size,
            media_type=f.media_type,
            thumbnail_url=f.thumbnail_url if link.allow_preview else None,
        )
        for f in resolved.files
    ]
    return StartSessionResult(session_id=str(session.id), files=files)


@router.post("/{token}/sessions/{session_id}/files/{file_id}/url", response_model=DownloadUrlResult)
async def get_download_url(
    token: str,
    session_id: str,
    file_id: str,
    response: Response,
    db: AsyncSession = Depends(get_session),
) -> DownloadUrlResult:
    link = await _get_link(db, token)
    _require_usable(link)
    session = await _load_session(db, link, session_id)

    # Slide the device trust window on real activity (verified links only).
    if link.verify_email and session.viewer_email:
        verify.issue_trust(response, session.viewer_email)

    if link.max_downloads is not None and link.downloads_count >= link.max_downloads:
        raise HTTPException(status_code=429, detail="This link's download limit has been reached")

    # The recipient may only download files that belong to the link's source.
    if not await _source_contains(link.source, file_id):
        raise HTTPException(status_code=404, detail="File not found in this link")

    account_id = str(link.source["account_id"])
    backend = FrameioStorageBackend(client=get_frameio_client())
    try:
        result = await backend.get_download_url(
            DestinationConfig(type="frameio", data={"account_id": account_id}), file_id
        )
    except FrameioNotConnected as exc:
        raise HTTPException(
            status_code=503, detail="Downloads are temporarily unavailable"
        ) from exc
    except FrameioError as exc:
        log.warning("public.download.url_failed", error=str(exc))
        raise HTTPException(status_code=502, detail="Could not generate the download") from exc

    db.add(
        DownloadEvent(download_session_id=session.id, frameio_file_id=file_id, completed=False)
    )
    link.downloads_count += 1
    await db.commit()
    log.info("public.download.url_minted", token=token, file_id=file_id)
    return DownloadUrlResult(url=result.url)


async def _source_contains(source: dict[str, Any], file_id: str) -> bool:
    stype = source.get("type")
    if stype == "file":
        return file_id == source.get("file_id")
    if stype == "selection":
        return file_id in (source.get("file_ids") or [])
    if stype == "folder":
        resolved = await resolve_source(get_frameio_client(), source)
        return any(f.id == file_id for f in resolved.files)
    return False
