"""Abandoned-upload sweep (issue #41): silent in_progress sessions get flagged "abandoned"."""

import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

import pytest_asyncio
from sqlalchemy import delete

from portal.db.models import Destination, UploadLink, UploadSession
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.services.upload_sessions import abandon_stale_upload_sessions
from portal.uploads.links import generate_token

NOW = datetime(2026, 7, 11, 12, 0, tzinfo=UTC)
STALE = NOW - timedelta(hours=2)
FRESH = NOW - timedelta(minutes=5)


@pytest_asyncio.fixture(autouse=True)
async def _clean(monkeypatch: Any) -> None:
    monkeypatch.setattr(get_settings(), "upload_stale_after_minutes", 30)
    async with get_sessionmaker()() as db:
        await db.execute(delete(UploadLink))  # sessions cascade
        await db.execute(delete(Destination))
        await db.commit()


async def _make_link_id() -> Any:
    async with get_sessionmaker()() as db:
        dest = Destination(display_name="D", config={"type": "frameio"})
        link = UploadLink(token=generate_token(), destination=dest)
        db.add_all([dest, link])
        await db.commit()
        await db.refresh(link)
        return link.id


async def _add_session(link_id: Any, **kw: Any) -> str:
    async with get_sessionmaker()() as db:
        session = UploadSession(upload_link_id=link_id, **kw)
        db.add(session)
        await db.commit()
        await db.refresh(session)
        return str(session.id)


async def _status(session_id: str) -> str:
    async with get_sessionmaker()() as db:
        row = await db.get(UploadSession, uuid.UUID(session_id))
        assert row is not None
        return row.status


async def _sweep() -> int:
    async with get_sessionmaker()() as db:
        return await abandon_stale_upload_sessions(db, now=NOW)


async def test_flags_stale_keeps_fresh_and_terminal() -> None:
    link_id = await _make_link_id()
    stale = await _add_session(link_id, status="in_progress", last_activity_at=STALE)
    fresh = await _add_session(link_id, status="in_progress", last_activity_at=FRESH)
    done = await _add_session(link_id, status="completed", started_at=STALE, completed_at=STALE)
    gone = await _add_session(link_id, status="abandoned", last_activity_at=STALE)

    assert await _sweep() == 1
    assert await _status(stale) == "abandoned"
    assert await _status(fresh) == "in_progress"
    assert await _status(done) == "completed"
    assert await _status(gone) == "abandoned"


async def test_fresh_heartbeat_outranks_old_started_at() -> None:
    link_id = await _make_link_id()
    # A legitimately long upload: started hours ago but still heartbeating.
    sid = await _add_session(
        link_id, status="in_progress", started_at=STALE, last_activity_at=FRESH
    )
    assert await _sweep() == 0
    assert await _status(sid) == "in_progress"


async def test_null_last_activity_falls_back_to_started_at() -> None:
    # Pre-heartbeat rows (e.g. the stuck session that prompted #41) have no last_activity_at;
    # the sweep must still catch them via started_at.
    link_id = await _make_link_id()
    sid = await _add_session(link_id, status="in_progress", started_at=NOW - timedelta(days=1))
    assert await _sweep() == 1
    assert await _status(sid) == "abandoned"


async def test_window_zero_disables_sweep(monkeypatch: Any) -> None:
    monkeypatch.setattr(get_settings(), "upload_stale_after_minutes", 0)
    link_id = await _make_link_id()
    sid = await _add_session(link_id, status="in_progress", last_activity_at=STALE)
    assert await _sweep() == 0
    assert await _status(sid) == "in_progress"
