"""ARQ worker — the background jobs queued by the API and the hourly reconciliation cron.

Tasks:
  process_webhook_event  turn an ingested webhook event into sync jobs (and enqueue their runs)
  run_sync_job           download one file to its local destination, with retry/backoff/dead-letter
  reconcile_rule_task    enqueue jobs for files in a rule's folder that don't have one
  reconcile_all (cron)   hourly: reconcile every enabled rule (catches missed webhook deliveries)

Bytes flow through this worker (not the API), and downloads stream to disk, so worker RSS stays
flat regardless of file size.
"""

import uuid
from datetime import UTC, datetime
from typing import Any

import httpx
from arq import cron
from arq.connections import RedisSettings
from sqlalchemy import select

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.lib.logging import configure_logging, get_logger
from portal.services.retention import purge_expired
from portal.services.upload_sessions import abandon_stale_upload_sessions
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.sync.events import process_event
from portal.sync.queue import RUN_SYNC_JOB
from portal.sync.runner import (
    SourceNotReady,
    execute_job,
    reclaim_orphaned_running,
    reconcile_rule,
)

log = get_logger("worker")

# Terminal job states the worker never re-runs.
_TERMINAL = {"done", "skipped", "dead_letter"}


def _unwrap_error(exc: BaseException) -> str:
    """Flatten an exception (incl. TaskGroup ExceptionGroups) to a readable message, so the job's
    recorded error is the real cause (e.g. an HTTP 403) rather than the opaque group wrapper."""
    if isinstance(exc, BaseExceptionGroup):
        return "; ".join(_unwrap_error(e) for e in exc.exceptions)
    return f"{type(exc).__name__}: {exc}"


async def process_webhook_event(ctx: dict[str, Any], event_id: str) -> int:
    """Process a webhook event into sync jobs, enqueueing a download for each new job."""
    redis = ctx["redis"]

    async def _enqueue(job_id: uuid.UUID) -> None:
        await redis.enqueue_job(RUN_SYNC_JOB, str(job_id))

    async with get_sessionmaker()() as db:
        return await process_event(db, uuid.UUID(event_id), on_job_created=_enqueue)


async def run_sync_job(ctx: dict[str, Any], job_id: str) -> str:
    """Download one file. On failure, retry with exponential backoff; dead-letter after the cap."""
    settings = get_settings()
    http: httpx.AsyncClient = ctx["http"]
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, uuid.UUID(job_id))
        if job is None or job.status in _TERMINAL:
            return "skip"
        # TimestampMixin.created_at is stored tz-naive; make it aware so the patient-wait math below
        # (now(UTC) - created_at) never raises and strands the job as "running".
        created_at = job.created_at
        if created_at.tzinfo is None:
            created_at = created_at.replace(tzinfo=UTC)
        job.status = "running"
        job.started_at = datetime.now(UTC)
        await db.commit()

        try:
            await execute_job(db, job, backend=FrameioStorageBackend(), http=http)
            await db.commit()
            return job.status
        except SourceNotReady as exc:
            # The original isn't downloadable yet (still finalizing). Wait patiently — re-check on
            # a fixed cadence WITHOUT spending the retry budget — and give up only after the
            # readiness timeout, so a 100 GB upload that takes a while to finalize still syncs.
            waited = (datetime.now(UTC) - created_at).total_seconds()
            if waited > settings.sync_source_ready_timeout_seconds:
                job.status = "dead_letter"
                job.error = f"source never became ready: {exc}"
                await db.commit()
                log.warning("sync.job.source_timeout", job_id=job_id, waited_s=int(waited))
                return "dead_letter"
            job.status = "waiting"
            job.error = str(exc)
            await db.commit()
            poll = settings.sync_source_poll_seconds
            await ctx["redis"].enqueue_job(RUN_SYNC_JOB, job_id, _defer_by=poll)
            log.info("sync.job.waiting_source", job_id=job_id, poll_s=poll)
            return "waiting"
        except Exception as exc:  # noqa: BLE001 - turn any failure into retry/dead-letter
            error = _unwrap_error(exc)
            job.retry_count += 1
            job.error = error
            if job.retry_count >= settings.sync_max_retries:
                job.status = "dead_letter"
                await db.commit()
                log.warning("sync.job.dead_letter", job_id=job_id, error=error)
                return "dead_letter"
            job.status = "pending"
            await db.commit()
            defer = settings.sync_retry_base_seconds * (2 ** (job.retry_count - 1))
            await ctx["redis"].enqueue_job(RUN_SYNC_JOB, job_id, _defer_by=defer)
            log.info("sync.job.retry", job_id=job_id, attempt=job.retry_count, defer_s=defer)
            return "retry"


