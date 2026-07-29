import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

import httpx
import pytest_asyncio
from sqlalchemy import delete

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.storage.base import DestinationConfig, RemoteFile
from portal.sync import worker
from portal.sync.worker import _unwrap_error


def test_unwrap_plain_exception() -> None:
    assert _unwrap_error(ValueError("boom")) == "ValueError: boom"


def test_unwrap_exception_group() -> None:
    # A TaskGroup wraps the real cause in an ExceptionGroup; the recorded error should name it.
    err = httpx.HTTPStatusError(
        "403", request=httpx.Request("GET", "https://x"), response=httpx.Response(403)
    )
    group = BaseExceptionGroup("unhandled errors in a TaskGroup", [err])
    out = _unwrap_error(group)
    assert "HTTPStatusError" in out
    assert "403" in out


def test_unwrap_nested_group() -> None:
    inner = BaseExceptionGroup("inner", [OSError("disk full")])
    outer = BaseExceptionGroup("outer", [inner])
    assert "OSError: disk full" in _unwrap_error(outer)


# --- the source-readiness window ------------------------------------------------------------------


class _NotReadyBackend:
    """A Frame.io file that never finishes uploading — it stays in status "created" forever."""

    async def get_file(self, dest: DestinationConfig, file_id: str) -> RemoteFile:
        return RemoteFile(
            id=file_id,
            name="clip.mov",
            size=10,
            parent_id="fld-1",
            relative_dir="",
            status="created",
        )


class _StubRedis:
    def __init__(self) -> None:
        self.enqueued: list[str] = []

    async def enqueue_job(self, fn: str, *args: Any, **kw: Any) -> None:
        self.enqueued.append(fn)


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.commit()


async def _make_job(tmp_dest: str, **over: Any) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(
            name="r",
            source_config={
                "type": "frameio",
                "account_id": "a1",
                "workspace_id": "w1",
                "project_id": "p1",
                "folder_id": "fld-1",
                "recursive": True,
            },
            destination_path=tmp_dest,
            enabled=True,
        )
        db.add(rule)
        await db.flush()
        job = SyncJob(sync_rule_id=rule.id, frameio_file_id="file-1", status="pending", **over)
        db.add(job)
        await db.commit()
        await db.refresh(job)
        return job.id


async def test_first_wait_starts_the_readiness_clock(tmp_path: Any, monkeypatch: Any) -> None:
    monkeypatch.setattr(worker, "FrameioStorageBackend", _NotReadyBackend)
    job_id = await _make_job(str(tmp_path))

    result = await worker.run_sync_job({"http": None, "redis": _StubRedis()}, str(job_id))

    assert result == "waiting"
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "waiting"
        assert job.source_wait_started_at is not None  # the window starts now, not at created_at


async def test_expired_window_skips_rather_than_dead_letters(
    tmp_path: Any, monkeypatch: Any
) -> None:
    """An upload abandoned on Frame.io isn't a failure needing attention — it's a terminal skip.

    Dead-lettering it kept the dashboard alert lit forever and had reconcile re-pick it on every
    pass, which is what issue #53 was actually watching happen every 15 minutes.
    """
    monkeypatch.setattr(worker, "FrameioStorageBackend", _NotReadyBackend)
    long_ago = datetime.now(UTC) - timedelta(days=7)
    job_id = await _make_job(str(tmp_path), source_wait_started_at=long_ago)

    result = await worker.run_sync_job({"http": None, "redis": _StubRedis()}, str(job_id))

    assert result == "skipped"
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "skipped"
        assert job.completed_at is not None
        assert "never became ready" in (job.error or "")


async def test_a_retried_job_gets_a_full_window(tmp_path: Any, monkeypatch: Any) -> None:
    """The regression behind "manual sync does nothing": a job re-picked long after it was created
    used to measure its wait from created_at and expire on its first tick."""
    monkeypatch.setattr(worker, "FrameioStorageBackend", _NotReadyBackend)
    # Created a week ago, but its clock was cleared by a retry/reconcile reset.
    job_id = await _make_job(str(tmp_path), source_wait_started_at=None)
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        job.created_at = datetime.now(UTC) - timedelta(days=7)
        await db.commit()

    result = await worker.run_sync_job({"http": None, "redis": _StubRedis()}, str(job_id))

    assert result == "waiting"  # not "skipped" — the ancient created_at is irrelevant now
