"""The destination guard: a sync rule must not write into an unmounted share's mountpoint."""

import uuid
from pathlib import Path
from typing import Any

import pytest
import pytest_asyncio
from sqlalchemy import delete

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.storage.base import DestinationConfig, DownloadURL, RemoteFile
from portal.sync import runner, worker
from portal.sync.destination import (
    MARKER_NAME,
    DestinationUnavailable,
    arm_destination,
    check_destination,
)
from portal.sync.download import DownloadResult


class _StubBackend:
    async def get_file(self, dest: DestinationConfig, file_id: str) -> RemoteFile:
        return RemoteFile(
            id=file_id,
            name=f"{file_id}.mov",
            size=100,
            parent_id="fld-1",
            relative_dir="",
            status="transcoded",
        )

    async def get_download_url(self, dest: DestinationConfig, file_id: str) -> DownloadURL:
        return DownloadURL(url=f"https://s3/{file_id}")


def _source() -> dict[str, Any]:
    return {
        "type": "frameio",
        "account_id": "a1",
        "workspace_id": "w1",
        "project_id": "p1",
        "folder_id": "fld-1",
        "recursive": True,
        "folder_name": "Dailies",
        "project_name": "Acme",
    }


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.commit()


async def _make_rule(dest_path: str) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(
            name="r",
            source_config=_source(),
            destination_path=dest_path,
            conflict_policy="rename_suffix",
            enabled=True,
        )
        db.add(rule)
        await db.commit()
        await db.refresh(rule)
        return rule.id


async def _make_job(
    rule_id: uuid.UUID, file_id: str = "file-1", status: str = "pending"
) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        job = SyncJob(sync_rule_id=rule_id, frameio_file_id=file_id, status=status)
        db.add(job)
        await db.commit()
        await db.refresh(job)
        return job.id


def _patch_download(monkeypatch: Any, payload: bytes = b"hello-bytes") -> None:
    async def fake(url: str, size: int | None, tmp: Path, **kw: Any) -> DownloadResult:
        tmp.parent.mkdir(parents=True, exist_ok=True)  # noqa: ASYNC240 - test stub, sync I/O is fine
        tmp.write_bytes(payload)  # noqa: ASYNC240
        return DownloadResult(bytes_written=len(payload), sha256="deadbeef")

    monkeypatch.setattr(runner, "download_to_file", fake)


# --- the guard itself ---------------------------------------------------------------------------


def test_unarmed_destination_passes_without_marker(tmp_path: Path) -> None:
    check_destination(tmp_path, armed=False)  # a rule that never synced can't be judged yet


def test_armed_destination_without_marker_raises(tmp_path: Path) -> None:
    with pytest.raises(DestinationUnavailable):
        check_destination(tmp_path, armed=True)


def test_armed_destination_without_marker_but_with_contents_passes(tmp_path: Path) -> None:
    # A rule that predates the marker: what it synced before is still there, so the share is up.
    (tmp_path / "an-earlier-sync.mov").write_bytes(b"x")
    check_destination(tmp_path, armed=True)


def test_armed_destination_that_does_not_exist_raises(tmp_path: Path) -> None:
    with pytest.raises(DestinationUnavailable):
        check_destination(tmp_path / "not-there", armed=True)


def test_armed_destination_with_marker_passes(tmp_path: Path) -> None:
    arm_destination(tmp_path)
    check_destination(tmp_path, armed=True)


def test_arm_is_idempotent_and_keeps_existing_marker(tmp_path: Path) -> None:
    (tmp_path / MARKER_NAME).write_text("operator's own note")
    arm_destination(tmp_path)
    assert (tmp_path / MARKER_NAME).read_text() == "operator's own note"


def test_arm_on_unwritable_destination_does_not_raise(tmp_path: Path) -> None:
    # A read-only export must not fail an otherwise good sync — it just leaves the rule unarmed.
    dest = tmp_path / "ro"
    dest.mkdir(mode=0o500)
    arm_destination(dest)
    assert not (dest / MARKER_NAME).exists()


# --- the runner ---------------------------------------------------------------------------------


async def test_first_sync_arms_the_destination(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    job_id = await _make_job(rule_id)
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend())  # type: ignore[arg-type]
        await db.commit()
    assert (tmp_path / "file-1.mov").read_bytes() == b"hello-bytes"
    assert (tmp_path / MARKER_NAME).exists()


async def test_armed_rule_refuses_to_write_when_marker_is_gone(
    tmp_path: Path, monkeypatch: Any
) -> None:
    # Exactly the incident: the share unmounted, leaving an empty but writable mountpoint behind.
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    await _make_job(rule_id, file_id="already-synced", status="done")
    job_id = await _make_job(rule_id, file_id="file-2")
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        with pytest.raises(DestinationUnavailable):
            await runner.execute_job(db, job, backend=_StubBackend())  # type: ignore[arg-type]
    assert list(tmp_path.iterdir()) == []  # noqa: ASYNC240 - sync check in test


async def test_rule_that_predates_the_marker_arms_itself(tmp_path: Path, monkeypatch: Any) -> None:
    """Upgrade path: already-synced rules must not deadlock waiting for a marker they never got."""
    _patch_download(monkeypatch)
    (tmp_path / "synced-before-the-guard.mov").write_bytes(b"x")
    rule_id = await _make_rule(str(tmp_path))
    await _make_job(rule_id, file_id="already-synced", status="done")
    job_id = await _make_job(rule_id, file_id="file-2")
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend())  # type: ignore[arg-type]
        await db.commit()
    assert (tmp_path / "file-2.mov").read_bytes() == b"hello-bytes"
    assert (tmp_path / MARKER_NAME).exists()  # armed from now on


async def test_armed_rule_writes_when_the_share_is_mounted(
    tmp_path: Path, monkeypatch: Any
) -> None:
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    await _make_job(rule_id, file_id="already-synced", status="done")
    arm_destination(tmp_path)  # marker visible => share is there
    job_id = await _make_job(rule_id, file_id="file-2")
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend())  # type: ignore[arg-type]
        await db.commit()
    assert (tmp_path / "file-2.mov").read_bytes() == b"hello-bytes"


# --- the worker -----------------------------------------------------------------------------------


class _StubRedis:
    def __init__(self) -> None:
        self.enqueued: list[tuple[str, Any]] = []

    async def enqueue_job(self, fn: str, *args: Any, **kw: Any) -> None:
        self.enqueued.append((fn, kw.get("_defer_by")))


async def test_worker_waits_instead_of_dead_lettering(tmp_path: Path, monkeypatch: Any) -> None:
    """An unmounted share must not burn the retry budget — it comes back, and the job resumes."""
    _patch_download(monkeypatch)
    monkeypatch.setattr(worker, "FrameioStorageBackend", _StubBackend)
    rule_id = await _make_rule(str(tmp_path))
    await _make_job(rule_id, file_id="already-synced", status="done")
    job_id = await _make_job(rule_id, file_id="file-2")

    redis = _StubRedis()
    result = await worker.run_sync_job({"http": None, "redis": redis}, str(job_id))

    assert result == "waiting"
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "waiting"
        assert job.retry_count == 0
        assert MARKER_NAME in (job.error or "")
    assert len(redis.enqueued) == 1  # re-queued for a later attempt
    assert list(tmp_path.iterdir()) == []  # noqa: ASYNC240 - sync check in test
