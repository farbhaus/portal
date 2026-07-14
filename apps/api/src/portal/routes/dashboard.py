"""Dashboard — an operations view: live uploads, a unified recent-transfer feed, sync health."""

from datetime import UTC, datetime, timedelta

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy import and_, func, or_, select
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
    uploaded_bytes: int | None  # live progress from the uploader page's heartbeat
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


class HistoryOut(BaseModel):
    items: list[RecentTransfer]
    has_more: bool


def _iso(dt: datetime | None) -> str | None:
    return dt.isoformat() if dt else None


def _planned_bytes(session: UploadSession) -> int | None:
    """Sum of registered file sizes — the in-flight stand-in for total_bytes, which is only
    written at session completion."""
    entries = session.frameio_asset_ids or []
    total = sum(int(e.get("size") or 0) for e in entries if isinstance(e, dict))
    return total or None


# Uploads that belong in the transfer feeds: completed ones, plus abandoned ones that actually
# registered a file (a session someone opened and walked away from is noise, not a transfer).
_upload_feed_filter = or_(
    UploadSession.status == "completed",
    and_(
        UploadSession.status == "abandoned",
        UploadSession.frameio_asset_ids.isnot(None),
        func.jsonb_array_length(UploadSession.frameio_asset_ids) > 0,
    ),
)

# Feed recency: completion time, or — for abandoned sessions — when they were last heard from.
_upload_feed_at = func.coalesce(
    UploadSession.completed_at,
    UploadSession.last_activity_at,
    UploadSession.started_at,
    UploadSession.created_at,
)


def _upload_feed_item(s: UploadSession, brand: str | None) -> tuple[datetime, RecentTransfer]:
    at = s.completed_at or s.last_activity_at or s.started_at or s.created_at
    if s.status == "completed":
        status, size = "completed", s.total_bytes
    else:  # abandoned → shown as "incomplete", sized by what actually made it up
        status, size = "incomplete", s.uploaded_bytes or _planned_bytes(s)
    return (
        at,
        RecentTransfer(
            id=str(s.id),
            kind="upload",
            label=brand or "Upload",
            who=s.uploader_name or s.uploader_email,
            bytes=size,
            status=status,
            at=at.isoformat(),
        ),
    )


@router.get("/transfers", response_model=TransfersOut)
async def transfers(
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    limit: int = 20,
) -> TransfersOut:
    limit = max(1, min(limit, 100))

    # --- live: uploads still in flight -------------------------------------
    # The 24h bound is belt-and-braces against a dead worker: the abandon sweep normally clears
    # silent sessions within ~upload_stale_after_minutes.
    active_rows = (
        await db.execute(
            select(UploadSession, UploadLink.brand_display_name)
            .join(UploadLink, UploadSession.upload_link_id == UploadLink.id)
            .where(
                UploadSession.status == "in_progress",
                func.coalesce(
                    UploadSession.last_activity_at,
                    UploadSession.started_at,
                    UploadSession.created_at,
                )
                >= datetime.now(UTC) - timedelta(hours=24),
            )
            .order_by(UploadSession.started_at.desc().nullslast())
            .limit(50)
        )
    ).all()
    active_uploads = [
        ActiveUpload(
            id=str(s.id),
            who=s.uploader_name or s.uploader_email,
            brand=brand,
            uploaded_bytes=s.uploaded_bytes,
            total_bytes=s.total_bytes if s.total_bytes is not None else _planned_bytes(s),
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
            .where(_upload_feed_filter)
            .order_by(_upload_feed_at.desc())
            .limit(limit)
        )
    ).all()
    for s, brand in upload_rows:
        feed.append(_upload_feed_item(s, brand))

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


@router.get("/history", response_model=HistoryOut)
async def history(
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    kind: str = "all",
    limit: int = 50,
    offset: int = 0,
) -> HistoryOut:
    """Paginated, full transfer history — completed uploads and file downloads merged newest-first.

    Backs the dashboard's "Activity log" link (an expanded view of the recent-transfers feed).
    `kind` filters to "upload"/"download" (default "all"); pagination is offset/limit.
    """
    limit = max(1, min(limit, 200))
    offset = max(0, offset)
    want_upload = kind in ("all", "upload")
    want_download = kind in ("all", "download")

    # To slice a merged feed correctly at [offset, offset+limit) we need the top (offset+limit+1)
    # of each included source; the +1 tells us whether a further page exists.
    take = offset + limit + 1
    pool: list[tuple[datetime, RecentTransfer]] = []

    if want_upload:
        upload_rows = (
            await db.execute(
                select(UploadSession, UploadLink.brand_display_name)
                .join(UploadLink, UploadSession.upload_link_id == UploadLink.id)
                .where(_upload_feed_filter)
                .order_by(_upload_feed_at.desc())
                .limit(take)
            )
        ).all()
        for s, brand in upload_rows:
            pool.append(_upload_feed_item(s, brand))

    if want_download:
        download_rows = (
            await db.execute(
                select(DownloadEvent, DownloadSession.viewer_email)
                .join(DownloadSession, DownloadEvent.download_session_id == DownloadSession.id)
                .order_by(DownloadEvent.started_at.desc())
                .limit(take)
            )
        ).all()
        for ev, viewer in download_rows:
            pool.append(
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

    def _key(row: tuple[datetime, RecentTransfer]) -> datetime:
        dt = row[0]
        return dt if dt.tzinfo else dt.replace(tzinfo=UTC)

    pool.sort(key=_key, reverse=True)
    window = pool[offset : offset + limit + 1]
    has_more = len(window) > limit
    return HistoryOut(items=[item for _at, item in window[:limit]], has_more=has_more)
