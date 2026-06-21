from typing import Any

from httpx import AsyncClient

from portal.lib.config import get_settings


async def test_login_success_and_me(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "test-password"},
    )
    assert resp.status_code == 200
    assert resp.json()["email"] == "admin@example.com"

    me = await client.get("/api/auth/me")
    assert me.status_code == 200
    assert me.json()["email"] == "admin@example.com"


async def test_login_wrong_password_rejected(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "nope"},
    )
    assert resp.status_code == 401


async def test_login_unknown_email_rejected(client: AsyncClient) -> None:
    # Unknown account still returns the same 401 (and pays a dummy bcrypt verify, so timing
    # doesn't reveal which emails exist).
    resp = await client.post(
        "/api/auth/login",
        json={"email": "nobody@example.com", "password": "whatever"},
    )
    assert resp.status_code == 401


async def test_me_requires_auth(client: AsyncClient) -> None:
    resp = await client.get("/api/auth/me")
    assert resp.status_code == 401


async def test_logout_clears_session(client: AsyncClient) -> None:
    await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "test-password"},
    )
    assert (await client.get("/api/auth/me")).status_code == 200
    await client.post("/api/auth/logout")
    assert (await client.get("/api/auth/me")).status_code == 401


async def test_login_is_rate_limited(client: AsyncClient, monkeypatch: Any) -> None:
    s = get_settings()
    monkeypatch.setattr(s, "rate_limit_enabled", True)
    monkeypatch.setattr(s, "rate_limit_strict_per_minute", 3)
    monkeypatch.setattr(s, "trusted_proxy_hops", 1)
    hdr = {"X-Forwarded-For": "198.51.100.50"}  # unique bucket so the test is isolated

    # The brute-force throttle applies regardless of whether the credentials are valid.
    for _ in range(3):
        r = await client.post(
            "/api/auth/login",
            json={"email": "admin@example.com", "password": "nope"},
            headers=hdr,
        )
        assert r.status_code == 401
    r = await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "nope"},
        headers=hdr,
    )
    assert r.status_code == 429
