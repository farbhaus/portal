"""Daily data-retention sweep — bounds the growth of high-volume history tables.

`sync_jobs` are deliberately NOT pruned: reconcile treats a job's existence as the copy-once record
for a file, so deleting them would re-download already-synced files. Each window of 0 disables that
table's sweep.
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any, cast

from sqlalchemy import delete
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import AuditLog, DownloadSession, UploadSession, WebhookEvent
from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("services.retention")


async def purge_expired(db: AsyncSession, *, now: datetime | None = None) -> dict[str, int]:
    """Delete rows older than the configured windows; return per-table delete counts."""
    settings = get_settings()
    now = now or datetime.now(UTC)
    deleted: dict[str, int] = {}

    async def _sweep(label: str, model: type[Any], column: Any, days: int) -> None:
        if days <= 0:  # disabled (e.g. audit log kept indefinitely)
            return
        cutoff = now - timedelta(days=days)
        result = await db.execute(delete(model).where(column < cutoff))
        deleted[label] = cast("CursorResult[Any]", result).rowcount or 0

    await _sweep(
        "webhook_events", WebhookEvent, WebhookEvent.received_at,
        settings.retention_webhook_events_days,
    )
    # download_events cascade from download_sessions (FK ondelete=CASCADE).
    await _sweep(
        "upload_sessions", UploadSession, UploadSession.created_at,
        settings.retention_sessions_days,
    )
    await _sweep(
        "download_sessions", DownloadSession, DownloadSession.created_at,
        settings.retention_sessions_days,
    )
    await _sweep(
        "audit_log", AuditLog, AuditLog.created_at, settings.retention_audit_log_days,
    )
    await db.commit()
    if any(deleted.values()):
        log.info("retention.purged", **deleted)
    return deleted
