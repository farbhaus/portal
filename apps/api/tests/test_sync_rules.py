import uuid
from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import SyncJob, SyncRule
from portal.db.session import get_sessionmaker
from portal.frameio.client import PickerItem, ProjectItem, WebhookSubscription
from portal.routes import sync_rules as routes

CREDS = {"email": "admin@example.com", "password": "test-password"}
SOURCE = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "fld-1",
    "recursive": True,
}


class _StubClient:
    def __init__(self) -> None:
        self.deleted: list[str] = []
        self.created = 0

    async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
        return PickerItem(id=folder_id, name="Dailies")

    async def list_projects(self, account_id: str, workspace_id: str) -> list[ProjectItem]:
        return [ProjectItem(id="p1", name="Acme", root_folder_id="rf")]

    async def create_webhook(
        self, account_id: str, workspace_id: str, *, name: str, url: str, events: list[str]
    ) -> WebhookSubscription:
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
    enqueued: list[uuid.UUID] = []

    async def fake_enqueue(rule_id: uuid.UUID) -> None:
        enqueued.append(rule_id)

    monkeypatch.setattr(routes, "enqueue_reconcile_rule", fake_enqueue)
    client.enqueued = enqueued  # type: ignore[attr-defined]
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


async def test_run_enqueues_reconcile(client: AsyncClient, stub: _StubClient) -> None:
    await _login(client)
    rid = (
        await client.post(
            "/api/sync-rules",
            json={"name": "R", "source": SOURCE, "destination_path": "/d", "enabled": False},
        )
    ).json()["id"]
    resp = await client.post(f"/api/sync-rules/{rid}/run")
    assert resp.status_code == 200
    assert stub.enqueued == [uuid.UUID(rid)]  # type: ignore[attr-defined]


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
