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


async def test_general(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.get("/api/settings/general")
    assert resp.status_code == 200
    assert "base_url" in resp.json()


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
