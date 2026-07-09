"""Global per-token lockout for link password grinding (portal.lib.lockout).

The per-IP strict rate limit is covered in test_ratelimit.py; these tests cover the global
counter that a distributed attacker can't sidestep by rotating IPs. Each test mints a fresh
token, so lockout counters never leak between tests.
"""

from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.auth.passwords import hash_password
from portal.db.models import Destination, DownloadLink, UploadLink
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.uploads.links import generate_token

PASSWORD = "correct-horse"


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(UploadLink))
        await db.execute(delete(DownloadLink))
        await db.execute(delete(Destination))
        await db.commit()


def _enable(monkeypatch: Any, max_failures: int) -> None:
    s = get_settings()
    monkeypatch.setattr(s, "rate_limit_enabled", True)
    # Keep the per-IP limiter out of the way so only the global counter is under test.
    monkeypatch.setattr(s, "rate_limit_strict_per_minute", 1000)
    monkeypatch.setattr(s, "rate_limit_default_per_minute", 1000)
    monkeypatch.setattr(s, "password_lockout_max_failures", max_failures)


async def _make_upload_link() -> str:
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        token = generate_token()
        db.add(
            UploadLink(
                token=token, destination_id=dest.id, password_hash=hash_password(PASSWORD)
            )
        )
        await db.commit()
        return token


async def _make_download_link() -> str:
    async with get_sessionmaker()() as db:
        token = generate_token()
        db.add(
            DownloadLink(
                token=token,
                source={"type": "file", "account_id": "a1", "file_id": "f1"},
                password_hash=hash_password(PASSWORD),
            )
        )
        await db.commit()
        return token


async def test_verify_password_locks_out_globally(client: AsyncClient, monkeypatch: Any) -> None:
    _enable(monkeypatch, max_failures=3)
    token = await _make_upload_link()
    url = f"/api/public/links/{token}/verify-password"

    for _ in range(3):
        resp = await client.post(url, json={"password": "wrong"})
        assert resp.status_code == 200
        assert resp.json()["ok"] is False

    # Budget exhausted: even the correct password answers 429 until the window expires.
    assert (await client.post(url, json={"password": PASSWORD})).status_code == 429


async def test_correct_password_does_not_consume_budget(
    client: AsyncClient, monkeypatch: Any
) -> None:
    _enable(monkeypatch, max_failures=2)
    token = await _make_upload_link()
    url = f"/api/public/links/{token}/verify-password"

    for _ in range(5):
        resp = await client.post(url, json={"password": PASSWORD})
        assert resp.status_code == 200
        assert resp.json()["ok"] is True


async def test_upload_session_start_locks_out(client: AsyncClient, monkeypatch: Any) -> None:
    _enable(monkeypatch, max_failures=2)
    token = await _make_upload_link()
    url = f"/api/public/links/{token}/sessions"

    assert (await client.post(url, json={"password": "wrong"})).status_code == 403
    assert (await client.post(url, json={"password": "wrong"})).status_code == 403
    assert (await client.post(url, json={"password": "wrong"})).status_code == 429
    assert (await client.post(url, json={"password": PASSWORD})).status_code == 429


async def test_download_session_start_locks_out(client: AsyncClient, monkeypatch: Any) -> None:
    _enable(monkeypatch, max_failures=2)
    token = await _make_download_link()
    url = f"/api/public/downloads/{token}/sessions"

    assert (await client.post(url, json={"password": "wrong"})).status_code == 403
    assert (await client.post(url, json={"password": "wrong"})).status_code == 403
    assert (await client.post(url, json={"password": "wrong"})).status_code == 429


async def test_lockout_inert_when_rate_limiting_disabled(client: AsyncClient) -> None:
    # RATE_LIMIT_ENABLED=false (the test default) also disables the global counter.
    token = await _make_upload_link()
    url = f"/api/public/links/{token}/verify-password"
    for _ in range(40):
        assert (await client.post(url, json={"password": "wrong"})).status_code == 200