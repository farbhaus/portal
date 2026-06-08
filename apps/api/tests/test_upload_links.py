from datetime import UTC, datetime, timedelta

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination, UploadLink
from portal.db.session import get_sessionmaker
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
