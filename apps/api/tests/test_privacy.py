from datetime import UTC, datetime, timedelta

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, func, select

from portal.db.models import (
    Destination,
    DownloadEvent,
    DownloadLink,
    DownloadSession,
    EmailVerification,
    UploadLink,
    UploadSession,
)
from portal.db.session import get_sessionmaker
from portal.uploads.links import generate_token

CREDS = {"email": "admin@example.com", "password": "test-password"}
TARGET = "Client@Example.com"  # stored mixed-case; looked up case-insensitively
OTHER = "someone-else@example.com"


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(EmailVerification))
        await db.execute(delete(DownloadLink))
        await db.execute(delete(UploadLink))
        await db.execute(delete(Destination))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def _seed() -> None:
    """An upload + a download (with an event) + a verification for TARGET; decoys for OTHER."""
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        ul = UploadLink(token=generate_token(), destination_id=dest.id)
        db.add(ul)
        await db.flush()
        db.add(
            UploadSession(
                upload_link_id=ul.id,
                uploader_name="Client Name",
                uploader_email=TARGET,
                uploader_message="hello",
                status="completed",
                completed_at=datetime.now(UTC),
            )
        )
        db.add(UploadSession(upload_link_id=ul.id, uploader_email=OTHER, status="completed"))

        dl = DownloadLink(
            token=generate_token(), source={"type": "file", "account_id": "a1", "file_id": "f1"}
        )
        db.add(dl)
        await db.flush()
        sess = DownloadSession(
            download_link_id=dl.id,
            viewer_name="Client Name",
            viewer_email=TARGET,
            ip="203.0.113.5",
            user_agent="curl/8",
            started_at=datetime.now(UTC),
        )
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
        db.add(
            EmailVerification(
                email=TARGET.lower(),
                code_hash="x" * 64,
                expires_at=datetime.now(UTC) + timedelta(minutes=10),
            )
        )
        await db.commit()


async def test_export_subject(client: AsyncClient) -> None:
    await _login(client)
    await _seed()
    data = (await client.get("/api/privacy/subject?email=client@example.com")).json()
    assert len(data["upload_sessions"]) == 1
    assert data["upload_sessions"][0]["uploader_email"] == TARGET
    assert data["upload_sessions"][0]["uploader_message"] == "hello"
    assert len(data["download_sessions"]) == 1
    assert data["download_sessions"][0]["ip"] == "203.0.113.5"
    assert len(data["download_sessions"][0]["events"]) == 1
    assert len(data["email_verifications"]) == 1
    # A code hash is never exported.
    assert "code_hash" not in data["email_verifications"][0]


async def test_erase_anonymize_keeps_rows(client: AsyncClient) -> None:
    await _login(client)
    await _seed()
    r = (
        await client.post(
            "/api/privacy/subject/erase", json={"email": "client@example.com", "mode": "anonymize"}
        )
    ).json()
    assert (
        r["upload_sessions"] == 1 and r["download_sessions"] == 1 and r["email_verifications"] == 1
    )

    async with get_sessionmaker()() as db:
        # Target row survives but its PII is nulled (the decoy for OTHER keeps its email).
        anon = (
            (await db.execute(select(UploadSession).where(UploadSession.uploader_email.is_(None))))
            .scalars()
            .all()
        )
        assert len(anon) == 1
        dl = (await db.execute(select(DownloadSession))).scalars().one()
        assert dl.viewer_email is None and dl.ip is None and dl.user_agent is None
        # Its download event still exists (row kept).
        assert (await db.scalar(select(func.count()).select_from(DownloadEvent))) == 1
        # The decoy upload for OTHER is untouched.
        assert (
            await db.scalar(
                select(func.count())
                .select_from(UploadSession)
                .where(UploadSession.uploader_email == OTHER)
            )
        ) == 1
        # Pending verification is gone.
        assert (await db.scalar(select(func.count()).select_from(EmailVerification))) == 0


async def test_erase_delete_removes_rows_and_events(client: AsyncClient) -> None:
    await _login(client)
    await _seed()
    r = (
        await client.post(
            "/api/privacy/subject/erase", json={"email": "client@example.com", "mode": "delete"}
        )
    ).json()
    assert r["download_sessions"] == 1

    async with get_sessionmaker()() as db:
        # Target download session + its event are gone; the decoy upload remains.
        assert (await db.scalar(select(func.count()).select_from(DownloadSession))) == 0
        assert (await db.scalar(select(func.count()).select_from(DownloadEvent))) == 0
        assert (
            await db.scalar(select(func.count()).select_from(UploadSession))
        ) == 1  # OTHER decoy
        assert (await db.scalar(select(func.count()).select_from(EmailVerification))) == 0


async def test_requires_admin(client: AsyncClient) -> None:
    assert (await client.get("/api/privacy/subject?email=x@y.com")).status_code == 401
    assert (
        await client.post("/api/privacy/subject/erase", json={"email": "x@y.com", "mode": "delete"})
    ).status_code == 401
