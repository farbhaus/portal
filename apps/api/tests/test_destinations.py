import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination
from portal.db.session import get_sessionmaker
from portal.frameio.client import FrameioError, FrameioNotConnected, PickerItem
from portal.routes import destinations as dest_routes

CREDS = {"email": "admin@example.com", "password": "test-password"}

VALID_CONFIG = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "f1",
}


class _StubClient:
    async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
        return PickerItem(id=folder_id, name="Camera Originals")


@pytest_asyncio.fixture(autouse=True)
async def _clear_destinations() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(Destination))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


def _use_stub(monkeypatch, client_cls=_StubClient) -> None:
    monkeypatch.setattr(dest_routes, "get_frameio_client", lambda: client_cls())


# ── create ────────────────────────────────────────────────────────────────────


async def test_create_destination(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    resp = await client.post(
        "/api/destinations",
        json={
            "display_name": "DIT Drop",
            "config": VALID_CONFIG,
            "subtitle": "Send camera originals here",
        },
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["display_name"] == "DIT Drop"
    assert body["subtitle"] == "Send camera originals here"
    # Folder name resolved from Frame.io and cached in config.
    assert body["config"]["folder_name"] == "Camera Originals"
    assert body["config"]["folder_id"] == "f1"


async def test_create_validates_folder_not_connected(client: AsyncClient, monkeypatch) -> None:
    class _NC:
        async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
            raise FrameioNotConnected("nope")

    await _login(client)
    _use_stub(monkeypatch, _NC)
    resp = await client.post(
        "/api/destinations", json={"display_name": "X", "config": VALID_CONFIG}
    )
    assert resp.status_code == 409


async def test_create_rejects_bad_folder(client: AsyncClient, monkeypatch) -> None:
    class _Bad:
        async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
            raise FrameioError("not found")

    await _login(client)
    _use_stub(monkeypatch, _Bad)
    resp = await client.post(
        "/api/destinations", json={"display_name": "X", "config": VALID_CONFIG}
    )
    assert resp.status_code == 400


async def test_create_rejects_invalid_config(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    resp = await client.post(
        "/api/destinations",
        json={"display_name": "X", "config": {"type": "frameio", "account_id": "a1"}},
    )
    assert resp.status_code == 422


# ── list / get / update / delete ──────────────────────────────────────────────


async def test_list_and_get(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    created = (
        await client.post(
            "/api/destinations", json={"display_name": "One", "config": VALID_CONFIG}
        )
    ).json()

    listed = await client.get("/api/destinations")
    assert listed.status_code == 200
    assert [d["id"] for d in listed.json()] == [created["id"]]

    got = await client.get(f"/api/destinations/{created['id']}")
    assert got.status_code == 200
    assert got.json()["display_name"] == "One"


async def test_update_branding(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    created = (
        await client.post(
            "/api/destinations", json={"display_name": "One", "config": VALID_CONFIG}
        )
    ).json()

    resp = await client.patch(
        f"/api/destinations/{created['id']}",
        json={"display_name": "Renamed", "subtitle": "new subtitle"},
    )
    assert resp.status_code == 200
    assert resp.json()["display_name"] == "Renamed"
    assert resp.json()["subtitle"] == "new subtitle"
    # Untouched fields preserved.
    assert resp.json()["config"]["folder_id"] == "f1"


async def test_delete(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    created = (
        await client.post(
            "/api/destinations", json={"display_name": "One", "config": VALID_CONFIG}
        )
    ).json()

    resp = await client.delete(f"/api/destinations/{created['id']}")
    assert resp.status_code == 204
    assert (await client.get(f"/api/destinations/{created['id']}")).status_code == 404


async def test_get_missing_returns_404(client: AsyncClient) -> None:
    await _login(client)
    missing = "00000000-0000-0000-0000-000000000000"
    assert (await client.get(f"/api/destinations/{missing}")).status_code == 404


async def test_destinations_require_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/destinations")).status_code == 401
    assert (await client.post("/api/destinations", json={})).status_code == 401