async def reconcile_rule_task(ctx: dict[str, Any], rule_id: str) -> int:
    redis = ctx["redis"]

    async def _enqueue(job_id: uuid.UUID) -> None:
        await redis.enqueue_job(RUN_SYNC_JOB, str(job_id))

    async with get_sessionmaker()() as db:
        return await reconcile_rule(
            db, uuid.UUID(rule_id), backend=FrameioStorageBackend(), enqueue=_enqueue
        )


async def reconcile_all(ctx: dict[str, Any]) -> int:
    """Hourly cron: reconcile every enabled rule."""
    async with get_sessionmaker()() as db:
        rules = (
            (await db.execute(select(SyncRule.id).where(SyncRule.enabled.is_(True))))
            .scalars()
            .all()
        )
    total = 0
    for rule_id in rules:
        total += await reconcile_rule_task(ctx, str(rule_id))
    return total


async def retention_sweep(ctx: dict[str, Any]) -> dict[str, int]:
    """Daily cron: prune expired history rows (sync jobs excluded — see services.retention)."""
    async with get_sessionmaker()() as db:
        return await purge_expired(db)


async def abandon_stale_uploads(ctx: dict[str, Any]) -> int:
    """Cron: flag in_progress upload sessions whose uploader tab went silent as "abandoned"."""
    async with get_sessionmaker()() as db:
        return await abandon_stale_upload_sessions(db)


async def startup(ctx: dict[str, Any]) -> None:
    configure_logging(get_settings().log_level)
    ctx["http"] = httpx.AsyncClient(
        timeout=httpx.Timeout(60.0, connect=30.0, read=300.0), follow_redirects=True
    )
    log.info("worker.startup")

    # Any job still "running" was orphaned by a previous worker (single-worker deployment) — most
    # often killed mid-download by a deploy. Re-pick them now so they don't strand until the 24h
    # orphan window; this self-heals sync on every restart.
    redis = ctx["redis"]

    async def _enqueue(job_id: uuid.UUID) -> None:
        await redis.enqueue_job(RUN_SYNC_JOB, str(job_id))

    async with get_sessionmaker()() as db:
        await reclaim_orphaned_running(db, _enqueue)


async def shutdown(ctx: dict[str, Any]) -> None:
    http = ctx.get("http")
    if http is not None:
        await http.aclose()
    log.info("worker.shutdown")


# Run reconciliation every N minutes (N from config, clamped to a sane range).
_interval = max(1, min(get_settings().sync_reconcile_interval_minutes, 60))
_reconcile_minutes = set(range(0, 60, _interval))


class WorkerSettings:
    functions: list[Any] = [
        process_webhook_event,
        run_sync_job,
        reconcile_rule_task,
    ]
    cron_jobs = [
        cron(reconcile_all, minute=_reconcile_minutes),
        cron(retention_sweep, hour=3, minute=30),  # daily off-peak history prune
        cron(abandon_stale_uploads, minute={0, 15, 30, 45}),  # clear dead "Uploading now" rows
    ]
    on_startup = startup
    on_shutdown = shutdown
    redis_settings = RedisSettings.from_dsn(get_settings().redis_url)
    # arq cancels a task past job_timeout (its default is only 300s — fatal for multi-GB downloads).
    # Allow a full file pull; we run our own retry/dead-letter, so disable arq's auto-retry.
    job_timeout = get_settings().sync_job_timeout_seconds
    max_tries = 1
    # Cap parallel downloads — the NAS link is the bottleneck, so more just thrashes it.
    max_jobs = get_settings().sync_max_concurrent_jobs
