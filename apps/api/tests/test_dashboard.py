from datetime import UTC, datetime, timedelta

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import (
    Destination,
    DownloadEvent,
    DownloadLink,
    DownloadSession,
    SyncJob,
    SyncRule,
    UploadLink,
    UploadSession,
)
from portal.db.session import get_sessionmaker
from portal.uploads.links import generate_token

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.execute(delete(DownloadLink))
        await db.execute(delete(UploadLink))
        await db.execute(delete(Destination))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_transfers_active_recent_and_health(client: AsyncClient) -> None:
    await _login(client)
    now = datetime.now(UTC)
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        ul = UploadLink(
            token=generate_token(), destination_id=dest.id, brand_display_name="Acme"
        )
        db.add(ul)
        await db.flush()
        # one in-flight, one completed
        db.add(
            UploadSession(
                upload_link_id=ul.id,
                status="in_progress",
                uploader_email="up@x.com",
                started_at=now,
            )
        )
        db.add(
            UploadSession(
                upload_link_id=ul.id,
                status="completed",
                total_bytes=500,
                completed_at=now,
            )
        )

        dl = DownloadLink(
            token=generate_token(),
            source={"type": "file", "account_id": "a1", "file_id": "f1"},
        )
        db.add(dl)
        await db.flush()
        sess = DownloadSession(download_link_id=dl.id, viewer_email="v@x.com", started_at=now)
        db.add(sess)
        await db.flush()
        db.add(
            DownloadEvent(
                download_session_id=sess.id,
                frameio_file_id="f1",
                file_name="clip.mov",
                completed=True,
            )
        )

        rule = SyncRule(name="Daily", source_config={"folder_id": "x"}, destination_path="/nas")
        db.add(rule)
        await db.flush()
        db.add(SyncJob(sync_rule_id=rule.id, frameio_file_id="j1", status="waiting"))
        db.add(SyncJob(sync_rule_id=rule.id, frameio_file_id="j2", status="dead_letter"))
        db.add(
            SyncJob(
                sync_rule_id=rule.id,
                frameio_file_id="j3",
                status="done",
                completed_at=now,
            )
        )
        await db.commit()

    data = (await client.get("/api/dashboard/transfers")).json()

    assert len(data["active_uploads"]) == 1
    assert data["active_uploads"][0]["who"] == "up@x.com"
    assert data["active_uploads"][0]["brand"] == "Acme"

    kinds = {r["kind"] for r in data["recent"]}
    assert {"upload", "download", "sync"} <= kinds

    health = data["sync_health"]
    assert health["waiting"] == 1
    assert health["dead_letter"] == 1
    assert health["done_24h"] == 1


async def test_done_24h_excludes_old(client: AsyncClient) -> None:
    await _login(client)
    old = datetime.now(UTC) - timedelta(days=2)
    async with get_sessionmaker()() as db:
        rule = SyncRule(name="R", source_config={"folder_id": "x"}, destination_path="/nas")
        db.add(rule)
        await db.flush()
        db.add(
            SyncJob(
                sync_rule_id=rule.id, frameio_file_id="old", status="done", completed_at=old
            )
        )
        await db.commit()

    health = (await client.get("/api/dashboard/transfers")).json()["sync_health"]
    assert health["done_24h"] == 0


async def test_history_pagination_and_filter(client: AsyncClient) -> None:
    await _login(client)
    base = datetime.now(UTC)
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        ul = UploadLink(token=generate_token(), destination_id=dest.id, brand_display_name="Up")
        db.add(ul)
        await db.flush()
        dl = DownloadLink(
            token=generate_token(), source={"type": "file", "account_id": "a1", "file_id": "f1"}
        )
        db.add(dl)
        await db.flush()
        sess = DownloadSession(download_link_id=dl.id, viewer_email="v@x.com", started_at=base)
        db.add(sess)
        await db.flush()
        # 3 completed uploads interleaved with 3 downloads, all at distinct descending times.
        for i in range(3):
            db.add(
                UploadSession(
                    upload_link_id=ul.id,
                    status="completed",
                    total_bytes=i,
                    completed_at=base - timedelta(minutes=i),
                )
            )
            db.add(
                DownloadEvent(
                    download_session_id=sess.id,
                    frameio_file_id=f"f{i}",
                    file_name=f"c{i}.mov",
                    completed=True,
                    started_at=base - timedelta(minutes=i, seconds=30),
                )
            )
        await db.commit()

    # All transfers (6), newest-first, paginated 4 at a time.
    p1 = (await client.get("/api/dashboard/history?limit=4&offset=0")).json()
    assert len(p1["items"]) == 4 and p1["has_more"] is True
    assert p1["items"][0]["kind"] == "upload"  # up0 is newest
    p2 = (await client.get("/api/dashboard/history?limit=4&offset=4")).json()
    assert len(p2["items"]) == 2 and p2["has_more"] is False

    # kind filter narrows to one source.
    ups = (await client.get("/api/dashboard/history?kind=upload")).json()
    assert len(ups["items"]) == 3 and all(r["kind"] == "upload" for r in ups["items"])
    downs = (await client.get("/api/dashboard/history?kind=download")).json()
    assert len(downs["items"]) == 3 and all(r["kind"] == "download" for r in downs["items"])


async def test_transfers_requires_admin(client: AsyncClient) -> None:
    assert (await client.get("/api/dashboard/transfers")).status_code == 401
    assert (await client.get("/api/dashboard/history")).status_code == 401
