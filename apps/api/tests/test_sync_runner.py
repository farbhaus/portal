import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest
import pytest_asyncio
from sqlalchemy import delete

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.storage.base import DestinationConfig, DownloadURL, RemoteFile
from portal.sync import runner
from portal.sync.download import DownloadResult


class _StubBackend:
    def __init__(
        self,
        files: list[RemoteFile],
        *,
        parent_id: str = "fld-1",
        status: str = "transcoded",
    ) -> None:
        self._files = files
        self._parent_id = parent_id
        self._status = status

    async def get_file(self, dest: DestinationConfig, file_id: str) -> RemoteFile:
        return RemoteFile(
            id=file_id,
            name=f"{file_id}.mov",
            size=100,
            parent_id=self._parent_id,
            status=self._status,
        )

    async def get_download_url(self, dest: DestinationConfig, file_id: str) -> DownloadURL:
        return DownloadURL(url=f"https://s3/{file_id}")

    async def list_files_recursive(self, dest: DestinationConfig) -> list[RemoteFile]:
        return self._files

    async def list_files_in_destination(self, dest: DestinationConfig) -> list[RemoteFile]:
        return self._files


def _source(recursive: bool = True) -> dict[str, Any]:
    return {
        "type": "frameio",
        "account_id": "a1",
        "workspace_id": "w1",
        "project_id": "p1",
        "folder_id": "fld-1",
        "recursive": recursive,
        "folder_name": "Dailies",
        "project_name": "Acme",
    }


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.commit()


async def _make_rule(dest_path: str, **over: Any) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(
            name="r",
            source_config={**_source(), **over.pop("source", {})},
            destination_path=dest_path,
            conflict_policy=over.get("conflict_policy", "rename_suffix"),
            path_template=over.get("path_template"),
            enabled=True,
        )
        db.add(rule)
        await db.commit()
        await db.refresh(rule)
        return rule.id


async def _make_job(rule_id: uuid.UUID, file_id: str = "file-1") -> uuid.UUID:
    async with get_sessionmaker()() as db:
        job = SyncJob(sync_rule_id=rule_id, frameio_file_id=file_id, status="pending")
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


async def test_execute_job_downloads_and_places(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path), path_template="{project}/{filename}")
    job_id = await _make_job(rule_id)
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend([]))  # type: ignore[arg-type]
        await db.commit()
    assert (tmp_path / "Acme" / "file-1.mov").read_bytes() == b"hello-bytes"
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "done"
        assert job.bytes == len(b"hello-bytes")
        assert job.sha256 == "deadbeef"


async def test_execute_job_out_of_scope_skipped(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    # non-recursive rule, file's parent is a different folder -> skipped
    rule_id = await _make_rule(str(tmp_path), source={"recursive": False})
    job_id = await _make_job(rule_id)
    backend = _StubBackend([], parent_id="some-other-folder")
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=backend)  # type: ignore[arg-type]
        await db.commit()
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "skipped"
    assert not any(tmp_path.iterdir())  # noqa: ASYNC240 - sync check in test


async def test_execute_job_conflict_skip(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    (tmp_path / "file-1.mov").write_bytes(b"existing")
    rule_id = await _make_rule(str(tmp_path), conflict_policy="skip")
    job_id = await _make_job(rule_id)
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend([]))  # type: ignore[arg-type]
        await db.commit()
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "skipped"
    assert (tmp_path / "file-1.mov").read_bytes() == b"existing"


async def test_reconcile_creates_and_enqueues(tmp_path: Path) -> None:
    rule_id = await _make_rule(str(tmp_path))
    await _make_job(rule_id, file_id="already")  # pre-existing job is not duplicated
    backend = _StubBackend(
        [
            RemoteFile(id="already", name="a"),
            RemoteFile(id="new-1", name="b"),
            RemoteFile(id="new-2", name="c"),
        ]
    )
    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        created = await runner.reconcile_rule(db, rule_id, backend=backend, enqueue=enqueue)  # type: ignore[arg-type]
    assert created == 2
    assert len(enqueued) == 2
    async with get_sessionmaker()() as db:
        from sqlalchemy import func, select

        total = await db.scalar(select(func.count()).select_from(SyncJob))
    assert total == 3


async def test_execute_job_waits_when_source_not_ready(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    job_id = await _make_job(rule_id)
    backend = _StubBackend([], status="transcoding")  # not yet downloadable
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        with pytest.raises(runner.SourceNotReady):
            await runner.execute_job(db, job, backend=backend)  # type: ignore[arg-type]
    assert not any(tmp_path.iterdir())  # noqa: ASYNC240 - sync check in test


async def test_execute_job_proceeds_when_transcoded(tmp_path: Path, monkeypatch: Any) -> None:
    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    job_id = await _make_job(rule_id)
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_StubBackend([], status="transcoded"))  # type: ignore[arg-type]
        await db.commit()
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "done"


