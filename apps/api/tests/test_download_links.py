from datetime import UTC, datetime, timedelta

import pytest
import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import DownloadEvent, DownloadLink
from portal.db.session import get_sessionmaker
from portal.downloads.links import branding, resolve_source, validate_source
from portal.frameio.client import DownloadFile, FileItem
from portal.routes import public_downloads as pubdl

CREDS = {"email": "admin@example.com", "password": "test-password"}
FILE_SOURCE = {"type": "file", "account_id": "a1", "file_id": "file-1"}


# ── stub client ───────────────────────────────────────────────────────────────


class _StubDownloadClient:
    async def get_file(self, account_id: str, file_id: str) -> DownloadFile:
        return DownloadFile(
            id=file_id,
            name=f"{file_id}.mov",
            file_size=10,
            media_type="video/quicktime",
            download_url=None,
            thumbnail_url=f"https://thumb/{file_id}",
        )

    async def list_files(self, account_id: str, folder_id: str) -> list[FileItem]:
        return [
            FileItem(id="f1", name="f1.mov", file_size=10, media_type="video/quicktime"),
            FileItem(id="f2", name="f2.mov", file_size=20, media_type="video/quicktime"),
        ]

    async def list_files_recursive(self, account_id: str, folder_id: str) -> list[FileItem]:
        return await self.list_files(account_id, folder_id)

    async def get_download_url(self, account_id: str, file_id: str) -> str:
        return f"https://s3/signed/{file_id}"


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(DownloadLink))
        await db.commit()


def _use_stub(monkeypatch) -> None:
    monkeypatch.setattr(pubdl, "get_frameio_client", lambda: _StubDownloadClient())


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def _make_link(client: AsyncClient, **kw: object) -> dict:
    body = {"source": FILE_SOURCE, **kw}
    resp = await client.post("/api/download-links", json=body)
    assert resp.status_code == 201, resp.text
    return resp.json()


async def _start(client: AsyncClient, token: str, **body: object) -> str:
    resp = await client.post(f"/api/public/downloads/{token}/sessions", json=body)
    assert resp.status_code == 200, resp.text
    return resp.json()["session_id"]


# ── unit: source validation / resolution / branding ───────────────────────────


def test_validate_source() -> None:
    validate_source(FILE_SOURCE)
    validate_source({"type": "folder", "account_id": "a1", "folder_id": "fl1"})
    validate_source({"type": "selection", "account_id": "a1", "file_ids": ["x"]})
    for bad in (
        {"type": "file", "file_id": "x"},  # no account_id
        {"type": "file", "account_id": "a1"},  # no file_id
        {"type": "selection", "account_id": "a1", "file_ids": []},  # empty
        {"type": "nope", "account_id": "a1"},  # bad type
    ):
        with pytest.raises(ValueError):
            validate_source(bad)


async def test_resolve_source_modes() -> None:
    c = _StubDownloadClient()
    file_res = await resolve_source(c, FILE_SOURCE)  # type: ignore[arg-type]
    assert [f.id for f in file_res.files] == ["file-1"]

    sel = await resolve_source(c, {"type": "selection", "account_id": "a1", "file_ids": ["a", "b"]})  # type: ignore[arg-type]
    assert [f.id for f in sel.files] == ["a", "b"]

    folder = await resolve_source(c, {"type": "folder", "account_id": "a1", "folder_id": "fl"})  # type: ignore[arg-type]
    assert [f.id for f in folder.files] == ["f1", "f2"]
    assert folder.files[0].thumbnail_url is None  # folder sources skip thumbnails


def test_branding() -> None:
    link = DownloadLink(brand_display_name="Deliverables", brand_subtitle=None)
    b = branding(link)
    assert b["display_name"] == "Deliverables"
    assert branding(DownloadLink())["display_name"] == "Download"


# ── admin CRUD ────────────────────────────────────────────────────────────────


async def test_create_and_list(client: AsyncClient) -> None:
    await _login(client)
    link = await _make_link(client, brand_display_name="Client deliverables", max_downloads=5)
    assert link["public_url"].endswith(f"/d/{link['token']}")
    assert link["state"] == "ok"
    assert link["max_downloads"] == 5
    listed = await client.get("/api/download-links")
    assert [link_["id"] for link_ in listed.json()] == [link["id"]]


