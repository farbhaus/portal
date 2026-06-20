from datetime import UTC, datetime, timedelta

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination, UploadLink
from portal.db.session import get_sessionmaker
from portal.frameio.client import ChunkUrl, UploadStatus, UploadTarget
from portal.routes import public as public_routes
from portal.uploads.links import (
    generate_token,
    link_state,
    merged_branding,
    normalize_extensions,
)

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(UploadLink))
        await db.execute(delete(Destination))
        await db.commit()


async def _make_destination(**kw: object) -> str:
    async with get_sessionmaker()() as db:
        dest = Destination(
            display_name=kw.get("display_name", "Camera Dest"),
            config={
                "type": "frameio",
                "account_id": "a1",
                "workspace_id": "w1",
                "project_id": "p1",
                "folder_id": "f1",
                "folder_name": "Shorts",
            },
            logo_url=kw.get("logo_url"),
            accent_color=kw.get("accent_color"),
            subtitle=kw.get("subtitle"),
        )
        db.add(dest)
        await db.commit()
        await db.refresh(dest)
        return str(dest.id)


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


# ── unit helpers ──────────────────────────────────────────────────────────────


def test_normalize_extensions() -> None:
    assert normalize_extensions([".MOV", "r3d", "mov", " .WAV "]) == ["mov", "r3d", "wav"]
    assert normalize_extensions([]) is None
    assert normalize_extensions(None) is None


def test_generate_token_unique() -> None:
    assert generate_token() != generate_token()
    assert len(generate_token()) >= 20


def test_link_state() -> None:
    ok = UploadLink(expires_at=None, revoked_at=None)
    expired = UploadLink(expires_at=datetime.now(UTC) - timedelta(minutes=1))
    revoked = UploadLink(revoked_at=datetime.now(UTC))
    assert link_state(ok) == "ok"
    assert link_state(expired) == "expired"
    assert link_state(revoked) == "revoked"


def test_merged_branding_prefers_link_override() -> None:
    dest = Destination(
        display_name="Dest Name", subtitle="dest sub", logo_url="d.png", accent_color="#000"
    )
    link = UploadLink(brand_display_name="Link Name", brand_logo_url=None)
    merged = merged_branding(link, dest)
    assert merged["display_name"] == "Link Name"  # override wins
    assert merged["subtitle"] == "dest sub"  # falls back to destination
    assert merged["logo_url"] == "d.png"


# ── admin CRUD ────────────────────────────────────────────────────────────────


