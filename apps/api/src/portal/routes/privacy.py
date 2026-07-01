"""Admin data-subject tools (GDPR access + erasure) — find a person's records by email.

Portal's personal data about link recipients lives in a few places:
  - upload_sessions   — uploader_name / uploader_email / uploader_message
  - download_sessions — viewer_name / viewer_email / ip / user_agent (+ their download_events)
  - email_verifications — a pending one-time code keyed by email

These endpoints let the operator (the data controller) honour access requests (export) and erasure
requests (anonymise the rows, keeping stats intact, or hard-delete them). Matching is
case-insensitive. Erasure is audited without the email so it isn't re-introduced into the audit log.
"""

from datetime import UTC, datetime
from typing import Any, Literal

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy import delete, func, select, update
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import (
    AuditLog,
    DownloadEvent,
    DownloadSession,
    EmailVerification,
    UploadSession,
    User,
)
from portal.db.session import get_session
from portal.lib.logging import get_logger

log = get_logger("routes.privacy")
router = APIRouter(prefix="/privacy", tags=["privacy"])


class SubjectExport(BaseModel):
    email: str
    generated_at: str
    upload_sessions: list[dict[str, Any]]
    download_sessions: list[dict[str, Any]]
    email_verifications: list[dict[str, Any]]


class EraseRequest(BaseModel):
    email: str
    mode: Literal["anonymize", "delete"]


class EraseResult(BaseModel):
    mode: str
    upload_sessions: int
    download_sessions: int
    email_verifications: int


def _iso(dt: datetime | None) -> str | None:
    return dt.isoformat() if dt else None


@router.get("/subject", response_model=SubjectExport)
async def export_subject(
    email: str,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> SubjectExport:
    """Everything Portal holds about a person, by email — for an access/portability request."""
    e = email.strip().lower()

    uploads = (
        (
            await db.execute(
                select(UploadSession)
                .where(func.lower(UploadSession.uploader_email) == e)
                .order_by(UploadSession.created_at.desc())
            )
        )
        .scalars()
        .all()
    )
    downloads = (
        (
            await db.execute(
                select(DownloadSession)
                .where(func.lower(DownloadSession.viewer_email) == e)
                .order_by(DownloadSession.started_at.desc().nullslast())
            )
        )
        .scalars()
        .all()
    )

    events_by_session: dict[Any, list[dict[str, Any]]] = {}
    dl_ids = [d.id for d in downloads]
    if dl_ids:
        for ev in (
            (
                await db.execute(
                    select(DownloadEvent)
                    .where(DownloadEvent.download_session_id.in_(dl_ids))
                    .order_by(DownloadEvent.started_at.desc())
                )
            )
            .scalars()
            .all()
        ):
            events_by_session.setdefault(ev.download_session_id, []).append(
                {
                    "file_name": ev.file_name,
                    "frameio_file_id": ev.frameio_file_id,
                    "completed": ev.completed,
                    "bytes_served": ev.bytes_served,
                    "started_at": _iso(ev.started_at),
                }
            )

    verifications = (
        (
            await db.execute(
                select(EmailVerification).where(func.lower(EmailVerification.email) == e)
            )
        )
        .scalars()
        .all()
    )

    db.add(
        AuditLog(
            user_id=user.id,
            action="privacy.subject_exported",
            detail={
                "email": e,
                "upload_sessions": len(uploads),
                "download_sessions": len(downloads),
            },
        )
    )
    await db.commit()

    return SubjectExport(
        email=e,
        generated_at=datetime.now(UTC).isoformat(),
        upload_sessions=[
            {
                "id": str(s.id),
                "upload_link_id": str(s.upload_link_id),
                "uploader_name": s.uploader_name,
                "uploader_email": s.uploader_email,
                "uploader_message": s.uploader_message,
                "status": s.status,
                "total_bytes": s.total_bytes,
                "started_at": _iso(s.started_at),
                "completed_at": _iso(s.completed_at),
                "created_at": _iso(s.created_at),
            }
            for s in uploads
        ],
        download_sessions=[
            {
                "id": str(s.id),
                "download_link_id": str(s.download_link_id),
                "viewer_name": s.viewer_name,
                "viewer_email": s.viewer_email,
                "ip": s.ip,
                "user_agent": s.user_agent,
                "started_at": _iso(s.started_at),
                "created_at": _iso(s.created_at),
                "events": events_by_session.get(s.id, []),
            }
            for s in downloads
        ],
        email_verifications=[
            {
                "email": v.email,
                "expires_at": _iso(v.expires_at),
                "attempts": v.attempts,
                "last_sent_at": _iso(v.last_sent_at),
            }
            for v in verifications
        ],
    )


@router.post("/subject/erase", response_model=EraseResult)
async def erase_subject(
    body: EraseRequest,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> EraseResult:
    """Erase a person's records by email — either anonymise the rows or hard-delete them."""
    e = body.email.strip().lower()

    up_ids = (
        (
            await db.execute(
                select(UploadSession.id).where(func.lower(UploadSession.uploader_email) == e)
            )
        )
        .scalars()
        .all()
    )
    dl_ids = (
        (
            await db.execute(
                select(DownloadSession.id).where(func.lower(DownloadSession.viewer_email) == e)
            )
        )
        .scalars()
        .all()
    )
    ver_ids = (
        (
            await db.execute(
                select(EmailVerification.id).where(func.lower(EmailVerification.email) == e)
            )
        )
        .scalars()
        .all()
    )

    if body.mode == "anonymize":
        if up_ids:
            await db.execute(
                update(UploadSession)
                .where(UploadSession.id.in_(up_ids))
                .values(uploader_name=None, uploader_email=None, uploader_message=None)
            )
        if dl_ids:
            await db.execute(
                update(DownloadSession)
                .where(DownloadSession.id.in_(dl_ids))
                .values(viewer_name=None, viewer_email=None, ip=None, user_agent=None)
            )
    else:  # delete — download_events cascade via the FK's ON DELETE CASCADE
        if up_ids:
            await db.execute(delete(UploadSession).where(UploadSession.id.in_(up_ids)))
        if dl_ids:
            await db.execute(delete(DownloadSession).where(DownloadSession.id.in_(dl_ids)))
    # A pending verification is keyed by the email itself, so anonymising is meaningless — always
    # delete it under either mode.
    if ver_ids:
        await db.execute(delete(EmailVerification).where(EmailVerification.id.in_(ver_ids)))

    # Audit the erasure WITHOUT the email, so we don't re-introduce the data we just removed.
    db.add(
        AuditLog(
            user_id=user.id,
            action="privacy.subject_erased",
            detail={
                "mode": body.mode,
                "upload_sessions": len(up_ids),
                "download_sessions": len(dl_ids),
                "email_verifications": len(ver_ids),
            },
        )
    )
    await db.commit()
    log.info(
        "privacy.subject_erased",
        mode=body.mode,
        upload_sessions=len(up_ids),
        download_sessions=len(dl_ids),
        email_verifications=len(ver_ids),
    )
    return EraseResult(
        mode=body.mode,
        upload_sessions=len(up_ids),
        download_sessions=len(dl_ids),
        email_verifications=len(ver_ids),
    )
