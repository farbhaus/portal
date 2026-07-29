"""Admin actions on individual sync jobs.

Rules are managed in ``portal.routes.sync_rules``; this module is the remediation surface for the
jobs themselves. Without it a failed job is a dead end — the dashboard reports "N sync jobs failed
and need attention" and there is nothing anywhere to act on it.

Two actions, both terminal decisions an admin makes about one file:

``retry``    put the job back in the queue for another attempt. Resets the readiness clock too, so
             the attempt gets a full window instead of inheriting an expired one and failing again
             on its first tick (see ``portal.sync.worker``).
``dismiss``  stop caring about this file. Marks it ``skipped`` — the same terminal state the engine
             uses for a source that no longer exists — so reconcile won't resurrect it and it drops
             out of the dashboard's failure count. The error text is kept for the record.
"""

import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Depends
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import AuditLog, SyncJob, User
from portal.db.session import get_session
from portal.lib.errors import NotFoundError
from portal.lib.logging import get_logger
from portal.sync.queue import enqueue_sync_job

log = get_logger("routes.sync_jobs")

router = APIRouter(prefix="/sync-jobs", tags=["sync-jobs"])


async def _get_or_404(db: AsyncSession, job_id: uuid.UUID) -> SyncJob:
    job = await db.get(SyncJob, job_id)
    if job is None:
        raise NotFoundError("sync job not found")
    return job


@router.post("/{job_id}/retry")
async def retry_job(
    job_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, Any]:
    """Queue another attempt at one job, whatever state it ended in."""
    job = await _get_or_404(db, job_id)
    job.status = "pending"
    job.retry_count = 0
    job.error = None
    job.started_at = None
    job.completed_at = None
    job.source_wait_started_at = None
    db.add(
        AuditLog(user_id=user.id, action="sync_job.retried", detail={"sync_job_id": str(job_id)})
    )
    await db.commit()
    await enqueue_sync_job(job.id)
    log.info("sync_job.retry", job_id=str(job_id), file_id=job.frameio_file_id)
    return {"ok": True, "status": job.status}


@router.post("/{job_id}/dismiss")
async def dismiss_job(
    job_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, Any]:
    """Retire a job the admin has decided not to chase — typically an abandoned upload."""
    job = await _get_or_404(db, job_id)
    job.status = "skipped"
    job.completed_at = datetime.now(UTC)
    db.add(
        AuditLog(user_id=user.id, action="sync_job.dismissed", detail={"sync_job_id": str(job_id)})
    )
    await db.commit()
    log.info("sync_job.dismissed", job_id=str(job_id), file_id=job.frameio_file_id)
    return {"ok": True, "status": job.status}
