from collections.abc import AsyncIterator

import pyotp
import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, update

from portal.db.models import AppSettings, User, WebauthnCredential
from portal.db.session import get_sessionmaker

CREDS = {"email": "admin@example.com", "password": "test-password"}
PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64


async def _login(client: AsyncClient) -> None:
    resp = await client.post("/api/auth/login", json=CREDS)
    assert resp.status_code == 200


@pytest_asyncio.fixture(autouse=True)
async def _reset_security() -> AsyncIterator[None]:
    """The admin row is shared across the session — reset 2FA/passkeys/branding after each test
    so an enabled TOTP doesn't leak into other tests' logins."""
    yield
    async with get_sessionmaker()() as db:
        await db.execute(
            update(User).values(
                totp_enabled=False, totp_secret_encrypted=None, totp_recovery_codes=None
            )
        )
        await db.execute(delete(WebauthnCredential))
        await db.execute(delete(AppSettings))
        await db.commit()


# ── branding ────────────────────────────────────────────────────────────────


async def test_branding_logo_upload_serve_remove(client: AsyncClient) -> None:
    await _login(client)
    # non-PNG rejected (magic bytes)
    bad = await client.post(
        "/api/settings/branding/logo", files={"file": ("x.png", b"not a png", "image/png")}
    )
    assert bad.status_code == 400

    ok = await client.post(
        "/api/settings/branding/logo", files={"file": ("logo.png", PNG, "image/png")}
    )
    assert ok.status_code == 200
    assert ok.json()["has_logo"] is True

    # served (the endpoint requires no auth — see test_branding_admin_requires_auth)
    served = await client.get("/api/branding/logo")
    assert served.status_code == 200
    assert served.headers["content-type"].startswith("image/png")
    assert served.content == PNG

    rm = await client.delete("/api/settings/branding/logo")
    assert rm.status_code == 200
    assert rm.json()["has_logo"] is False
    assert (await client.get("/api/branding/logo")).status_code == 404


async def test_branding_admin_requires_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/settings/branding")).status_code == 401
    assert (await client.post("/api/settings/branding/logo")).status_code == 401


# ── TOTP ──────────────────────────────────────────────────────────────────────


async def _enable_totp(client: AsyncClient) -> list[str]:
    begin = await client.post("/api/settings/totp/begin")
    assert begin.status_code == 200
    secret = begin.json()["secret"]
    assert "<svg" in begin.json()["qr_svg"]
    en = await client.post("/api/settings/totp/enable", json={"code": pyotp.TOTP(secret).now()})
    assert en.status_code == 200
    return en.json()["recovery_codes"]


async def test_totp_enable_rejects_bad_code(client: AsyncClient) -> None:
    await _login(client)
    assert (await client.post("/api/settings/totp/begin")).status_code == 200
    bad = await client.post("/api/settings/totp/enable", json={"code": "000000"})
    assert bad.status_code == 400


async def test_totp_login_two_step(client: AsyncClient) -> None:
    await _login(client)
    recovery = await _enable_totp(client)
    assert len(recovery) == 10
    assert (await client.get("/api/settings/security")).json()["totp_enabled"] is True

    # New password login now demands a second factor.
    await client.post("/api/auth/logout")
    login = await client.post("/api/auth/login", json=CREDS)
    assert login.status_code == 200
    assert login.json()["totp_required"] is True
    # No session yet.
    assert (await client.get("/api/auth/me")).status_code == 401

    # A recovery code completes the login.
    done = await client.post("/api/auth/login/totp", json={"code": recovery[0]})
    assert done.status_code == 200
    assert done.json()["email"] == CREDS["email"]
    assert (await client.get("/api/auth/me")).status_code == 200


async def test_totp_login_wrong_code_rejected(client: AsyncClient) -> None:
    await _login(client)
    await _enable_totp(client)
    await client.post("/api/auth/logout")
    assert (await client.post("/api/auth/login", json=CREDS)).json()["totp_required"] is True
    assert (await client.post("/api/auth/login/totp", json={"code": "000000"})).status_code == 401


# ── passkeys ────────────────────────────────────────────────────────────────


async def test_passkey_register_begin_requires_admin(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/passkeys/register/begin")).status_code == 401


async def test_passkey_register_begin_returns_options(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post("/api/auth/passkeys/register/begin")
    assert resp.status_code == 200
    body = resp.json()
    assert "challenge" in body
    assert body["rp"]["id"]


async def test_passkey_login_begin_is_public(client: AsyncClient) -> None:
    resp = await client.post("/api/auth/passkeys/login/begin")
    assert resp.status_code == 200
    assert "challenge" in resp.json()


async def test_passkey_login_complete_without_challenge_rejected(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/auth/passkeys/login/complete", json={"credential": {"id": "nope"}}
    )
    assert resp.status_code == 401
