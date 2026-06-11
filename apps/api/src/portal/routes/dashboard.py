"""Dashboard — an operations view: live uploads, a unified recent-transfer feed, sync health."""

from datetime import UTC, datetime, timedelta

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import (
    DownloadEvent,
    DownloadSession,
    SyncJob,
    SyncRule,
    UploadLink,
    UploadSession,
    User,
)
from portal.db.session import get_session

router = APIRouter(prefix="/dashboard", tags=["dashboard"])


class ActiveUpload(BaseModel):
    id: str
    who: str | None
    brand: str | None
    total_bytes: int | None
    started_at: str | None


class RecentTransfer(BaseModel):
    id: str
    kind: str  # "upload" | "download" | "sync"
    label: str | None
    who: str | None
    bytes: int | None
    status: str
    at: str


class SyncHealth(BaseModel):
    waiting: int
    running: int
    dead_letter: int
    done_24h: int


class TransfersOut(BaseModel):
    active_uploads: list[ActiveUpload]
    recent: list[RecentTransfer]
    sync_health: SyncHealth


def _iso(dt: datetime | None) -> str | None:
    return dt.isoformat() if dt else None


@router.get("/transfers", response_model=TransfersOut)
async def transfers(
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    limit: int = 20,
) -> TransfersOut:
    limit = max(1, min(limit, 100))

    # --- live: uploads still in flight -------------------------------------
    active_rows = (
        await db.execute(
            select(UploadSession, UploadLink.brand_display_name)
            .join(UploadLink, UploadSession.upload_link_id == UploadLink.id)
            .where(UploadSession.status == "in_progress")
            .order_by(UploadSession.started_at.desc().nullslast())
            .limit(50)
        )
    ).all()
    active_uploads = [
        ActiveUpload(
            id=str(s.id),
            who=s.uploader_name or s.uploader_email,
            brand=brand,
            total_bytes=s.total_bytes,
            started_at=_iso(s.started_at),
        )
        for s, brand in active_rows
    ]

    # --- recent: merge completed uploads, downloads, and sync jobs ----------
    feed: list[tuple[datetime, RecentTransfer]] = []

    upload_rows = (
        await db.execute(
            select(UploadSession, UploadLink.brand_display_name)
            .join(UploadLink, UploadSession.upload_link_id == UploadLink.id)
            .where(UploadSession.status == "completed")
            .order_by(UploadSession.completed_at.desc().nullslast())
            .limit(limit)
        )
    ).all()
    for s, brand in upload_rows:
        at = s.completed_at or s.created_at
        feed.append(
            (
                at,
                RecentTransfer(
                    id=str(s.id),
                    kind="upload",
                    label=brand or "Upload",
                    who=s.uploader_name or s.uploader_email,
                    bytes=s.total_bytes,
                    status="completed",
                    at=at.isoformat(),
                ),
            )
        )

    download_rows = (
        await db.execute(
            select(DownloadEvent, DownloadSession.viewer_email)
            .join(DownloadSession, DownloadEvent.download_session_id == DownloadSession.id)
            .order_by(DownloadEvent.started_at.desc())
            .limit(limit)
        )
    ).all()
    for ev, viewer in download_rows:
        feed.append(
            (
                ev.started_at,
                RecentTransfer(
                    id=str(ev.id),
                    kind="download",
                    label=ev.file_name or "Download",
                    who=viewer,
                    bytes=ev.bytes_served,
                    status="completed" if ev.completed else "in_progress",
                    at=ev.started_at.isoformat(),
                ),
            )
        )

    sync_rows = (
        await db.execute(
            select(SyncJob, SyncRule.name)
            .join(SyncRule, SyncJob.sync_rule_id == SyncRule.id)
            .order_by(SyncJob.created_at.desc())
            .limit(limit)
        )
    ).all()
    for job, rule_name in sync_rows:
        at = job.completed_at or job.started_at or job.created_at
        feed.append(
            (
                at,
                RecentTransfer(
                    id=str(job.id),
                    kind="sync",
                    label=rule_name,
                    who=None,
                    bytes=job.bytes,
                    status=job.status,
                    at=at.isoformat(),
                ),
            )
        )

    # Some timestamp columns can come back naive; normalise so the merge sort
    # never compares offset-naive against offset-aware datetimes.
    def _key(row: tuple[datetime, RecentTransfer]) -> datetime:
        dt = row[0]
        return dt if dt.tzinfo else dt.replace(tzinfo=UTC)

    feed.sort(key=_key, reverse=True)
    recent = [item for _at, item in feed[:limit]]

    # --- sync health --------------------------------------------------------
    status_rows = (
        await db.execute(select(SyncJob.status, func.count()).group_by(SyncJob.status))
    ).all()
    status_counts: dict[str, int] = {row[0]: row[1] for row in status_rows}
    since = datetime.now(UTC) - timedelta(hours=24)
    done_24h = (
        await db.scalar(
            select(func.count())
            .select_from(SyncJob)
            .where(SyncJob.status == "done", SyncJob.completed_at >= since)
        )
        or 0
    )
    sync_health = SyncHealth(
        waiting=status_counts.get("waiting", 0),
        running=status_counts.get("running", 0),
        dead_letter=status_counts.get("dead_letter", 0),
        done_24h=done_24h,
    )

    return TransfersOut(active_uploads=active_uploads, recent=recent, sync_health=sync_health)
