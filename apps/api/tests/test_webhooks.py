import hashlib
import hmac
import json
import time
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import SyncJob, SyncRule, WebhookEvent
from portal.db.session import get_sessionmaker
from portal.frameio.webhooks import (
    SIGNATURE_HEADER,
    TIMESTAMP_HEADER,
    WebhookPayload,
    parse_webhook_payload,
    verify_signature,
)
from portal.routes import webhooks as webhooks_route
from portal.sync.events import is_sync_trigger, process_event, rule_matches

SECRET = "whsec_test_secret"
SOURCE = {
    "type": "frameio",
    "account_id": "acct-1",
    "workspace_id": "ws-1",
    "project_id": "proj-1",
    "folder_id": "fld-1",
}


def _sign(secret: str, timestamp: str, body: bytes) -> str:
    base = b"v0:" + timestamp.encode() + b":" + body
    return "v0=" + hmac.new(secret.encode(), base, hashlib.sha256).hexdigest()


def _file_payload(
    file_id: str = "file-1", event_type: str = "file.upload.completed"
) -> dict[str, Any]:
    return {
        "type": event_type,
        "account": {"id": "acct-1"},
        "workspace": {"id": "ws-1"},
        "project": {"id": "proj-1"},
        "resource": {"id": file_id, "type": "file"},
        "user": {"id": "user-1"},
    }


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(SyncJob))
        await db.execute(delete(SyncRule))
        await db.execute(delete(WebhookEvent))
        await db.commit()


async def _make_rule(*, enabled: bool = True, secret: str | None = SECRET) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        rule = SyncRule(
            name="rule",
            source_config=SOURCE,
            destination_path="/data/out",
            enabled=enabled,
            webhook_secret=secret,
        )
        db.add(rule)
        await db.commit()
        await db.refresh(rule)
        return rule.id


async def _make_event(payload: dict[str, Any]) -> uuid.UUID:
    async with get_sessionmaker()() as db:
        event = WebhookEvent(
            source="frameio",
            event_type=payload.get("type"),
            raw=payload,
            signature_valid=True,
        )
        db.add(event)
        await db.commit()
        await db.refresh(event)
        return event.id


# ── signature verification ─────────────────────────────────────────────────────


def test_verify_signature_valid() -> None:
    ts = str(int(time.time()))
    body = b'{"hello":"world"}'
    sig = _sign(SECRET, ts, body)
    assert verify_signature(SECRET, ts, body, sig) is True


def test_verify_signature_tampered_body() -> None:
    ts = str(int(time.time()))
    sig = _sign(SECRET, ts, b'{"hello":"world"}')
    assert verify_signature(SECRET, ts, b'{"hello":"evil"}', sig) is False


def test_verify_signature_wrong_secret() -> None:
    ts = str(int(time.time()))
    body = b"payload"
    sig = _sign(SECRET, ts, body)
    assert verify_signature("other-secret", ts, body, sig) is False


def test_verify_signature_replay_rejected() -> None:
    now = datetime.now(UTC)
    old = str(int((now - timedelta(minutes=10)).timestamp()))
    body = b"payload"
    sig = _sign(SECRET, old, body)
    assert verify_signature(SECRET, old, body, sig, now=now) is False


def test_verify_signature_within_window() -> None:
    now = datetime.now(UTC)
    recent = str(int((now - timedelta(minutes=2)).timestamp()))
    body = b"payload"
    sig = _sign(SECRET, recent, body)
    assert verify_signature(SECRET, recent, body, sig, now=now) is True


def test_verify_signature_missing_inputs() -> None:
    assert verify_signature("", "1", b"x", "v0=abc") is False
    assert verify_signature(SECRET, "", b"x", "v0=abc") is False
    assert verify_signature(SECRET, "1", b"x", "") is False
    assert verify_signature(SECRET, "not-a-number", b"x", "v0=abc") is False


# ── payload parsing / matching ──────────────────────────────────────────────────


def test_parse_webhook_payload() -> None:
    p = parse_webhook_payload(_file_payload())
    assert p == WebhookPayload(
        type="file.upload.completed",
        account_id="acct-1",
        workspace_id="ws-1",
        project_id="proj-1",
        resource_id="file-1",
        resource_type="file",
        user_id="user-1",
    )


def test_parse_webhook_payload_missing_fields() -> None:
    p = parse_webhook_payload({"type": "folder.created"})
    assert p.type == "folder.created"
    assert p.account_id is None
    assert p.resource_id is None


def test_rule_matches() -> None:
    p = parse_webhook_payload(_file_payload())
    assert rule_matches(SOURCE, p) is True
    assert rule_matches({**SOURCE, "project_id": "other"}, p) is False
    assert rule_matches({"account_id": "acct-1", "workspace_id": "ws-1"}, p) is False


def test_rule_matches_requires_all_ids_present() -> None:
    p = parse_webhook_payload({"type": "file.ready", "resource": {"id": "f", "type": "file"}})
    assert rule_matches(SOURCE, p) is False  # payload has no account/workspace/project


def test_is_sync_trigger() -> None:
    assert is_sync_trigger(parse_webhook_payload(_file_payload())) is True
    assert is_sync_trigger(parse_webhook_payload(_file_payload(event_type="file.ready"))) is True
    assert is_sync_trigger(parse_webhook_payload(_file_payload(event_type="file.updated"))) is False
    folder = {"type": "file.upload.completed", "resource": {"id": "x", "type": "folder"}}
    assert is_sync_trigger(parse_webhook_payload(folder)) is False


# ── processor ───────────────────────────────────────────────────────────────────


async def _jobs() -> list[SyncJob]:
    async with get_sessionmaker()() as db:
        return list((await db.execute(select(SyncJob))).scalars().all())


