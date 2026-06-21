from collections.abc import AsyncIterator

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import select

from portal.auth.passwords import hash_password
from portal.db.models import User
from portal.db.session import get_sessionmaker

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _restore_password() -> AsyncIterator[None]:
    """Always restore the admin password so other tests' logins keep working."""
    yield
    async with get_sessionmaker()() as db:
        admin = (await db.execute(select(User))).scalars().first()
        if admin is not None:
            admin.password_hash = hash_password("test-password")
            await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_email_config_roundtrip(client: AsyncClient) -> None:
    await _login(client)

    # Save SMTP config (with a password) — secret is never echoed back.
    saved = await client.post(
        "/api/settings/email",
        json={
            "smtp_host": "smtp.example.com",
            "smtp_port": 465,
            "smtp_username": "user@example.com",
            "smtp_password": "s3cret",
            "smtp_from": "portal@example.com",
            "smtp_use_tls": True,
            "smtp_starttls": False,
            "notify_email": "ops@example.com",
        },
    )
    assert saved.status_code == 200
    body = saved.json()
    assert body["configured"] is True
    assert body["smtp_host"] == "smtp.example.com"
    assert body["smtp_port"] == 465
    assert body["has_password"] is True
    assert body["notify_email"] == "ops@example.com"
    assert "smtp_password" not in body

    # GET reflects the stored config without leaking the password.
    status = (await client.get("/api/settings/email")).json()
    assert status["configured"] is True
    assert status["has_password"] is True

    # Re-saving with a blank password keeps the stored one.
    again = await client.post(
        "/api/settings/email",
        json={
            "smtp_host": "smtp.example.com",
            "smtp_port": 465,
            "smtp_from": "portal@example.com",
        },
    )
    assert again.json()["has_password"] is True

    # Clean up: app_settings is shared across the test session, so leave email unconfigured.
    cleared = await client.post("/api/settings/email", json={"clear_password": True})
    assert cleared.json()["configured"] is False


async def test_frameio_config_roundtrip(client: AsyncClient) -> None:
    await _login(client)
    saved = await client.post(
        "/api/settings/frameio-config",
        json={"client_id": "abc123", "client_secret": "shh"},
    )
    assert saved.status_code == 200
    body = saved.json()
    assert body["client_id"] == "abc123"
    assert body["has_secret"] is True
    assert body["configured"] is True
    assert "client_secret" not in body
    assert body["redirect_uri"].endswith("/api/frameio/oauth/callback")


async def test_settings_config_requires_admin(client: AsyncClient) -> None:
    assert (await client.get("/api/settings/email")).status_code == 401
    assert (await client.get("/api/settings/frameio-config")).status_code == 401


async def test_password_change(client: AsyncClient) -> None:
    await _login(client)

    # Wrong current password is rejected.
    bad = await client.post(
        "/api/settings/password",
        json={"current_password": "wrong", "new_password": "brandnewpass"},
    )
    assert bad.status_code == 403

    # Too-short new password rejected by validation.
    short = await client.post(
        "/api/settings/password",
        json={"current_password": "test-password", "new_password": "x"},
    )
    assert short.status_code == 422

    ok = await client.post(
        "/api/settings/password",
        json={"current_password": "test-password", "new_password": "brandnewpass"},
    )
    assert ok.status_code == 200

    # The new password now works for login.
    relog = await client.post(
        "/api/auth/login", json={"email": "admin@example.com", "password": "brandnewpass"}
    )
    assert relog.status_code == 200


async def test_password_requires_admin(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/settings/password",
        json={"current_password": "x", "new_password": "yyyyyyyy"},
    )
    assert resp.status_code == 401
