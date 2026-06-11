import uuid
from typing import Any

import pytest_asyncio
from sqlalchemy import delete, select

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.sync import events

SOURCE = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "fld-1",
    "recursive": True,
}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.commit()


@pytest_asyncio.fixture
def enqueued(monkeypatch: Any) -> list[uuid.UUID]:
    seen: list[uuid.UUID] = []

    async def fake(job_id: uuid.UUID) -> None:
        seen.append(job_id)

    monkeypatch.setattr(events, "enqueue_sync_job", fake)
    return seen


async def _make_rule(*, enabled: bool = True, **over: Any) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(
            name="r",
            source_config={**SOURCE, **over},
            destination_path="/mnt/sync",
            enabled=enabled,
        )
        db.add(rule)
        await db.commit()
        await db.refresh(rule)
        return rule.id


async def _job_count() -> int:
    async with get_sessionmaker()() as db:
        from sqlalchemy import func

        return await db.scalar(select(func.count()).select_from(SyncJob)) or 0


async def test_trigger_creates_and_enqueues_jobs(enqueued: list[uuid.UUID]) -> None:
    await _make_rule()
    created = await events.trigger_sync_for_upload("a1", "fld-1", ["f1", "f2"])
    assert created == 2
    assert len(enqueued) == 2
    assert await _job_count() == 2


async def test_trigger_dedupes(enqueued: list[uuid.UUID]) -> None:
    await _make_rule()
    assert await events.trigger_sync_for_upload("a1", "fld-1", ["f1"]) == 1
    # Same file again (e.g. re-completion) doesn't create a second job.
    assert await events.trigger_sync_for_upload("a1", "fld-1", ["f1"]) == 0
    assert await _job_count() == 1


async def test_trigger_only_matches_same_folder(enqueued: list[uuid.UUID]) -> None:
    await _make_rule(folder_id="other-folder")
    assert await events.trigger_sync_for_upload("a1", "fld-1", ["f1"]) == 0
    assert await _job_count() == 0


async def test_trigger_skips_disabled_rule(enqueued: list[uuid.UUID]) -> None:
    await _make_rule(enabled=False)
    assert await events.trigger_sync_for_upload("a1", "fld-1", ["f1"]) == 0


async def test_trigger_empty_file_ids(enqueued: list[uuid.UUID]) -> None:
    await _make_rule()
    assert await events.trigger_sync_for_upload("a1", "fld-1", []) == 0
    assert enqueued == []
