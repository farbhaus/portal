"""Turn ingested Frame.io webhook events into sync jobs.

A verified webhook event is persisted by the ingestion endpoint (``routes.webhooks``); this is
the processor that interprets it. For a file-arrival event it finds the enabled sync rules whose
source matches and creates one pending ``sync_job`` per (rule, file). The job is the unit the
Step 11 worker downloads.

Folder-level matching: Frame.io webhooks are workspace-scoped, and the delivery body names the
file's account/workspace/project but not its parent folder. So we match on those three ids here
and leave the precise folder-membership check to the Step 11 worker, which fetches file metadata
(including parent_id) before downloading and skips files outside the rule's folder.
"""

from __future__ import annotations

import uuid
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import SyncJob, SyncRule, WebhookEvent
from portal.frameio.webhooks import SYNC_TRIGGER_EVENTS, WebhookPayload, parse_webhook_payload
from portal.lib.logging import get_logger

log = get_logger("sync.events")


def rule_matches(source_config: dict[str, object], payload: WebhookPayload) -> bool:
    """True if a sync rule's source is in the same account/workspace/project as the event.

    All three ids must be present and equal — a missing id never matches, so an event without a
    project can't accidentally fan out to every rule.
    """
    for key, value in (
        ("account_id", payload.account_id),
        ("workspace_id", payload.workspace_id),
        ("project_id", payload.project_id),
    ):
        if value is None or source_config.get(key) != value:
            return False
    return True


def is_sync_trigger(payload: WebhookPayload) -> bool:
    """True if the event is a file arrival worth syncing."""
    return payload.type in SYNC_TRIGGER_EVENTS and payload.resource_type == "file"


async def process_event(
    db: AsyncSession,
    event_id: uuid.UUID,
    on_job_created: Callable[[uuid.UUID], Awaitable[None]] | None = None,
) -> int:
    """Process one persisted webhook event; return the number of sync jobs created.

    For each created job, ``on_job_created`` (if given) is awaited with the new job id — the worker
    uses it to enqueue the download. Idempotent: already-processed events are skipped, and a
    (rule, file) pair that already has a job is not duplicated, so webhook retries and the
    file.ready / file.upload.completed pair don't create extra jobs.
    """
    event = await db.get(WebhookEvent, event_id)
    if event is None or event.processed:
        return 0

    created_ids: list[uuid.UUID] = []
    try:
        payload = parse_webhook_payload(event.raw)
        if is_sync_trigger(payload) and payload.resource_id is not None:
            created_ids = await _create_jobs(db, payload, payload.resource_id)
        event.processed = True
        event.processed_at = datetime.now(UTC)
        event.error = None
    except Exception as exc:  # noqa: BLE001 - record and stop; reconciliation is the safety net
        # Mark processed so a malformed event doesn't loop forever; the reconciliation job catches
        # anything genuinely missed.
        event.processed = True
        event.processed_at = datetime.now(UTC)
        event.error = str(exc)
        log.warning("webhook.process_failed", event_id=str(event_id), error=str(exc))

    await db.commit()
    if created_ids:
        log.info("webhook.jobs_created", event_id=str(event_id), count=len(created_ids))
        if on_job_created is not None:
            for job_id in created_ids:
                await on_job_created(job_id)
    return len(created_ids)


async def _create_jobs(
    db: AsyncSession, payload: WebhookPayload, file_id: str
) -> list[uuid.UUID]:
    result = await db.execute(select(SyncRule).where(SyncRule.enabled.is_(True)))
    created: list[uuid.UUID] = []
    for rule in result.scalars().all():
        if not rule_matches(rule.source_config, payload):
            continue
        existing = await db.scalar(
            select(SyncJob.id).where(
                SyncJob.sync_rule_id == rule.id,
                SyncJob.frameio_file_id == file_id,
            )
        )
        if existing is not None:
            continue
        job = SyncJob(sync_rule_id=rule.id, frameio_file_id=file_id, status="pending")
        db.add(job)
        await db.flush()
        created.append(job.id)
    return created
