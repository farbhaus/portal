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


async def test_transfers_stale_and_abandoned_uploads(client: AsyncClient) -> None:
    """Issue #41: dead sessions must not sit in "Uploading now" forever, and abandoned ones
    that registered files surface as "incomplete" in the feeds."""
    await _login(client)
    now = datetime.now(UTC)
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        ul = UploadLink(token=generate_token(), destination_id=dest.id, brand_display_name="Acme")
        db.add(ul)
        await db.flush()
        # Live upload: heartbeat progress + planned size from its registered files.
        db.add(
            UploadSession(
                upload_link_id=ul.id,
                status="in_progress",
                uploaded_bytes=250,
                frameio_asset_ids=[
                    {"file_id": "f1", "name": "a.mov", "size": 100},
                    {"file_id": "f2", "name": "b.mov", "size": 250},
                ],
                started_at=now,
                last_activity_at=now,
            )
        )
        # Ancient in_progress with no liveness signal: hidden by the 24h belt-and-braces bound
        # even if the abandon sweep never ran.
        db.add(
            UploadSession(
                upload_link_id=ul.id, status="in_progress", started_at=now - timedelta(days=2)
            )
        )
        # Abandoned mid-upload with a registered file: shows as an "incomplete" transfer.
        db.add(
            UploadSession(
                upload_link_id=ul.id,
                status="abandoned",
                uploader_name="Ze",
                uploaded_bytes=42,
                frameio_asset_ids=[{"file_id": "f3", "name": "c.mov", "size": 500}],
                started_at=now - timedelta(hours=2),
                last_activity_at=now - timedelta(hours=1),
            )
        )
        # Abandoned before any file was registered: pure noise, appears nowhere.
        db.add(
            UploadSession(
                upload_link_id=ul.id, status="abandoned", started_at=now - timedelta(hours=1)
            )
        )
        await db.commit()

    data = (await client.get("/api/dashboard/transfers")).json()

    assert len(data["active_uploads"]) == 1
    active = data["active_uploads"][0]
    assert active["uploaded_bytes"] == 250
    assert active["total_bytes"] == 350  # summed from registered files pre-completion

    ups = [r for r in data["recent"] if r["kind"] == "upload"]
    assert len(ups) == 1
    assert ups[0]["status"] == "incomplete"
    assert ups[0]["who"] == "Ze"
    assert ups[0]["bytes"] == 42  # what actually made it up, not the planned size

    hist = (await client.get("/api/dashboard/history?kind=upload")).json()
    assert [r["status"] for r in hist["items"]] == ["incomplete"]


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
