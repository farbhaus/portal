from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination, UploadLink
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.uploads.links import generate_token

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(UploadLink))
        await db.execute(delete(Destination))
        await db.commit()


async def _make_link() -> str:
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        db.add(dest)
        await db.flush()
        token = generate_token()
        db.add(UploadLink(token=token, destination_id=dest.id))
        await db.commit()
        return token


async def test_strict_endpoint_rate_limited(client: AsyncClient, monkeypatch: Any) -> None:
    token = await _make_link()
    s = get_settings()
    monkeypatch.setattr(s, "rate_limit_enabled", True)
    monkeypatch.setattr(s, "rate_limit_strict_per_minute", 2)

    # verify-password is a "strict" endpoint; the 3rd call within the window is throttled.
    url = f"/api/public/links/{token}/verify-password"
    assert (await client.post(url, json={"password": "x"})).status_code == 200
    assert (await client.post(url, json={"password": "x"})).status_code == 200
    assert (await client.post(url, json={"password": "x"})).status_code == 429


async def test_disabled_by_default(client: AsyncClient) -> None:
    token = await _make_link()
    url = f"/api/public/links/{token}/verify-password"
    for _ in range(5):
        assert (await client.post(url, json={"password": "x"})).status_code == 200


async def test_xff_spoof_does_not_multiply_budget(client: AsyncClient, monkeypatch: Any) -> None:
    token = await _make_link()
    s = get_settings()
    monkeypatch.setattr(s, "rate_limit_enabled", True)
    monkeypatch.setattr(s, "rate_limit_strict_per_minute", 2)
    monkeypatch.setattr(s, "trusted_proxy_hops", 2)
    url = f"/api/public/links/{token}/verify-password"

    # Two trusted hops → the real client is the 2nd entry from the right (198.51.100.9). Rotating
    # the spoofable leftmost entry must not hand out fresh buckets: the 3rd call still throttles.
    async def post(spoof: str) -> int:
        headers = {"X-Forwarded-For": f"{spoof}, 198.51.100.9, 10.0.0.1"}
        return (await client.post(url, json={"password": "x"}, headers=headers)).status_code

    assert await post("1.1.1.1") == 200
    assert await post("2.2.2.2") == 200
    assert await post("3.3.3.3") == 429


async def test_distinct_real_clients_get_separate_budgets(
    client: AsyncClient, monkeypatch: Any
) -> None:
    token = await _make_link()
    s = get_settings()
    monkeypatch.setattr(s, "rate_limit_enabled", True)
    monkeypatch.setattr(s, "rate_limit_strict_per_minute", 1)
    monkeypatch.setattr(s, "trusted_proxy_hops", 2)
    url = f"/api/public/links/{token}/verify-password"

    a = {"X-Forwarded-For": "spoof, 203.0.113.1, 10.0.0.1"}
    b = {"X-Forwarded-For": "spoof, 203.0.113.2, 10.0.0.1"}
    assert (await client.post(url, json={"password": "x"}, headers=a)).status_code == 200
    assert (await client.post(url, json={"password": "x"}, headers=a)).status_code == 429
    # A genuinely different client (different trusted-hop IP) gets its own fresh budget.
    assert (await client.post(url, json={"password": "x"}, headers=b)).status_code == 200
