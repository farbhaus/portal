import uuid

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete

from portal.db.models import Destination, SyncRule, UploadLink
from portal.db.session import get_sessionmaker
from portal.frameio.client import FrameioError, FrameioNotConnected, PickerItem, ProjectItem
from portal.routes import destinations as dest_routes
from portal.services import frameio_paths
from portal.uploads.links import generate_token

CREDS = {"email": "admin@example.com", "password": "test-password"}

VALID_CONFIG = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "f1",
}

# What the picker sends: the full breadcrumb it already knows, so create resolves nothing.
CLIENT_PATH = [
    {"id": "root", "name": "Proj One (root)"},
    {"id": "sub", "name": "Sub"},
    {"id": "f1", "name": "Camera Originals"},
]


class _StubClient:
    def __init__(self) -> None:
        self.ancestry_calls = 0

    async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
        return PickerItem(id=folder_id, name="Camera Originals")

    async def folder_ancestry(self, account_id: str, folder_id: str) -> list[PickerItem]:
        self.ancestry_calls += 1
        return [
            PickerItem(id="root", name="root"),
            PickerItem(id=folder_id, name="Camera Originals"),
        ]

    async def get_project(self, account_id: str, project_id: str) -> ProjectItem:
        return ProjectItem(
            id=project_id, name="Proj One", root_folder_id="root", workspace_id="w1"
        )


@pytest_asyncio.fixture(autouse=True)
async def _clear_destinations() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncRule))
        await db.execute(delete(Destination))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


def _use_stub(monkeypatch, client_cls=_StubClient) -> _StubClient:
    stub = client_cls()
    # Both the route validation and the shared path-resolution helper hit Frame.io.
    monkeypatch.setattr(dest_routes, "get_frameio_client", lambda: stub)
    monkeypatch.setattr(frameio_paths, "get_frameio_client", lambda: stub)
    return stub


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


