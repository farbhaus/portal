from datetime import UTC, datetime, timedelta
from typing import Any

import pytest_asyncio
from sqlalchemy import delete, func, select

from portal.db.models import (
    AuditLog,
    Destination,
    DownloadLink,
    DownloadSession,
    SyncJob,
    SyncRule,
    UploadLink,
    UploadSession,
    WebhookEvent,
)
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.services.retention import purge_expired
from portal.uploads.links import generate_token

NOW = datetime(2026, 6, 21, 12, 0, tzinfo=UTC)
OLD = NOW - timedelta(days=120)
RECENT = NOW - timedelta(days=1)


@pytest_asyncio.fixture(autouse=True)
async def _clean() -> None:
    async with get_sessionmaker()() as db:
        for model in (WebhookEvent, AuditLog, Destination, DownloadLink, SyncRule):
            await db.execute(delete(model))  # links/rules cascade to their sessions/jobs
        await db.commit()


async def _count(model: type[Any]) -> int:
    async with get_sessionmaker()() as db:
        return await db.scalar(select(func.count()).select_from(model)) or 0


async def _seed() -> None:
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        ulink = UploadLink(token=generate_token(), destination=dest)
        dlink = DownloadLink(token=generate_token(), source={"type": "file"})
        rule = SyncRule(name="R", source_config={"type": "frameio"}, destination_path="/x")
        db.add_all([dest, ulink, dlink, rule])
        await db.flush()
        for ts in (OLD, RECENT):
            db.add(UploadSession(upload_link_id=ulink.id, status="completed", created_at=ts))
            db.add(DownloadSession(download_link_id=dlink.id, created_at=ts))
            db.add(WebhookEvent(source="frameio", raw={}, received_at=ts))
            db.add(AuditLog(action="test.event", created_at=ts))
            db.add(SyncJob(sync_rule_id=rule.id, frameio_file_id=f"f-{ts.day}", created_at=ts))
        await db.commit()


async def test_purge_deletes_old_keeps_recent(monkeypatch: Any) -> None:
    s = get_settings()
    monkeypatch.setattr(s, "retention_webhook_events_days", 30)
    monkeypatch.setattr(s, "retention_sessions_days", 30)
    monkeypatch.setattr(s, "retention_audit_log_days", 30)
    await _seed()

    async with get_sessionmaker()() as db:
        deleted = await purge_expired(db, now=NOW)

    assert deleted == {
        "webhook_events": 1,
        "upload_sessions": 1,
        "download_sessions": 1,
        "audit_log": 1,
    }
    # One recent row of each survives.
    assert await _count(WebhookEvent) == 1
    assert await _count(UploadSession) == 1
    assert await _count(DownloadSession) == 1
    assert await _count(AuditLog) == 1
    # Sync jobs are the copy-once ledger and must never be pruned.
    assert await _count(SyncJob) == 2


async def test_window_zero_disables_sweep(monkeypatch: Any) -> None:
    s = get_settings()
    monkeypatch.setattr(s, "retention_webhook_events_days", 0)
    monkeypatch.setattr(s, "retention_sessions_days", 0)
    monkeypatch.setattr(s, "retention_audit_log_days", 0)
    await _seed()

    async with get_sessionmaker()() as db:
        deleted = await purge_expired(db, now=NOW)

    assert deleted == {}
    assert await _count(WebhookEvent) == 2
    assert await _count(AuditLog) == 2
    assert await _count(UploadSession) == 2
