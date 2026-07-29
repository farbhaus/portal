"""Admin retry/dismiss for individual sync jobs — the remediation issue #53 was missing."""

import uuid
from datetime import UTC, datetime
from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.routes import sync_jobs as routes

CREDS = {"email": "admin@example.com", "password": "test-password"}
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


@pytest_asyncio.fixture(autouse=True)
async def _no_redis(monkeypatch: Any) -> list[uuid.UUID]:
    """Capture enqueues instead of needing a live ARQ pool."""
    enqueued: list[uuid.UUID] = []

    async def fake(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    monkeypatch.setattr(routes, "enqueue_sync_job", fake)
    return enqueued


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def _make_job(status: str = "dead_letter", **over: Any) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(name="r", source_config=SOURCE, destination_path="/data/out", enabled=True)
        db.add(rule)
        await db.flush()
        job = SyncJob(
            sync_rule_id=rule.id,
            frameio_file_id="file-1",
            status=status,
            error=over.get("error", "source never became ready"),
            retry_count=over.get("retry_count", 5),
            completed_at=over.get("completed_at"),
            source_wait_started_at=over.get("source_wait_started_at"),
        )
        db.add(job)
        await db.commit()
        await db.refresh(job)
        return job.id


async def test_retry_resets_the_job_and_queues_it(
    client: AsyncClient, _no_redis: list[uuid.UUID]
) -> None:
    await _login(client)
    # An expired readiness clock is the thing that made the old self-heal futile: a job re-picked
    # with a stale clock timed out again on its first tick. Retry must clear it.
    job_id = await _make_job(source_wait_started_at=datetime(2020, 1, 1, tzinfo=UTC))

    resp = await client.post(f"/api/sync-jobs/{job_id}/retry")
    assert resp.status_code == 200, resp.text
    assert resp.json()["status"] == "pending"

    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "pending"
        assert job.retry_count == 0
        assert job.error is None
        assert job.started_at is None
        assert job.completed_at is None
        assert job.source_wait_started_at is None
    assert _no_redis == [job_id]


async def test_dismiss_retires_the_job_and_keeps_the_error(client: AsyncClient) -> None:
    await _login(client)
    job_id = await _make_job(error="source never became ready: file status is 'created'")

    resp = await client.post(f"/api/sync-jobs/{job_id}/dismiss")
    assert resp.status_code == 200, resp.text

    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        # "skipped" is terminal: reconcile won't resurrect it, and the dashboard stops counting it.
        assert job.status == "skipped"
        assert job.completed_at is not None
        assert "never became ready" in (job.error or "")


async def test_actions_require_admin(client: AsyncClient) -> None:
    job_id = await _make_job()
    assert (await client.post(f"/api/sync-jobs/{job_id}/retry")).status_code == 401
    assert (await client.post(f"/api/sync-jobs/{job_id}/dismiss")).status_code == 401


async def test_unknown_job_is_404(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post(f"/api/sync-jobs/{uuid.uuid4()}/retry")
    assert resp.status_code == 404