async def test_reconcile_repicks_dead_letter(tmp_path: Path) -> None:
    rule_id = await _make_rule(str(tmp_path))
    # A previously dead-lettered file should be reset + re-enqueued by the next sweep.
    async with get_sessionmaker()() as db:
        job = SyncJob(
            sync_rule_id=rule_id,
            frameio_file_id="stuck",
            status="dead_letter",
            retry_count=5,
            error="boom",
        )
        db.add(job)
        await db.commit()
        dead_id = job.id
    backend = _StubBackend([RemoteFile(id="stuck", name="s")])
    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        created = await runner.reconcile_rule(db, rule_id, backend=backend, enqueue=enqueue)  # type: ignore[arg-type]
    assert created == 1
    assert enqueued == [dead_id]
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, dead_id)
        assert job.status == "pending"
        assert job.retry_count == 0
        assert job.error is None


async def test_reconcile_repicks_orphaned_running(tmp_path: Path) -> None:
    # A job left "running" long ago (worker killed mid-download) is orphaned -> re-picked.
    from datetime import UTC, datetime, timedelta

    rule_id = await _make_rule(str(tmp_path))
    async with get_sessionmaker()() as db:
        job = SyncJob(
            sync_rule_id=rule_id,
            frameio_file_id="orphan",
            status="running",
            started_at=datetime.now(UTC) - timedelta(days=2),
        )
        db.add(job)
        await db.commit()
        orphan_id = job.id
    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        created = await runner.reconcile_rule(
            db, rule_id, backend=_StubBackend([RemoteFile(id="orphan", name="o")]), enqueue=enqueue  # type: ignore[arg-type]
        )
    assert created == 1
    assert enqueued == [orphan_id]
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, orphan_id)
        assert job.status == "pending"


async def test_reconcile_leaves_fresh_running_alone(tmp_path: Path) -> None:
    # A job that's actively running (recent started_at) must NOT be re-picked mid-download.
    from datetime import UTC, datetime

    rule_id = await _make_rule(str(tmp_path))
    async with get_sessionmaker()() as db:
        db.add(
            SyncJob(
                sync_rule_id=rule_id,
                frameio_file_id="busy",
                status="running",
                started_at=datetime.now(UTC),
            )
        )
        await db.commit()
    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        created = await runner.reconcile_rule(
            db, rule_id, backend=_StubBackend([RemoteFile(id="busy", name="b")]), enqueue=enqueue  # type: ignore[arg-type]
        )
    assert created == 0
    assert enqueued == []


async def test_reconcile_leaves_done_alone(tmp_path: Path) -> None:
    rule_id = await _make_rule(str(tmp_path))
    async with get_sessionmaker()() as db:
        db.add(SyncJob(sync_rule_id=rule_id, frameio_file_id="ok", status="done", bytes=1))
        await db.commit()
    backend = _StubBackend([RemoteFile(id="ok", name="o")])
    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        created = await runner.reconcile_rule(db, rule_id, backend=backend, enqueue=enqueue)  # type: ignore[arg-type]
    assert created == 0
    assert enqueued == []


async def test_execute_job_source_gone_skipped(tmp_path: Path, monkeypatch: Any) -> None:
    from portal.frameio.client import FrameioNotFound

    _patch_download(monkeypatch)
    rule_id = await _make_rule(str(tmp_path))
    job_id = await _make_job(rule_id)

    class _GoneBackend(_StubBackend):
        async def get_file(self, dest: DestinationConfig, file_id: str) -> RemoteFile:
            raise FrameioNotFound("Frame.io entity not found: get_file_detail")

    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        await runner.execute_job(db, job, backend=_GoneBackend([]))  # type: ignore[arg-type]
        await db.commit()
    async with get_sessionmaker()() as db:
        job = await db.get(SyncJob, job_id)
        assert job.status == "skipped"
        assert job.error is not None and "no longer exists" in job.error
    assert not any(tmp_path.iterdir())  # noqa: ASYNC240 - sync check in test


async def test_reclaim_orphaned_running_requeues_and_leaves_others() -> None:
    rule_id = await _make_rule("/dest")
    async with get_sessionmaker()() as db:
        running = SyncJob(sync_rule_id=rule_id, frameio_file_id="stuck", status="running")
        running.started_at = datetime.now(UTC)
        done = SyncJob(sync_rule_id=rule_id, frameio_file_id="ok", status="done")
        db.add_all([running, done])
        await db.commit()
        running_id = running.id
        done_id = done.id

    enqueued: list[uuid.UUID] = []

    async def enqueue(job_id: uuid.UUID) -> None:
        enqueued.append(job_id)

    async with get_sessionmaker()() as db:
        count = await runner.reclaim_orphaned_running(db, enqueue)

    assert count == 1
    assert enqueued == [running_id]
    async with get_sessionmaker()() as db:
        reclaimed = await db.get(SyncJob, running_id)
        untouched = await db.get(SyncJob, done_id)
        assert reclaimed is not None and reclaimed.status == "pending"
        assert reclaimed.started_at is None
        assert untouched is not None and untouched.status == "done"