async def test_create_link(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    resp = await client.post(
        "/api/upload-links",
        json={
            "destination_id": dest_id,
            "password": "secret",
            "max_file_size": 1000,
            "allowed_extensions": [".MOV", "r3d"],
            "uploader_fields_required": {"name": True, "email": True},
        },
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["token"]
    assert body["public_url"].endswith(f"/u/{body['token']}")
    assert body["state"] == "ok"
    assert body["password_protected"] is True
    assert body["allowed_extensions"] == ["mov", "r3d"]
    assert body["uploader_fields_required"] == {"name": True, "email": True, "message": False}
    assert body["destination_name"] == "Camera Dest"


async def test_create_link_bad_destination(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/upload-links",
        json={"destination_id": "00000000-0000-0000-0000-000000000000"},
    )
    assert resp.status_code == 400


async def test_update_link_password_and_revoke(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    link = (await client.post("/api/upload-links", json={"destination_id": dest_id})).json()
    assert link["password_protected"] is False

    # set a password
    upd = await client.patch(f"/api/upload-links/{link['id']}", json={"password": "pw"})
    assert upd.json()["password_protected"] is True
    # clear it with empty string
    upd = await client.patch(f"/api/upload-links/{link['id']}", json={"password": ""})
    assert upd.json()["password_protected"] is False

    # revoke
    rev = await client.post(f"/api/upload-links/{link['id']}/revoke")
    assert rev.json()["state"] == "revoked"
    assert rev.json()["revoked_at"] is not None


async def test_update_link_change_destination(client: AsyncClient) -> None:
    await _login(client)
    dest_a = await _make_destination(display_name="A")
    dest_b = await _make_destination(display_name="B")
    link = (await client.post("/api/upload-links", json={"destination_id": dest_a})).json()
    assert link["destination_id"] == dest_a

    upd = await client.patch(f"/api/upload-links/{link['id']}", json={"destination_id": dest_b})
    assert upd.status_code == 200
    assert upd.json()["destination_id"] == dest_b

    bad = await client.patch(
        f"/api/upload-links/{link['id']}",
        json={"destination_id": "00000000-0000-0000-0000-000000000000"},
    )
    assert bad.status_code == 400


async def test_create_link_with_target_subfolder(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    resp = await client.post(
        "/api/upload-links",
        json={
            "destination_id": dest_id,
            "target_folder_id": "sub123",
            "target_folder_name": "Dailies",
            "subfolder_template": "  {date}/{uploader_name}  ",
        },
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["target_folder_id"] == "sub123"
    assert body["target_folder_name"] == "Dailies"
    assert body["subfolder_template"] == "{date}/{uploader_name}"  # trimmed

    # An empty template string clears it back to null.
    upd = await client.patch(
        f"/api/upload-links/{body['id']}", json={"subfolder_template": "  "}
    )
    assert upd.json()["subfolder_template"] is None


async def test_list_and_delete(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    link = (await client.post("/api/upload-links", json={"destination_id": dest_id})).json()

    listed = await client.get("/api/upload-links")
    assert [link_["id"] for link_ in listed.json()] == [link["id"]]

    assert (await client.delete(f"/api/upload-links/{link['id']}")).status_code == 204
    assert (await client.get(f"/api/upload-links/{link['id']}")).status_code == 404


async def test_extension_preset(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.get("/api/upload-links/extension-preset")
    assert resp.status_code == 200
    assert "r3d" in resp.json() and "mov" in resp.json()


async def test_admin_routes_require_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/upload-links")).status_code == 401
    assert (await client.post("/api/upload-links", json={})).status_code == 401


# ── public resolve / password ─────────────────────────────────────────────────


async def test_public_resolve_merges_branding_and_hides_internals(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination(subtitle="dest subtitle", accent_color="#222222")
    link = (
        await client.post(
            "/api/upload-links",
            json={"destination_id": dest_id, "brand_display_name": "Send me footage"},
        )
    ).json()

    # public, no auth needed — use a cookieless client call
    resp = await client.get(f"/api/public/links/{link['token']}")
    assert resp.status_code == 200
    body = resp.json()
    assert body["state"] == "ok"
    assert body["display_name"] == "Send me footage"  # link override
    assert body["subtitle"] == "dest subtitle"  # destination fallback
    assert body["password_required"] is False
    # Frame.io internals must never be exposed publicly.
    assert "account_id" not in resp.text and "folder_id" not in resp.text


async def test_public_resolve_unknown_token_404(client: AsyncClient) -> None:
    assert (await client.get("/api/public/links/nope")).status_code == 404


async def test_public_resolve_expired_and_revoked(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    link = (await client.post("/api/upload-links", json={"destination_id": dest_id})).json()
    # expire it directly
    async with get_sessionmaker()() as db:
        row = await db.get(UploadLink, __import__("uuid").UUID(link["id"]))
        assert row is not None
        row.expires_at = datetime.now(UTC) - timedelta(hours=1)
        await db.commit()
    assert (await client.get(f"/api/public/links/{link['token']}")).json()["state"] == "expired"

    await client.post(f"/api/upload-links/{link['id']}/revoke")
    assert (await client.get(f"/api/public/links/{link['token']}")).json()["state"] == "revoked"


async def test_verify_password(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    link = (
        await client.post(
            "/api/upload-links", json={"destination_id": dest_id, "password": "letmein"}
        )
    ).json()
    token = link["token"]
    vp = f"/api/public/links/{token}/verify-password"

    assert (await client.post(vp, json={"password": "letmein"})).json()["ok"] is True
    assert (await client.post(vp, json={"password": "wrong"})).json()["ok"] is False


async def test_verify_password_open_link_ok(client: AsyncClient) -> None:
    await _login(client)
    dest_id = await _make_destination()
    link = (await client.post("/api/upload-links", json={"destination_id": dest_id})).json()
    resp = await client.post(
        f"/api/public/links/{link['token']}/verify-password", json={"password": ""}
    )
    assert resp.json()["ok"] is True


# ── public uploader session flow ──────────────────────────────────────────────


class _StubUploadClient:
    def __init__(self, status: UploadStatus | None = None) -> None:
        self._status = status or UploadStatus(complete=True, failed=False)

    async def ensure_folder_path(self, account_id: str, root: str, parts: list[str]) -> str:
        return "leaf"

    async def create_local_upload(
        self, account_id: str, folder_id: str, *, name: str, file_size: int
    ) -> UploadTarget:
        return UploadTarget(
            file_id="file-1",
            media_type="video/quicktime",
            chunks=[ChunkUrl(size=file_size, url="https://s3/part1")],
        )

    async def get_upload_status(self, account_id: str, file_id: str) -> UploadStatus:
        return self._status


def _use_upload_stub(monkeypatch, status: UploadStatus | None = None) -> None:
    monkeypatch.setattr(public_routes, "get_frameio_client", lambda: _StubUploadClient(status))


async def _make_link(client: AsyncClient, **kw: object) -> dict:
    dest_id = await _make_destination()
    return (await client.post("/api/upload-links", json={"destination_id": dest_id, **kw})).json()


async def test_full_upload_session_flow(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    link = await _make_link(client, allowed_extensions=["mov"])
    token = link["token"]

    # start a session
    started = await client.post(
        f"/api/public/links/{token}/sessions", json={"name": "DIT Berlin"}
    )
    assert started.status_code == 200
    sid = started.json()["session_id"]

    # request a file upload (with a subfolder path)
    fr = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files",
        json={"path": "A-CAM/clip.mov", "size": 1000},
    )
    assert fr.status_code == 200
    body = fr.json()
    assert body["file_id"] == "file-1"
    assert body["chunks"] == [{"part": 1, "size": 1000, "url": "https://s3/part1"}]
    assert body["headers"]["x-amz-acl"] == "private"
    assert body["headers"]["Content-Type"] == "video/quicktime"

    # mark file complete
    cf = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files/{body['file_id']}/complete"
    )
    assert cf.json() == {"complete": True, "failed": False}

    # complete session
    cs = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/complete", json={"total_bytes": 1000}
    )
    assert cs.json()["ok"] is True


class _CapturingUploadClient(_StubUploadClient):
    """Records the root folder + path parts the backend resolves an upload into."""

    def __init__(self) -> None:
        super().__init__()
        self.calls: dict[str, object] = {}

    async def ensure_folder_path(self, account_id: str, root: str, parts: list[str]) -> str:
        self.calls["root"] = root
        self.calls["parts"] = parts
        return "leaf"

    async def create_local_upload(
        self, account_id: str, folder_id: str, *, name: str, file_size: int
    ) -> UploadTarget:
        self.calls["folder_id"] = folder_id
        self.calls["name"] = name
        return await super().create_local_upload(
            account_id, folder_id, name=name, file_size=file_size
        )


async def _open_session_and_request(
    client: AsyncClient, token: str, *, name: str, path: str
) -> None:
    started = await client.post(
        f"/api/public/links/{token}/sessions", json={"name": name}
    )
    sid = started.json()["session_id"]
    fr = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files",
        json={"path": path, "size": 1000},
    )
    assert fr.status_code == 200


async def test_upload_applies_target_folder_and_template(
    client: AsyncClient, monkeypatch
) -> None:
    await _login(client)
    cap = _CapturingUploadClient()
    monkeypatch.setattr(public_routes, "get_frameio_client", lambda: cap)
    link = await _make_link(
        client, target_folder_id="tf99", target_folder_name="ProjectX",
        subfolder_template="{uploader_name}",
    )
    await _open_session_and_request(
        client, link["token"], name="DIT Berlin", path="A-CAM/clip.mov"
    )
    # Folder override → uploads root at the picked base subfolder, not the destination root.
    assert cap.calls["root"] == "tf99"
    # Template prefix + browser-supplied subdir, then the bare filename.
    assert cap.calls["parts"] == ["DIT Berlin", "A-CAM"]
    assert cap.calls["name"] == "clip.mov"


async def test_upload_without_target_uses_destination_root(
    client: AsyncClient, monkeypatch
) -> None:
    await _login(client)
    cap = _CapturingUploadClient()
    monkeypatch.setattr(public_routes, "get_frameio_client", lambda: cap)
    link = await _make_link(client)
    await _open_session_and_request(
        client, link["token"], name="DIT", path="A-CAM/clip.mov"
    )
    assert cap.calls["root"] == "f1"  # destination's root folder
    assert cap.calls["parts"] == ["A-CAM"]


async def test_start_session_password_required(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    link = await _make_link(client, password="sekret")
    token = link["token"]
    assert (
        await client.post(f"/api/public/links/{token}/sessions", json={"password": "wrong"})
    ).status_code == 403
    assert (
        await client.post(f"/api/public/links/{token}/sessions", json={"password": "sekret"})
    ).status_code == 200


async def test_start_session_missing_required_field(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    link = await _make_link(client, uploader_fields_required={"email": True})
    resp = await client.post(f"/api/public/links/{link['token']}/sessions", json={"name": "x"})
    assert resp.status_code == 422


async def test_start_session_expired_link_410(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    link = await _make_link(client)
    async with get_sessionmaker()() as db:
        row = await db.get(UploadLink, __import__("uuid").UUID(link["id"]))
        assert row is not None
        row.expires_at = datetime.now(UTC) - timedelta(hours=1)
        await db.commit()
    assert (
        await client.post(f"/api/public/links/{link['token']}/sessions", json={})
    ).status_code == 410


async def test_request_file_rejects_bad_ext_and_size(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    link = await _make_link(client, allowed_extensions=["mov"], max_file_size=500)
    token = link["token"]
    sid = (await client.post(f"/api/public/links/{token}/sessions", json={})).json()["session_id"]

    bad_ext = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files", json={"path": "notes.txt", "size": 10}
    )
    assert bad_ext.status_code == 400
    too_big = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files", json={"path": "clip.mov", "size": 999}
    )
    assert too_big.status_code == 400


async def test_completion_schedules_email(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch)
    calls: list[dict] = []

    async def capture(**kwargs: object) -> None:
        calls.append(kwargs)

    monkeypatch.setattr(public_routes, "send_upload_completion", capture)
    link = await _make_link(client, brand_display_name="DIT Drop")
    token = link["token"]
    sid = (
        await client.post(f"/api/public/links/{token}/sessions", json={"name": "DIT"})
    ).json()["session_id"]
    await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files",
        json={"path": "A-CAM/clip.mov", "size": 1000},
    )
    await client.post(
        f"/api/public/links/{token}/sessions/{sid}/complete", json={"total_bytes": 1000}
    )

    assert len(calls) == 1
    assert calls[0]["link_name"] == "DIT Drop"
    assert calls[0]["total_bytes"] == 1000
    assert calls[0]["files"][0].name == "A-CAM/clip.mov"
    assert calls[0]["files"][0].size == 1000


async def test_complete_file_pending(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_upload_stub(monkeypatch, UploadStatus(complete=False, failed=False))
    link = await _make_link(client)
    token = link["token"]
    sid = (await client.post(f"/api/public/links/{token}/sessions", json={})).json()["session_id"]
    fr = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files", json={"path": "clip.mov", "size": 10}
    )
    cf = await client.post(
        f"/api/public/links/{token}/sessions/{sid}/files/{fr.json()['file_id']}/complete"
    )
    assert cf.json() == {"complete": False, "failed": False}
