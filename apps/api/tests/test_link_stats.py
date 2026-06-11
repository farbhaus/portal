from datetime import UTC, datetime

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import (
    Destination,
    DownloadEvent,
    DownloadLink,
    DownloadSession,
    UploadLink,
    UploadSession,
)
from portal.db.session import get_sessionmaker
from portal.uploads.links import generate_token

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(UploadLink))
        await db.execute(delete(DownloadLink))
        await db.execute(delete(Destination))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_upload_link_stats(client: AsyncClient) -> None:
    await _login(client)
    now = datetime.now(UTC)
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        link = UploadLink(token=generate_token(), destination_id=dest.id)
        db.add(link)
        await db.flush()
        for size in (100, 200):
            db.add(
                UploadSession(
                    upload_link_id=link.id,
                    status="completed",
                    total_bytes=size,
                    completed_at=now,
                )
            )
        db.add(UploadSession(upload_link_id=link.id, status="in_progress"))
        await db.commit()
        link_id = str(link.id)

    stats = (await client.get(f"/api/upload-links/{link_id}/stats")).json()
    assert stats["uploads"] == 2
    assert stats["total_bytes"] == 300
    assert stats["last_activity"] is not None


async def test_download_link_events_and_stats(client: AsyncClient) -> None:
    await _login(client)
    async with get_sessionmaker()() as db:
        link = DownloadLink(
            token=generate_token(),
            source={"type": "file", "account_id": "a1", "file_id": "f1"},
        )
        db.add(link)
        await db.flush()
        now = datetime.now(UTC)
        s1 = DownloadSession(download_link_id=link.id, viewer_email="a@x.com", started_at=now)
        s2 = DownloadSession(download_link_id=link.id, viewer_email="b@x.com", started_at=now)
        db.add_all([s1, s2])
        await db.flush()
        db.add(DownloadEvent(download_session_id=s1.id, frameio_file_id="f1", file_name="a.mov"))
        db.add(DownloadEvent(download_session_id=s1.id, frameio_file_id="f2", file_name="b.mov"))
        db.add(DownloadEvent(download_session_id=s2.id, frameio_file_id="f1", file_name="a.mov"))
        await db.commit()
        link_id = str(link.id)

    events = (await client.get(f"/api/download-links/{link_id}/events")).json()
    assert len(events) == 3
    assert {"viewer_email", "file_name", "started_at"} <= set(events[0].keys())

    stats = (await client.get(f"/api/download-links/{link_id}/stats")).json()
    assert stats["downloads"] == 3
    assert stats["unique_viewers"] == 2
    assert stats["last_activity"] is not None


async def test_stats_require_admin(client: AsyncClient) -> None:
    import uuid

    fake = uuid.uuid4()
    assert (await client.get(f"/api/upload-links/{fake}/stats")).status_code == 401
    assert (await client.get(f"/api/download-links/{fake}/events")).status_code == 401
