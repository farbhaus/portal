"""Route-level email-verification flow for both upload and download links."""

from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination, DownloadLink, EmailVerification, UploadLink
from portal.db.session import get_sessionmaker
from portal.frameio.client import DownloadFile
from portal.routes import public_downloads as pubdl
from portal.uploads.links import generate_token
from portal.verify import service

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(EmailVerification))
        await db.execute(delete(UploadLink))
        await db.execute(delete(DownloadLink))
        await db.execute(delete(Destination))
        await db.commit()


def _enable_email(monkeypatch: Any) -> dict[str, str]:
    from portal.services.runtime_config import EmailConfig

    cfg = EmailConfig(
        smtp_host="smtp.test",
        smtp_port=587,
        smtp_username="",
        smtp_password="",
        smtp_from="portal@test",
        smtp_use_tls=False,
        smtp_starttls=True,
        notify_email="",
    )
    sent: dict[str, str] = {}

    async def fake_get_email_config(db: Any) -> EmailConfig:
        return cfg

    async def fake_send(config: Any, to: str, code: str, ttl: int, brand: Any = None) -> None:
        sent["to"] = to
        sent["code"] = code

    async def plausible(email: str, *, check_mx: bool) -> bool:
        return True

    monkeypatch.setattr(service, "get_email_config", fake_get_email_config)
    monkeypatch.setattr(service, "send_verification_code", fake_send)
    monkeypatch.setattr(service, "is_plausible_email", plausible)
    return sent


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def _make_upload_link() -> str:
    token = generate_token()
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="Dest", config={"type": "frameio", "account_id": "a1"})
        db.add(dest)
        await db.flush()
        db.add(
            UploadLink(
                token=token,
                destination_id=dest.id,
                uploader_fields_required={"email": True},
                verify_email=True,
            )
        )
        await db.commit()
    return token


# ── upload flow ─────────────────────────────────────────────────────────────────


async def test_upload_verify_flow(client: AsyncClient, monkeypatch: Any) -> None:
    sent = _enable_email(monkeypatch)
    token = await _make_upload_link()

    # ze@ze.ze is rejected before any code is sent (invalid email).
    async def never(email: str, *, check_mx: bool) -> bool:
        return False

    async def always(email: str, *, check_mx: bool) -> bool:
        return True

    monkeypatch.setattr(service, "is_plausible_email", never)
    bad = await client.post(f"/api/public/links/{token}/request-code", json={"email": "ze@ze.ze"})
    assert bad.status_code == 400

    # A real address gets a code.
    monkeypatch.setattr(service, "is_plausible_email", always)
    r = await client.post(f"/api/public/links/{token}/request-code", json={"email": "c@x.com"})
    assert r.status_code == 200 and r.json() == {"trusted": False, "sent": True}
    code = sent["code"]

    # No code → blocked; wrong code → 403.
    assert (
        await client.post(f"/api/public/links/{token}/sessions", json={"email": "c@x.com"})
    ).status_code == 401
    wrong = "000000" if code != "000000" else "111111"
    assert (
        await client.post(
            f"/api/public/links/{token}/sessions", json={"email": "c@x.com", "code": wrong}
        )
    ).status_code == 403

    # Correct code → session + trust cookie.
    ok = await client.post(
        f"/api/public/links/{token}/sessions", json={"email": "c@x.com", "code": code}
    )
    assert ok.status_code == 200
    assert "portal_verified" in ok.headers.get("set-cookie", "")

    # Now trusted on this client: request-code reports trusted, and a session opens with no code.
    again = await client.post(f"/api/public/links/{token}/request-code", json={"email": "c@x.com"})
    assert again.json() == {"trusted": True, "sent": False}
    assert (
        await client.post(f"/api/public/links/{token}/sessions", json={"email": "c@x.com"})
    ).status_code == 200


async def test_upload_verify_code_endpoint_unlocks(client: AsyncClient, monkeypatch: Any) -> None:
    sent = _enable_email(monkeypatch)
    token = await _make_upload_link()

    async def always(email: str, *, check_mx: bool) -> bool:
        return True

    monkeypatch.setattr(service, "is_plausible_email", always)
    await client.post(f"/api/public/links/{token}/request-code", json={"email": "c@x.com"})
    code = sent["code"]

    wrong = "000000" if code != "000000" else "111111"
    bad = await client.post(
        f"/api/public/links/{token}/verify-code", json={"email": "c@x.com", "code": wrong}
    )
    assert bad.status_code == 403

    good = await client.post(
        f"/api/public/links/{token}/verify-code", json={"email": "c@x.com", "code": code}
    )
    assert good.status_code == 200 and good.json()["ok"] is True
    assert "portal_verified" in good.headers.get("set-cookie", "")

    # Device now trusted: a session opens without re-sending a code.
    assert (
        await client.post(f"/api/public/links/{token}/sessions", json={"email": "c@x.com"})
    ).status_code == 200


# ── download flow ─────────────────────────────────────────────────────────────────


class _StubClient:
    async def get_file(self, account_id: str, file_id: str) -> DownloadFile:
        return DownloadFile(
            id=file_id, name="f.mov", file_size=1, media_type="video/quicktime",
            download_url=None, thumbnail_url=None,
        )


async def _make_download_link() -> str:
    token = generate_token()
    async with get_sessionmaker()() as db:
        db.add(
            DownloadLink(
                token=token,
                source={"type": "file", "account_id": "a1", "file_id": "f1"},
                viewer_fields_required={"email": True},
                verify_email=True,
            )
        )
        await db.commit()
    return token


async def test_download_verify_flow(client: AsyncClient, monkeypatch: Any) -> None:
    sent = _enable_email(monkeypatch)
    monkeypatch.setattr(pubdl, "get_frameio_client", lambda: _StubClient())
    token = await _make_download_link()

    r = await client.post(f"/api/public/downloads/{token}/request-code", json={"email": "c@x.com"})
    assert r.status_code == 200 and r.json()["sent"] is True
    code = sent["code"]

    # No code → blocked.
    assert (
        await client.post(f"/api/public/downloads/{token}/sessions", json={"email": "c@x.com"})
    ).status_code == 401

    # Correct code → session (file listing) + trust cookie.
    ok = await client.post(
        f"/api/public/downloads/{token}/sessions", json={"email": "c@x.com", "code": code}
    )
    assert ok.status_code == 200
    assert "portal_verified" in ok.headers.get("set-cookie", "")
    assert len(ok.json()["files"]) == 1


# ── admin: verify_email persists and forces email-required ──────────────────────


async def test_admin_download_link_verify_forces_email(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/download-links",
        json={
            "source": {"type": "file", "account_id": "a1", "file_id": "f1"},
            "verify_email": True,
            "viewer_fields_required": {"name": False, "email": False},
        },
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["verify_email"] is True
    assert body["viewer_fields_required"]["email"] is True  # forced on


async def test_admin_upload_link_verify_forces_email(client: AsyncClient) -> None:
    await _login(client)
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio", "account_id": "a1"})
        db.add(dest)
        await db.commit()
        dest_id = str(dest.id)
    resp = await client.post(
        "/api/upload-links",
        json={
            "destination_id": dest_id,
            "verify_email": True,
            "uploader_fields_required": {"name": False, "email": False, "message": False},
        },
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["verify_email"] is True
    assert body["uploader_fields_required"]["email"] is True
