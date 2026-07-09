import uuid
from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.frameio.client import (
    FrameioRateLimited,
    PickerItem,
    ProjectItem,
    WebhookSubscription,
)
from portal.routes import sync_rules as routes
from portal.services import frameio_paths

CREDS = {"email": "admin@example.com", "password": "test-password"}
SOURCE = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "fld-1",
    "recursive": True,
}
# What the UI sends — names included so create makes no Frame.io lookup calls.
SOURCE_WITH_NAMES = {**SOURCE, "folder_name": "Dailies", "project_name": "Acme"}
# The picker also sends the breadcrumb, so even the ancestry walk is skipped.
SOURCE_FULL = {
    **SOURCE_WITH_NAMES,
    "path": [{"id": "rf", "name": "Acme (root)"}, {"id": "fld-1", "name": "Dailies"}],
}


class _StubClient:
    def __init__(self) -> None:
        self.deleted: list[str] = []
        self.created = 0
        self.get_folder_calls = 0
        self.list_projects_calls = 0
        self.ancestry_calls = 0
        self.webhook_error: Exception | None = None

    async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
        self.get_folder_calls += 1
        return PickerItem(id=folder_id, name="Dailies")

    async def list_projects(self, account_id: str, workspace_id: str) -> list[ProjectItem]:
        self.list_projects_calls += 1
        return [ProjectItem(id="p1", name="Acme", root_folder_id="rf")]

    async def folder_ancestry(self, account_id: str, folder_id: str) -> list[PickerItem]:
        self.ancestry_calls += 1
        return [PickerItem(id="rf", name="root"), PickerItem(id=folder_id, name="Dailies")]

    async def get_project(self, account_id: str, project_id: str) -> ProjectItem:
        return ProjectItem(id=project_id, name="Acme", root_folder_id="rf", workspace_id="w1")

    async def create_webhook(
        self, account_id: str, workspace_id: str, *, name: str, url: str, events: list[str]
    ) -> WebhookSubscription:
        if self.webhook_error is not None:
            raise self.webhook_error
        self.created += 1
        return WebhookSubscription(id="wh-1", secret="whsec-xyz")

    async def delete_webhook(self, account_id: str, webhook_id: str) -> None:
        self.deleted.append(webhook_id)


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.commit()


