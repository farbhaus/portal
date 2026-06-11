from datetime import UTC, datetime, timedelta

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import FrameioConnection, User
from portal.db.session import get_sessionmaker
from portal.frameio import connection as conn_svc
from portal.frameio import oauth
from portal.frameio.oauth import IMS_AUTHORIZE_URL
from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear_connections() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(FrameioConnection))
        await db.commit()


async def _admin_id() -> object:
    async with get_sessionmaker()() as db:
        user = (await db.execute(select(User))).scalars().first()
        assert user is not None
        return user.id


def _cipher() -> TokenCipher:
    return TokenCipher(get_settings().token_encryption_key)


async def _login(client: AsyncClient) -> None:
    resp = await client.post("/api/auth/login", json=CREDS)
    assert resp.status_code == 200


# ── refresh-on-expiry core ────────────────────────────────────────────────────

def test_needs_refresh_logic() -> None:
    fresh = FrameioConnection(token_expires_at=datetime.now(UTC) + timedelta(hours=1))
    expired = FrameioConnection(token_expires_at=datetime.now(UTC) - timedelta(minutes=1))
    none = FrameioConnection(token_expires_at=None)
    assert conn_svc.needs_refresh(fresh) is False
    assert conn_svc.needs_refresh(expired) is True
    assert conn_svc.needs_refresh(none) is True


async def test_get_valid_access_token_refreshes_when_expired(monkeypatch) -> None:
    cipher = _cipher()
    user_id = await _admin_id()
    async with get_sessionmaker()() as db:
        db.add(
            FrameioConnection(
                user_id=user_id,
                access_token_encrypted=cipher.encrypt("old-access"),
                refresh_token_encrypted=cipher.encrypt("the-refresh"),
                token_expires_at=datetime.now(UTC) - timedelta(minutes=1),
            )
        )
        await db.commit()

    async def fake_refresh(refresh_token: str) -> oauth.TokenSet:
        assert refresh_token == "the-refresh"
        return oauth.TokenSet(
            access_token="new-access",
            refresh_token="new-refresh",
            expires_at=datetime.now(UTC) + timedelta(hours=24),
            scopes="openid",
        )

    monkeypatch.setattr(oauth, "refresh_tokens", fake_refresh)

    async with get_sessionmaker()() as db:
        conn = await conn_svc.get_connection(db, user_id)  # type: ignore[arg-type]
        assert conn is not None
        token = await conn_svc.get_valid_access_token(db, conn)
        assert token == "new-access"
        await db.refresh(conn)
        assert cipher.decrypt(conn.access_token_encrypted or "") == "new-access"
        assert cipher.decrypt(conn.refresh_token_encrypted or "") == "new-refresh"


async def test_get_valid_access_token_no_refresh_when_fresh(monkeypatch) -> None:
    cipher = _cipher()
    user_id = await _admin_id()
    async with get_sessionmaker()() as db:
        db.add(
            FrameioConnection(
                user_id=user_id,
                access_token_encrypted=cipher.encrypt("still-good"),
                refresh_token_encrypted=cipher.encrypt("unused"),
                token_expires_at=datetime.now(UTC) + timedelta(hours=2),
            )
        )
        await db.commit()

    async def boom(refresh_token: str) -> oauth.TokenSet:
        raise AssertionError("refresh should not be called for a fresh token")

    monkeypatch.setattr(oauth, "refresh_tokens", boom)

    async with get_sessionmaker()() as db:
        conn = await conn_svc.get_connection(db, user_id)  # type: ignore[arg-type]
        assert conn is not None
        assert await conn_svc.get_valid_access_token(db, conn) == "still-good"


# ── routes: connect / callback (state CSRF) / status / disconnect ─────────────

async def test_connect_redirects_to_ims(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.get("/api/frameio/oauth/connect")
    assert resp.status_code == 302
    assert resp.headers["location"].startswith(IMS_AUTHORIZE_URL)


async def test_callback_rejects_bad_state(client: AsyncClient) -> None:
    await _login(client)
    # No prior /connect, so session has no stored state -> mismatch.
    resp = await client.get("/api/frameio/oauth/callback", params={"code": "x", "state": "y"})
    assert resp.status_code == 302
    assert resp.headers["location"].endswith("/settings?frameio=error")


async def test_status_not_connected(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.get("/api/frameio/status")
    assert resp.status_code == 200
    assert resp.json()["connected"] is False


async def test_status_and_disconnect(client: AsyncClient) -> None:
    cipher = _cipher()
    user_id = await _admin_id()
    async with get_sessionmaker()() as db:
        db.add(
            FrameioConnection(
                user_id=user_id,
                adobe_email="colorist@example.com",
                access_token_encrypted=cipher.encrypt("a"),
                refresh_token_encrypted=cipher.encrypt("r"),
                token_expires_at=datetime.now(UTC) + timedelta(hours=2),
            )
        )
        await db.commit()

    await _login(client)
    status = await client.get("/api/frameio/status")
    assert status.json()["connected"] is True
    assert status.json()["adobe_email"] == "colorist@example.com"

    disc = await client.post("/api/frameio/disconnect")
    assert disc.status_code == 200
    assert (await client.get("/api/frameio/status")).json()["connected"] is False


async def test_status_requires_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/frameio/status")).status_code == 401
