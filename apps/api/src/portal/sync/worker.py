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
from typing import Any

import httpx
from arq import cron
from arq.connections import RedisSettings
from sqlalchemy import select

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.lib.logging import configure_logging, get_logger
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.sync.events import process_event
from portal.sync.queue import RUN_SYNC_JOB
from portal.sync.runner import execute_job, reconcile_rule

log = get_logger("worker")

# Terminal job states the worker never re-runs.
_TERMINAL = {"done", "skipped", "dead_letter"}


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
        job.status = "running"
        await db.commit()

        try:
            await execute_job(db, job, backend=FrameioStorageBackend(), http=http)
            await db.commit()
            return job.status
        except Exception as exc:  # noqa: BLE001 - turn any failure into retry/dead-letter
            job.retry_count += 1
            job.error = str(exc)
            if job.retry_count >= settings.sync_max_retries:
                job.status = "dead_letter"
                await db.commit()
                log.warning("sync.job.dead_letter", job_id=job_id, error=str(exc))
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


async def startup(ctx: dict[str, Any]) -> None:
    configure_logging(get_settings().log_level)
    ctx["http"] = httpx.AsyncClient(
        timeout=httpx.Timeout(60.0, connect=30.0, read=300.0), follow_redirects=True
    )
    log.info("worker.startup")


async def shutdown(ctx: dict[str, Any]) -> None:
    http = ctx.get("http")
    if http is not None:
        await http.aclose()
    log.info("worker.shutdown")


class WorkerSettings:
    functions: list[Any] = [
        process_webhook_event,
        run_sync_job,
        reconcile_rule_task,
    ]
    cron_jobs = [cron(reconcile_all, minute=0)]
    on_startup = startup
    on_shutdown = shutdown
    redis_settings = RedisSettings.from_dsn(get_settings().redis_url)