@pytest_asyncio.fixture
def stub(monkeypatch: Any) -> _StubClient:
    client = _StubClient()
    monkeypatch.setattr(routes, "get_frameio_client", lambda: client)
    # The shared breadcrumb-resolution helper hits Frame.io through its own module.
    monkeypatch.setattr(frameio_paths, "get_frameio_client", lambda: client)
    return client


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_create_enabled_subscribes(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/sync-rules",
        json={"name": "Rule A", "source": SOURCE, "destination_path": "/data/out"},
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["subscription_active"] is True
    assert body["source"]["folder_name"] == "Dailies"
    assert body["source"]["project_name"] == "Acme"
    assert stub.created == 1
    # secret stored, never returned in the API payload
    assert "webhook_secret" not in body
    async with get_sessionmaker()() as db:
        rule = (await db.execute(select(SyncRule))).scalar_one()
        assert rule.webhook_secret == "whsec-xyz"
        assert rule.frameio_webhook_id == "wh-1"


async def test_create_with_names_skips_lookups(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/sync-rules",
        json={"name": "R", "source": SOURCE_WITH_NAMES, "destination_path": "/mnt/sync"},
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["source"]["folder_name"] == "Dailies"
    assert body["source"]["project_name"] == "Acme"
    # The throttled folder/project lookups are skipped entirely when the UI supplies names;
    # only the display breadcrumb still gets resolved (best-effort) when it isn't sent.
    assert stub.get_folder_calls == 0
    assert stub.list_projects_calls == 0
    assert stub.ancestry_calls == 1
    assert stub.created == 1


async def test_create_with_path_makes_no_frameio_lookups(
    client: AsyncClient, stub: _StubClient
) -> None:
    await _login(client)
    resp = await client.post(
        "/api/sync-rules",
        json={"name": "R", "source": SOURCE_FULL, "destination_path": "/mnt/sync"},
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["source"]["path"] == SOURCE_FULL["path"]
    assert body["source"]["path_resolved_at"]
    # The full picker payload (names + breadcrumb) keeps creation at zero Frame.io calls.
    assert stub.get_folder_calls == 0
    assert stub.list_projects_calls == 0
    assert stub.ancestry_calls == 0


async def test_detail_get_backfills_missing_path(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE_FULL, "destination_path": "/d", "enabled": False},
        )
    ).json()["id"]

    # Simulate a legacy rule created before breadcrumbs were cached.
    async with get_sessionmaker()() as db:
        rule = await db.get(SyncRule, uuid.UUID(rid))
        assert rule is not None
        source = dict(rule.source_config)
        source.pop("path")
        source.pop("path_resolved_at")
        rule.source_config = source
        await db.commit()

    got = await client.get(f"/api/sync-rules/{rid}")
    assert got.status_code == 200
    assert got.json()["source"]["path"] == [
        {"id": "rf", "name": "Acme (root)"},
        {"id": "fld-1", "name": "Dailies"},
    ]
    assert stub.ancestry_calls == 1

    # Backfill is persisted, so the next detail GET serves the cached breadcrumb.
    assert (await client.get(f"/api/sync-rules/{rid}")).status_code == 200
    assert stub.ancestry_calls == 1


async def test_create_rate_limited_returns_503(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    stub.webhook_error = FrameioRateLimited("busy")
    resp = await client.post(
        "/api/sync-rules",
        json={"name": "R", "source": SOURCE_WITH_NAMES, "destination_path": "/mnt/sync"},
    )
    assert resp.status_code == 503
    assert "try again" in resp.json()["detail"].lower()


async def test_create_disabled_no_subscription(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/sync-rules",
        json={"name": "R", "source": SOURCE, "destination_path": "/data", "enabled": False},
    )
    assert resp.status_code == 201
    assert resp.json()["subscription_active"] is False
    assert stub.created == 0


async def test_enable_disable_toggles_subscription(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE, "destination_path": "/d", "enabled": False},
        )
    ).json()["id"]

    enabled = await client.patch(f"/api/sync-rules/{rid}", json={"enabled": True})
    assert enabled.json()["subscription_active"] is True
    assert stub.created == 1

    disabled = await client.patch(f"/api/sync-rules/{rid}", json={"enabled": False})
    assert disabled.json()["subscription_active"] is False
    assert stub.deleted == ["wh-1"]


async def test_bad_conflict_policy_rejected(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    resp = await client.post(
        "/api/sync-rules",
        json={
            "name": "R",
            "source": SOURCE,
            "destination_path": "/d",
            "conflict_policy": "explode",
            "enabled": False,
        },
    )
    assert resp.status_code == 400


async def test_run_reconciles_and_reports_count(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE, "destination_path": "/d", "enabled": False},
        )
    ).json()["id"]
    # A disabled rule reconciles to zero new jobs without touching the backend, so /run reports
    # created == 0 — the signal the UI turns into "already up to date" rather than a silent no-op.
    resp = await client.post(f"/api/sync-rules/{rid}/run")
    assert resp.status_code == 200
    assert resp.json() == {"ok": True, "created": 0}


async def test_jobs_and_stats(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE, "destination_path": "/d", "enabled": False},
        )
    ).json()["id"]
    async with get_sessionmaker()() as db:
        db.add(SyncJob(sync_rule_id=uuid.UUID(rid), frameio_file_id="f1", status="done", bytes=10))
        db.add(SyncJob(sync_rule_id=uuid.UUID(rid), frameio_file_id="f2", status="pending"))
        await db.commit()

    jobs = await client.get(f"/api/sync-rules/{rid}/jobs")
    assert jobs.status_code == 200
    assert len(jobs.json()) == 2

    stats = await client.get(f"/api/sync-rules/{rid}/stats")
    assert stats.json()["counts"] == {"done": 1, "pending": 1}


async def test_delete_unsubscribes(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE, "destination_path": "/d"},
        )
    ).json()["id"]
    resp = await client.delete(f"/api/sync-rules/{rid}")
    assert resp.status_code == 204
    assert stub.deleted == ["wh-1"]
    async with get_sessionmaker()() as db:
        assert (await db.execute(select(SyncRule))).first() is None


async def test_requires_admin(client: AsyncClient) -> None:
    assert (await client.get("/api/sync-rules")).status_code == 401
