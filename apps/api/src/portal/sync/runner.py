"""Sync job execution and folder reconciliation.

``execute_job`` downloads one file to its templated local path, applying the folder filter and
conflict policy, and records bytes + SHA-256 on the job. It raises only on genuine failures;
``skip`` outcomes (file outside the rule's folder, or conflict policy = skip) are terminal and
non-error. Retry/backoff/dead-letter bookkeeping lives in the worker wrapper (portal.sync.worker),
which owns the ARQ redis needed to re-enqueue.

``reconcile_rule`` lists a rule's source folder and enqueues jobs for files that don't have one —
the safety net for webhook deliveries missed during downtime.
"""

from __future__ import annotations

import uuid
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime

import httpx
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import SyncJob, SyncRule
from portal.frameio.client import FrameioForbidden, FrameioNotFound
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.storage.base import DestinationConfig, StorageBackend
from portal.sync.download import download_to_file
from portal.sync.paths import PathContext, resolve_conflict, resolve_destination

log = get_logger("sync.runner")

EnqueueJob = Callable[[uuid.UUID], Awaitable[None]]

# Frame.io file states from which we'll *attempt* the original download. "uploaded" means the
# upload finalized — the original is usually fetchable then, well before transcoding (proxies)
# finishes, so we don't make the user wait for transcode. If the original still isn't ready we get
# a 403, which we treat as "not ready yet" (patient wait) rather than a failure — see execute_job.
DOWNLOADABLE_STATUSES = frozenset({"uploaded", "transcoded"})


def _is_not_ready_403(exc: BaseException) -> bool:
    """True if exc (or anything in its ExceptionGroup) is an HTTP 403 — the signal that a Frame.io
    original isn't finalized yet. Lets us wait patiently instead of burning the retry budget."""
    if isinstance(exc, BaseExceptionGroup):
        return any(_is_not_ready_403(e) for e in exc.exceptions)
    if isinstance(exc, FrameioForbidden):
        return True
    return isinstance(exc, httpx.HTTPStatusError) and exc.response.status_code == 403


class SourceNotReady(Exception):
    """The remote file isn't downloadable yet (still finalizing/transcoding). Retry later, and
    do NOT count this toward the dead-letter cap — large originals can take a long time."""


def _destination(rule: SyncRule) -> DestinationConfig:
    cfg = dict(rule.source_config)
    return DestinationConfig(type=str(cfg.get("type", "frameio")), data=cfg)


def _is_orphaned(job: SyncJob, now: datetime, stale_after_seconds: int) -> bool:
    """A job stuck "running" longer than a full job timeout means its worker died mid-download
    (no task is alive to finish or fail it), so reconcile should re-pick it."""
    if job.status != "running" or job.started_at is None:
        return False
    return (now - job.started_at).total_seconds() > stale_after_seconds


def _in_scope(rule: SyncRule, parent_id: str | None) -> bool:
    """A non-recursive rule only takes files directly in its folder; recursive takes everything
    the source listing returns (we can't cheaply prove descendant-ship from a webhook event)."""
    if rule.source_config.get("recursive", True):
        return True
    return parent_id == rule.source_config.get("folder_id")