async def test_create_persists_client_supplied_path(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    stub = _use_stub(monkeypatch)
    resp = await client.post(
        "/api/destinations",
        json={
            "display_name": "DIT Drop",
            "config": {**VALID_CONFIG, "path": CLIENT_PATH, "project_name": "Proj One"},
        },
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["config"]["path"] == CLIENT_PATH
    assert body["config"]["project_name"] == "Proj One"
    assert body["config"]["path_resolved_at"]
    # The picker-supplied breadcrumb makes the rate-limited ancestry walk unnecessary.
    assert stub.ancestry_calls == 0


async def test_create_resolves_path_when_absent(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    stub = _use_stub(monkeypatch)
    resp = await client.post(
        "/api/destinations", json={"display_name": "X", "config": VALID_CONFIG}
    )
    assert resp.status_code == 201
    body = resp.json()
    assert stub.ancestry_calls == 1
    # Root ancestor labeled like the create pickers do.
    assert body["config"]["path"] == [
        {"id": "root", "name": "Proj One (root)"},
        {"id": "f1", "name": "Camera Originals"},
    ]
    assert body["config"]["project_name"] == "Proj One"


async def test_create_ignores_mismatched_client_path(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    stub = _use_stub(monkeypatch)
    bad_path = [{"id": "root", "name": "Root"}, {"id": "not-f1", "name": "Elsewhere"}]
    resp = await client.post(
        "/api/destinations",
        json={"display_name": "X", "config": {**VALID_CONFIG, "path": bad_path}},
    )
    assert resp.status_code == 201
    # A breadcrumb that doesn't end at the picked folder is dropped and re-resolved.
    assert resp.json()["config"]["path"][-1]["id"] == "f1"
    assert stub.ancestry_calls == 1


async def test_create_survives_path_resolution_failure(client: AsyncClient, monkeypatch) -> None:
    class _AncestryBoom(_StubClient):
        async def folder_ancestry(self, account_id: str, folder_id: str) -> list[PickerItem]:
            raise FrameioError("boom")

    await _login(client)
    _use_stub(monkeypatch, _AncestryBoom)
    resp = await client.post(
        "/api/destinations", json={"display_name": "X", "config": VALID_CONFIG}
    )
    # The breadcrumb is display-only; resolution failure must never fail the create.
    assert resp.status_code == 201
    assert "path" not in resp.json()["config"]


async def test_create_validates_folder_not_connected(client: AsyncClient, monkeypatch) -> None:
    class _NC(_StubClient):
        async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
            raise FrameioNotConnected("nope")

    await _login(client)
    _use_stub(monkeypatch, _NC)
    resp = await client.post(
        "/api/destinations", json={"display_name": "X", "config": VALID_CONFIG}
    )
    assert resp.status_code == 409


async def test_create_rejects_bad_folder(client: AsyncClient, monkeypatch) -> None:
    class _Bad(_StubClient):
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


async def test_detail_get_backfills_missing_path(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    stub = _use_stub(monkeypatch)
    created = (
        await client.post(
            "/api/destinations",
            json={"display_name": "One", "config": {**VALID_CONFIG, "path": CLIENT_PATH}},
        )
    ).json()
    assert stub.ancestry_calls == 0

    # Simulate a legacy row created before breadcrumbs were cached.
    async with get_sessionmaker()() as db:
        dest = await db.get(Destination, uuid.UUID(created["id"]))
        assert dest is not None
        config = dict(dest.config)
        config.pop("path")
        config.pop("path_resolved_at")
        dest.config = config
        await db.commit()

    got = await client.get(f"/api/destinations/{created['id']}")
    assert got.status_code == 200
    assert got.json()["config"]["path"] == [
        {"id": "root", "name": "Proj One (root)"},
        {"id": "f1", "name": "Camera Originals"},
    ]
    assert stub.ancestry_calls == 1

    # Backfill is persisted, so the next detail GET serves the cached breadcrumb.
    assert (await client.get(f"/api/destinations/{created['id']}")).status_code == 200
    assert stub.ancestry_calls == 1


async def test_responses_embed_sync_rules_and_upload_links(
    client: AsyncClient, monkeypatch
) -> None:
    await _login(client)
    _use_stub(monkeypatch)
    created = (
        await client.post(
            "/api/destinations",
            json={"display_name": "One", "config": {**VALID_CONFIG, "path": CLIENT_PATH}},
        )
    ).json()

    def _src(**over: object) -> dict:
        return {"type": "frameio", "account_id": "a1", "folder_id": "f1", **over}

    async with get_sessionmaker()() as db:
        db.add(SyncRule(name="exact", source_config=_src(), destination_path="/d/exact"))
        db.add(
            SyncRule(
                name="parent",
                source_config=_src(folder_id="sub", recursive=True),
                destination_path="/d/parent",
                enabled=False,
            )
        )
        db.add(
            SyncRule(
                name="parent-shallow",
                source_config=_src(folder_id="sub", recursive=False),
                destination_path="/d/shallow",
            )
        )
        db.add(
            SyncRule(
                name="other-account",
                source_config=_src(account_id="zz"),
                destination_path="/d/other",
            )
        )
        db.add(
            UploadLink(
                token=generate_token(),
                destination_id=uuid.UUID(created["id"]),
                brand_display_name="Cam A",
            )
        )
        await db.commit()

    listed = (await client.get("/api/destinations")).json()
    dest = listed[0]
    covering = {(r["name"], r["exact"], r["enabled"]) for r in dest["sync_rules"]}
    assert covering == {("exact", True, True), ("parent", False, False)}
    assert [(li["label"], li["state"]) for li in dest["upload_links"]] == [("Cam A", "ok")]

    # The detail GET carries the same enrichment (the hub and wizard rely on it).
    got = (await client.get(f"/api/destinations/{created['id']}")).json()
    assert {r["name"] for r in got["sync_rules"]} == {"exact", "parent"}


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