async def test_create_invalid_source(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post("/api/download-links", json={"source": {"type": "file"}})
    assert resp.status_code == 400


async def test_update_and_revoke_and_delete(client: AsyncClient) -> None:
    await _login(client)
    link = await _make_link(client)
    upd = await client.patch(
        f"/api/download-links/{link['id']}", json={"password": "pw", "allow_preview": False}
    )
    assert upd.json()["password_protected"] is True
    assert upd.json()["allow_preview"] is False

    rev = await client.post(f"/api/download-links/{link['id']}/revoke")
    assert rev.json()["state"] == "revoked"

    assert (await client.delete(f"/api/download-links/{link['id']}")).status_code == 204
    assert (await client.get(f"/api/download-links/{link['id']}")).status_code == 404


async def test_admin_requires_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/download-links")).status_code == 401
    assert (await client.post("/api/download-links", json={})).status_code == 401


# ── public recipient flow ─────────────────────────────────────────────────────


async def test_public_resolve_and_session_and_download(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    link = await _make_link(client, brand_display_name="Deliverables")
    token = link["token"]

    page = await client.get(f"/api/public/downloads/{token}")
    assert page.status_code == 200
    assert page.json()["state"] == "ok"
    assert page.json()["display_name"] == "Deliverables"
    assert page.json()["password_required"] is False

    started = await client.post(f"/api/public/downloads/{token}/sessions", json={})
    assert started.status_code == 200
    sid = started.json()["session_id"]
    files = started.json()["files"]
    assert files[0]["id"] == "file-1"
    assert files[0]["thumbnail_url"] == "https://thumb/file-1"  # allow_preview default true

    url = await client.post(
        f"/api/public/downloads/{token}/sessions/{sid}/files/file-1/url"
    )
    assert url.status_code == 200
    assert url.json()["url"] == "https://s3/signed/file-1"

    # download_event recorded + counter incremented
    async with get_sessionmaker()() as db:
        events = (await db.execute(select(DownloadEvent))).scalars().all()
        assert len(events) == 1 and events[0].frameio_file_id == "file-1"
        row = await db.get(DownloadLink, __import__("uuid").UUID(link["id"]))
        assert row is not None and row.downloads_count == 1


async def test_download_cap_enforced(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    link = await _make_link(client, max_downloads=1)
    token = link["token"]
    sid = await _start(client, token)
    first = await client.post(f"/api/public/downloads/{token}/sessions/{sid}/files/file-1/url")
    assert first.status_code == 200
    second = await client.post(f"/api/public/downloads/{token}/sessions/{sid}/files/file-1/url")
    assert second.status_code == 429


async def test_file_not_in_source(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    link = await _make_link(client)
    token = link["token"]
    sid = await _start(client, token)
    resp = await client.post(f"/api/public/downloads/{token}/sessions/{sid}/files/other-file/url")
    assert resp.status_code == 404


async def test_password_and_required_fields(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    link = await _make_link(
        client, password="secret", viewer_fields_required={"email": True}
    )
    token = link["token"]
    assert (
        await client.post(f"/api/public/downloads/{token}/sessions", json={"password": "wrong"})
    ).status_code == 403
    # right password but missing required email
    assert (
        await client.post(f"/api/public/downloads/{token}/sessions", json={"password": "secret"})
    ).status_code == 422
    ok = await client.post(
        f"/api/public/downloads/{token}/sessions",
        json={"password": "secret", "email": "client@example.com"},
    )
    assert ok.status_code == 200


async def test_expired_and_unknown(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    link = await _make_link(client)
    async with get_sessionmaker()() as db:
        row = await db.get(DownloadLink, __import__("uuid").UUID(link["id"]))
        assert row is not None
        row.expires_at = datetime.now(UTC) - timedelta(hours=1)
        await db.commit()
    assert (
        await client.post(f"/api/public/downloads/{link['token']}/sessions", json={})
    ).status_code == 410
    assert (await client.get("/api/public/downloads/nope")).status_code == 404