async def test_process_event_creates_job() -> None:
    await _make_rule()
    event_id = await _make_event(_file_payload())
    async with get_sessionmaker()() as db:
        created = await process_event(db, event_id)
    assert created == 1
    jobs = await _jobs()
    assert len(jobs) == 1
    assert jobs[0].frameio_file_id == "file-1"
    assert jobs[0].status == "pending"
    async with get_sessionmaker()() as db:
        ev = await db.get(WebhookEvent, event_id)
        assert ev is not None and ev.processed and ev.processed_at is not None


async def test_process_event_idempotent() -> None:
    await _make_rule()
    event_id = await _make_event(_file_payload())
    async with get_sessionmaker()() as db:
        assert await process_event(db, event_id) == 1
    # Re-processing the same event is a no-op (already processed).
    async with get_sessionmaker()() as db:
        assert await process_event(db, event_id) == 0
    assert len(await _jobs()) == 1


async def test_process_event_dedupes_same_file_across_events() -> None:
    await _make_rule()
    first = await _make_event(_file_payload(event_type="file.upload.completed"))
    second = await _make_event(_file_payload(event_type="file.ready"))
    async with get_sessionmaker()() as db:
        assert await process_event(db, first) == 1
    async with get_sessionmaker()() as db:
        assert await process_event(db, second) == 0  # same file, job already exists
    assert len(await _jobs()) == 1


async def test_process_event_skips_disabled_rule() -> None:
    await _make_rule(enabled=False)
    event_id = await _make_event(_file_payload())
    async with get_sessionmaker()() as db:
        assert await process_event(db, event_id) == 0
    assert await _jobs() == []


async def test_process_event_fires_callback_per_job() -> None:
    await _make_rule()
    event_id = await _make_event(_file_payload())
    seen: list[uuid.UUID] = []

    async def cb(job_id: uuid.UUID) -> None:
        seen.append(job_id)

    async with get_sessionmaker()() as db:
        assert await process_event(db, event_id, on_job_created=cb) == 1
    assert len(seen) == 1
    jobs = await _jobs()
    assert seen == [jobs[0].id]


async def test_process_event_non_trigger_marks_processed() -> None:
    await _make_rule()
    event_id = await _make_event(_file_payload(event_type="file.updated"))
    async with get_sessionmaker()() as db:
        assert await process_event(db, event_id) == 0
        ev = await db.get(WebhookEvent, event_id)
        assert ev is not None and ev.processed
    assert await _jobs() == []


# ── ingestion endpoint ──────────────────────────────────────────────────────────


def _no_enqueue(monkeypatch: Any) -> list[uuid.UUID]:
    enqueued: list[uuid.UUID] = []

    async def fake(event_id: uuid.UUID) -> None:
        enqueued.append(event_id)

    monkeypatch.setattr(webhooks_route, "enqueue_webhook_processing", fake)
    return enqueued


async def test_ingest_valid_signature(client: AsyncClient, monkeypatch: Any) -> None:
    enqueued = _no_enqueue(monkeypatch)
    rule_id = await _make_rule()
    body = json.dumps(_file_payload()).encode()
    ts = str(int(time.time()))
    resp = await client.post(
        f"/api/webhooks/frameio/{rule_id}",
        content=body,
        headers={TIMESTAMP_HEADER: ts, SIGNATURE_HEADER: _sign(SECRET, ts, body)},
    )
    assert resp.status_code == 200
    async with get_sessionmaker()() as db:
        events = list((await db.execute(select(WebhookEvent))).scalars().all())
    assert len(events) == 1
    assert events[0].signature_valid is True
    assert events[0].event_type == "file.upload.completed"
    assert enqueued == [events[0].id]


async def test_ingest_invalid_signature_persists_and_401(
    client: AsyncClient, monkeypatch: Any
) -> None:
    enqueued = _no_enqueue(monkeypatch)
    rule_id = await _make_rule()
    body = json.dumps(_file_payload()).encode()
    ts = str(int(time.time()))
    resp = await client.post(
        f"/api/webhooks/frameio/{rule_id}",
        content=body,
        headers={TIMESTAMP_HEADER: ts, SIGNATURE_HEADER: "v0=deadbeef"},
    )
    assert resp.status_code == 401
    async with get_sessionmaker()() as db:
        events = list((await db.execute(select(WebhookEvent))).scalars().all())
    assert len(events) == 1
    assert events[0].signature_valid is False
    assert enqueued == []  # not processed


async def test_ingest_unknown_rule_404(client: AsyncClient, monkeypatch: Any) -> None:
    _no_enqueue(monkeypatch)
    resp = await client.post(
        f"/api/webhooks/frameio/{uuid.uuid4()}",
        content=b"{}",
        headers={TIMESTAMP_HEADER: "1", SIGNATURE_HEADER: "v0=x"},
    )
    assert resp.status_code == 404


async def test_ingest_rule_without_secret_404(client: AsyncClient, monkeypatch: Any) -> None:
    _no_enqueue(monkeypatch)
    rule_id = await _make_rule(secret=None)
    resp = await client.post(
        f"/api/webhooks/frameio/{rule_id}",
        content=b"{}",
        headers={TIMESTAMP_HEADER: "1", SIGNATURE_HEADER: "v0=x"},
    )
    assert resp.status_code == 404


# ── admin event log ─────────────────────────────────────────────────────────────


async def test_admin_event_log(client: AsyncClient) -> None:
    await _make_event(_file_payload())
    # Unauthenticated → blocked.
    assert (await client.get("/api/webhook-events")).status_code == 401
    assert (
        await client.post(
            "/api/auth/login", json={"email": "admin@example.com", "password": "test-password"}
        )
    ).status_code == 200
    resp = await client.get("/api/webhook-events")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert rows[0]["event_type"] == "file.upload.completed"
