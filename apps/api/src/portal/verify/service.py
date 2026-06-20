"""Issue and check one-time email verification codes (table: email_verifications)."""

from __future__ import annotations

import hashlib
import hmac
import secrets
from datetime import UTC, datetime, timedelta

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import EmailVerification
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.notify.email import send_verification_code
from portal.services.app_settings import email_brand
from portal.services.runtime_config import get_email_config
from portal.verify.email_check import is_plausible_email

log = get_logger("verify.service")


class VerificationError(Exception):
    """Code request couldn't be fulfilled. Carries an HTTP status + public detail."""

    status_code = 400
    detail = "Verification failed"


class InvalidEmail(VerificationError):
    detail = "That email address looks invalid"


class ResendTooSoon(VerificationError):
    status_code = 429
    detail = "Please wait a moment before requesting another code"


class VerificationUnavailable(VerificationError):
    status_code = 503
    detail = "Email verification is temporarily unavailable"


def _norm(email: str) -> str:
    return email.strip().lower()


def _hash(code: str) -> str:
    return hashlib.sha256(code.strip().encode("utf-8")).hexdigest()


async def request_code(db: AsyncSession, email: str) -> None:
    """Validate the address, enforce the resend cooldown, email a fresh code, and store its hash."""
    settings = get_settings()
    email_config = await get_email_config(db)
    if not email_config.configured:
        raise VerificationUnavailable()

    email = _norm(email)
    if not await is_plausible_email(email, check_mx=settings.verify_check_mx):
        raise InvalidEmail()

    now = datetime.now(UTC)
    existing = await db.scalar(
        select(EmailVerification).where(EmailVerification.email == email)
    )
    if existing is not None:
        elapsed = (now - existing.last_sent_at).total_seconds()
        if elapsed < settings.verify_resend_cooldown_seconds:
            raise ResendTooSoon()

    code = f"{secrets.randbelow(1_000_000):06d}"
    expires = now + timedelta(seconds=settings.verify_code_ttl_seconds)
    brand = await email_brand(db, settings.base_url)
    # Send first; only persist if the email actually went out (no orphan codes on SMTP failure).
    await send_verification_code(
        email_config, email, code, settings.verify_code_ttl_seconds // 60, brand
    )

    if existing is None:
        db.add(
            EmailVerification(
                email=email, code_hash=_hash(code), expires_at=expires, attempts=0, last_sent_at=now
            )
        )
    else:
        existing.code_hash = _hash(code)
        existing.expires_at = expires
        existing.attempts = 0
        existing.last_sent_at = now
    await db.commit()
    log.info("verify.code_sent", email=email)


async def verify_code(db: AsyncSession, email: str, code: str) -> bool:
    """True iff `code` matches the active, unexpired code for `email` (consumed on success)."""
    settings = get_settings()
    email = _norm(email)
    rec = await db.scalar(select(EmailVerification).where(EmailVerification.email == email))
    if rec is None:
        return False
    now = datetime.now(UTC)
    if rec.expires_at < now or rec.attempts >= settings.verify_max_attempts:
        return False

    rec.attempts += 1
    if hmac.compare_digest(rec.code_hash, _hash(code)):
        await db.delete(rec)
        await db.commit()
        log.info("verify.code_ok", email=email)
        return True
    await db.commit()
    return False
