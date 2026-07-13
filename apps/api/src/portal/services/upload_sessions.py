"""Abandoned-upload sweep — flags upload sessions whose uploader tab went silent.

The uploader page heartbeats while an upload runs; if a session stays "in_progress" with no
liveness signal past the configured window (tab closed, crash, network gone), the worker cron
flips it to "abandoned" so it stops counting as "Uploading now" on the dashboard. A heartbeat or
file registration from a still-alive client revives it, and session completion always wins, so a
premature flag self-heals. A window of 0 disables the sweep.
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any, cast

from sqlalchemy import func, update
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import UploadSession
from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("services.upload_sessions")


async def abandon_stale_upload_sessions(db: AsyncSession, *, now: datetime | None = None) -> int:
    """Mark silent in_progress sessions "abandoned"; return how many were flagged."""
    minutes = get_settings().upload_stale_after_minutes
    if minutes <= 0:  # disabled
        return 0
    cutoff = (now or datetime.now(UTC)) - timedelta(minutes=minutes)
    result = await db.execute(
        update(UploadSession)
        .where(
            UploadSession.status == "in_progress",
            # Pre-heartbeat rows (last_activity_at NULL) fall back to when the upload started.
            func.coalesce(
                UploadSession.last_activity_at,
                UploadSession.started_at,
                UploadSession.created_at,
            )
            < cutoff,
        )
        .values(status="abandoned")
    )
    await db.commit()
    count = cast("CursorResult[Any]", result).rowcount or 0
    if count:
        log.info("uploads.abandoned_stale", count=count)
    return count
