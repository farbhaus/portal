import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import AuditLog, User
from portal.db.session import get_sessionmaker

CREDS = {"email": "admin@example.com", "password": "test-password"}


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(AuditLog))
        await db.commit()


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def _seed() -> None:
    async with get_sessionmaker()() as db:
        admin = (await db.execute(select(User))).scalars().first()
        assert admin is not None
        db.add(AuditLog(user_id=admin.id, action="destination.created", detail={"x": "1"}))
        db.add(AuditLog(user_id=admin.id, action="sync_rule.created"))
        await db.commit()


async def test_activity_list_and_filter(client: AsyncClient) -> None:
    await _login(client)
    await _seed()

    rows = (await client.get("/api/activity")).json()
    assert len(rows) >= 2
    assert {"action", "user_email", "detail", "created_at"} <= set(rows[0].keys())
    assert rows[0]["user_email"] == "admin@example.com"

    filtered = (await client.get("/api/activity?action=sync_rule.created")).json()
    assert filtered and all(r["action"] == "sync_rule.created" for r in filtered)

    actions = (await client.get("/api/activity/actions")).json()
    assert "destination.created" in actions and "sync_rule.created" in actions


async def test_summary(client: AsyncClient) -> None:
    await _login(client)
    summary = (await client.get("/api/activity/summary")).json()
    assert {"destinations", "upload_links", "download_links", "sync_rules"} == set(summary.keys())
    assert all(isinstance(v, int) for v in summary.values())


async def test_activity_requires_admin(client: AsyncClient) -> None:
    assert (await client.get("/api/activity")).status_code == 401
    assert (await client.get("/api/activity/summary")).status_code == 401