async def execute_job(
    db: AsyncSession,
    job: SyncJob,
    *,
    backend: StorageBackend,
    http: httpx.AsyncClient | None = None,
) -> None:
    """Run one sync job to completion, setting its terminal fields. Raises on real failures."""
    rule = await db.get(SyncRule, job.sync_rule_id)
    if rule is None:
        raise RuntimeError(f"sync rule {job.sync_rule_id} is gone")

    dest = _destination(rule)
    try:
        remote = await backend.get_file(dest, job.frameio_file_id)
    except FrameioNotFound:
        # The source was deleted/moved on Frame.io before we pulled it. Sync is additive/copy-once,
        # so there's nothing to fetch — terminal skip (don't retry into dead_letter).
        job.status = "skipped"
        job.completed_at = datetime.now(UTC)
        job.error = "source file no longer exists on Frame.io"
        log.info("sync.job.skipped_source_gone", job_id=str(job.id), file_id=job.frameio_file_id)
        return

    # Wait for the original to be downloadable. Frame.io reports a status; if it's not yet a
    # downloadable one, defer (the worker re-queues patiently without burning the retry budget).
    if remote.status is not None and remote.status not in DOWNLOADABLE_STATUSES:
        raise SourceNotReady(f"file status is '{remote.status}', not yet downloadable")

    if not _in_scope(rule, remote.parent_id):
        job.status = "skipped"
        job.completed_at = datetime.now(UTC)
        log.info("sync.job.skipped_out_of_scope", job_id=str(job.id), file_id=job.frameio_file_id)
        return

    ctx = PathContext(
        filename=remote.name,
        project=str(rule.source_config.get("project_name", "")),
        folder=str(rule.source_config.get("folder_name", "")),
        subfolder=remote.relative_dir,
        when=datetime.now(),
    )
    target = resolve_destination(rule.destination_path, rule.path_template, ctx)
    final = resolve_conflict(target, rule.conflict_policy)
    if final is None:
        job.status = "skipped"
        job.completed_at = datetime.now(UTC)
        log.info("sync.job.skipped_conflict", job_id=str(job.id), path=str(target))
        return

    try:
        download = await backend.get_download_url(dest, job.frameio_file_id)
    except FrameioForbidden as exc:
        # Original not finalized yet — wait patiently rather than spend the retry budget.
        raise SourceNotReady("original not downloadable yet (403)") from exc

    tmp = final.with_name(final.name + ".part")
    try:
        result = await download_to_file(
            download.url,
            remote.size,
            tmp,
            concurrency=get_settings().sync_download_concurrency,
            client=http,
        )
        tmp.replace(final)
    except BaseException as exc:
        tmp.unlink(missing_ok=True)
        if _is_not_ready_403(exc):
            raise SourceNotReady("original not finalized yet (403 during download)") from exc
        raise

    job.status = "done"
    job.bytes = result.bytes_written
    job.sha256 = result.sha256
    job.completed_at = datetime.now(UTC)
    job.error = None
    log.info(
        "sync.job.done",
        job_id=str(job.id),
        path=str(final),
        bytes=result.bytes_written,
        sha256=result.sha256,
    )


async def reclaim_orphaned_running(db: AsyncSession, enqueue: EnqueueJob) -> int:
    """Re-pick every job left ``running`` and re-enqueue it. Called once at worker startup.

    We run a single worker, so any job still ``running`` when the worker boots was orphaned by a
    previous worker that died mid-download (e.g. killed by a deploy; arq's ``max_tries=1`` means it
    is never re-queued). Without this, such a job sits ``running`` until ``_is_orphaned`` notices it
    a full ``sync_job_timeout`` (24h) later, and reconcile won't create a fresh job because a row
    already exists — so the file strands for a day. NOTE: assumes a single worker; with multiple
    workers this would steal in-flight jobs.
    """
    jobs = (await db.execute(select(SyncJob).where(SyncJob.status == "running"))).scalars().all()
    for job in jobs:
        job.status = "pending"
        job.started_at = None
        job.completed_at = None
        job.error = None
        await db.flush()
        await enqueue(job.id)
    await db.commit()
    if jobs:
        log.info("sync.reclaim.orphaned_running", count=len(jobs))
    return len(jobs)


async def reconcile_rule(
    db: AsyncSession, rule_id: uuid.UUID, *, backend: StorageBackend, enqueue: EnqueueJob
) -> int:
    """Create + enqueue jobs for files in a rule's folder that lack one. Returns the count."""
    rule = await db.get(SyncRule, rule_id)
    if rule is None or not rule.enabled:
        return 0

    dest = _destination(rule)
    if rule.source_config.get("recursive", True):
        files = await backend.list_files_recursive(dest)
    else:
        files = await backend.list_files_in_destination(dest)

    now = datetime.now(UTC)
    stale_after = get_settings().sync_job_timeout_seconds

    created = 0
    for f in files:
        existing = await db.scalar(
            select(SyncJob).where(
                SyncJob.sync_rule_id == rule.id, SyncJob.frameio_file_id == f.id
            )
        )
        if existing is None:
            job = SyncJob(sync_rule_id=rule.id, frameio_file_id=f.id, status="pending")
            db.add(job)
            await db.flush()
            await enqueue(job.id)
            created += 1
        elif existing.status == "dead_letter" or _is_orphaned(existing, now, stale_after):
            # Self-heal: a transient failure (source not ready in time, a network blip) or a job
            # left "running" by a killed worker shouldn't strand a file. Reset + re-attempt.
            existing.status = "pending"
            existing.retry_count = 0
            existing.error = None
            existing.started_at = None
            existing.completed_at = None
            await db.flush()
            await enqueue(existing.id)
            created += 1
        # done / skipped / pending / waiting / fresh-running: leave as-is

    await db.commit()
    if created:
        log.info("sync.reconcile.created", rule_id=str(rule_id), count=created)
    return created
