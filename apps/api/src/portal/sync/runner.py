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
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.storage.base import DestinationConfig, StorageBackend
from portal.sync.download import download_to_file
from portal.sync.paths import PathContext, resolve_conflict, resolve_destination

log = get_logger("sync.runner")

EnqueueJob = Callable[[uuid.UUID], Awaitable[None]]


def _destination(rule: SyncRule) -> DestinationConfig:
    cfg = dict(rule.source_config)
    return DestinationConfig(type=str(cfg.get("type", "frameio")), data=cfg)


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
    remote = await backend.get_file(dest, job.frameio_file_id)

    if not _in_scope(rule, remote.parent_id):
        job.status = "skipped"
        job.completed_at = datetime.now(UTC)
        log.info("sync.job.skipped_out_of_scope", job_id=str(job.id), file_id=job.frameio_file_id)
        return

    ctx = PathContext(
        filename=remote.name,
        project=str(rule.source_config.get("project_name", "")),
        folder=str(rule.source_config.get("folder_name", "")),
        when=datetime.now(),
    )
    target = resolve_destination(rule.destination_path, rule.path_template, ctx)
    final = resolve_conflict(target, rule.conflict_policy)
    if final is None:
        job.status = "skipped"
        job.completed_at = datetime.now(UTC)
        log.info("sync.job.skipped_conflict", job_id=str(job.id), path=str(target))
        return

    download = await backend.get_download_url(dest, job.frameio_file_id)
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
    except BaseException:
        tmp.unlink(missing_ok=True)
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

    created = 0
    for f in files:
        existing = await db.scalar(
            select(SyncJob.id).where(
                SyncJob.sync_rule_id == rule.id, SyncJob.frameio_file_id == f.id
            )
        )
        if existing is not None:
            continue
        job = SyncJob(sync_rule_id=rule.id, frameio_file_id=f.id, status="pending")
        db.add(job)
        await db.flush()
        await enqueue(job.id)
        created += 1

    await db.commit()
    if created:
        log.info("sync.reconcile.created", rule_id=str(rule_id), count=created)
    return created
