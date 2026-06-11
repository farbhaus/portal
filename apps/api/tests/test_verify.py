import json
import time
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from typing import Any

import dns.resolver
import pytest
import pytest_asyncio
from fastapi import Response
from sqlalchemy import delete

from portal.db.models import EmailVerification
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.verify import email_check, service, trust


@pytest_asyncio.fixture(autouse=True)
async def _clear() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(EmailVerification))
        await db.commit()


# ── email_check ────────────────────────────────────────────────────────────────


def test_valid_syntax() -> None:
    assert email_check.valid_syntax("a@b.co")
    assert email_check.valid_syntax("ze.maria+x@sub.domain.com")
    assert not email_check.valid_syntax("ze@")
    assert not email_check.valid_syntax("ze@ze")  # no dot in domain
    assert not email_check.valid_syntax("no-at-sign")


async def test_is_plausible_syntax_only() -> None:
    assert await email_check.is_plausible_email("a@b.co", check_mx=False)
    assert not await email_check.is_plausible_email("bad", check_mx=False)


async def test_is_plausible_with_mx(monkeypatch: Any) -> None:
    monkeypatch.setattr(email_check, "_domain_receives_mail", lambda d: d == "good.com")
    assert await email_check.is_plausible_email("x@good.com", check_mx=True)
    assert not await email_check.is_plausible_email("x@ze.ze", check_mx=True)


def test_domain_receives_mail(monkeypatch: Any) -> None:
    class FakeResolver:
        timeout = 0.0
        lifetime = 0.0

        def resolve(self, domain: str, record: str) -> list[str]:
            if domain == "nxdomain.test":
                raise dns.resolver.NXDOMAIN()
            if domain == "hasmx.test" and record == "MX":
                return ["mx1"]
            raise dns.resolver.NoAnswer()

    monkeypatch.setattr(dns.resolver, "Resolver", FakeResolver)
    assert email_check._domain_receives_mail("hasmx.test") is True
    assert email_check._domain_receives_mail("nxdomain.test") is False
    assert email_check._domain_receives_mail("noanswer.test") is False  # ze.ze-like


# ── trust cookie ───────────────────────────────────────────────────────────────


def _req(token: str | None) -> Any:
    return SimpleNamespace(cookies={trust.COOKIE_NAME: token} if token else {})


def test_is_trusted() -> None:
    cipher = trust._cipher()
    good = cipher.encrypt(json.dumps({"email": "ze@x.com", "exp": time.time() + 100}))
    assert trust.is_trusted(_req(good), "ze@x.com")
    assert trust.is_trusted(_req(good), "ZE@X.com")  # case-insensitive
    assert not trust.is_trusted(_req(good), "other@x.com")

    expired = cipher.encrypt(json.dumps({"email": "ze@x.com", "exp": time.time() - 1}))
    assert not trust.is_trusted(_req(expired), "ze@x.com")
    assert not trust.is_trusted(_req("garbage"), "ze@x.com")
    assert not trust.is_trusted(_req(None), "ze@x.com")


def test_issue_trust_sets_cookie() -> None:
    resp = Response()
    trust.issue_trust(resp, "ze@x.com")
    set_cookie = resp.headers.get("set-cookie") or ""
    assert trust.COOKIE_NAME in set_cookie
    assert "HttpOnly" in set_cookie


# ── code service ───────────────────────────────────────────────────────────────


def _enable_email(monkeypatch: Any) -> dict[str, str]:
    s = get_settings()
    monkeypatch.setattr(s, "smtp_host", "smtp.test")
    monkeypatch.setattr(s, "smtp_from", "portal@test")
    sent: dict[str, str] = {}

    async def fake_send(to: str, code: str, ttl: int) -> None:
        sent["to"] = to
        sent["code"] = code

    async def always_plausible(email: str, *, check_mx: bool) -> bool:
        return True

    monkeypatch.setattr(service, "send_verification_code", fake_send)
    monkeypatch.setattr(service, "is_plausible_email", always_plausible)
    return sent


async def test_request_then_verify(monkeypatch: Any) -> None:
    sent = _enable_email(monkeypatch)
    async with get_sessionmaker()() as db:
        await service.request_code(db, "ZE@x.com")
    assert sent["to"] == "ze@x.com"  # normalized
    code = sent["code"]
    assert len(code) == 6

    wrong = "000000" if code != "000000" else "111111"
    async with get_sessionmaker()() as db:
        assert await service.verify_code(db, "ze@x.com", wrong) is False
    async with get_sessionmaker()() as db:
        assert await service.verify_code(db, "ze@x.com", code) is True
    # consumed: a second verify finds no record
    async with get_sessionmaker()() as db:
        assert await service.verify_code(db, "ze@x.com", code) is False


async def test_request_cooldown(monkeypatch: Any) -> None:
    _enable_email(monkeypatch)
    async with get_sessionmaker()() as db:
        await service.request_code(db, "ze@x.com")
        with pytest.raises(service.ResendTooSoon):
            await service.request_code(db, "ze@x.com")


async def test_request_invalid_email(monkeypatch: Any) -> None:
    _enable_email(monkeypatch)

    async def never(email: str, *, check_mx: bool) -> bool:
        return False

    monkeypatch.setattr(service, "is_plausible_email", never)
    async with get_sessionmaker()() as db:
        with pytest.raises(service.InvalidEmail):
            await service.request_code(db, "ze@ze.ze")


async def test_request_unavailable_when_email_unconfigured() -> None:
    # No SMTP patched → email_configured is false in the test env.
    async with get_sessionmaker()() as db:
        with pytest.raises(service.VerificationUnavailable):
            await service.request_code(db, "ze@x.com")


async def test_verify_expired_and_max_attempts() -> None:
    now = datetime.now(UTC)
    async with get_sessionmaker()() as db:
        db.add(
            EmailVerification(
                email="exp@x.com",
                code_hash=service._hash("123456"),
                expires_at=now - timedelta(seconds=1),
                attempts=0,
            )
        )
        db.add(
            EmailVerification(
                email="max@x.com",
                code_hash=service._hash("123456"),
                expires_at=now + timedelta(minutes=10),
                attempts=999,
            )
        )
        await db.commit()
    async with get_sessionmaker()() as db:
        assert await service.verify_code(db, "exp@x.com", "123456") is False
        assert await service.verify_code(db, "max@x.com", "123456") is False
