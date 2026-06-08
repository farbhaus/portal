"""Persistence and token lifecycle for the Frame.io connection.

Tokens are encrypted at rest with AES-GCM (lib.crypto.TokenCipher). `get_valid_access_token`
is the refresh-on-expiry core: it transparently refreshes via Adobe IMS when the access token
is expired or about to expire.
"""

import uuid
from datetime import UTC, datetime
from functools import lru_cache

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import FrameioConnection
from portal.frameio import oauth
from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher
from portal.lib.logging import get_logger

log = get_logger("frameio.connection")

# Refresh when the access token has this many seconds (or fewer) of life left.
REFRESH_SKEW_SECONDS = 300


@lru_cache
def _cipher() -> TokenCipher:
    return TokenCipher(get_settings().token_encryption_key)


async def get_connection(db: AsyncSession, user_id: uuid.UUID) -> FrameioConnection | None:
    result = await db.execute(
        select(FrameioConnection).where(FrameioConnection.user_id == user_id)
    )
    return result.scalar_one_or_none()


async def get_primary_connection(db: AsyncSession) -> FrameioConnection | None:
    """The single connection used for API access (v1 has one admin / one connection)."""
    result = await db.execute(
        select(FrameioConnection).order_by(FrameioConnection.created_at).limit(1)
    )
    return result.scalar_one_or_none()


async def upsert_connection(
    db: AsyncSession,
    user_id: uuid.UUID,
    tokens: oauth.TokenSet,
    identity: oauth.Identity,
) -> FrameioConnection:
    cipher = _cipher()
    conn = await get_connection(db, user_id)
    if conn is None:
        conn = FrameioConnection(user_id=user_id)
        db.add(conn)
    conn.access_token_encrypted = cipher.encrypt(tokens.access_token)
    if tokens.refresh_token:
        conn.refresh_token_encrypted = cipher.encrypt(tokens.refresh_token)
    conn.token_expires_at = tokens.expires_at
    conn.adobe_ims_user_id = identity.frameio_user_id
    conn.adobe_email = identity.email
    await db.commit()
    await db.refresh(conn)
    return conn


def needs_refresh(conn: FrameioConnection, skew: int = REFRESH_SKEW_SECONDS) -> bool:
    if conn.token_expires_at is None:
        return True
    expires_at = conn.token_expires_at
    if expires_at.tzinfo is None:
        expires_at = expires_at.replace(tzinfo=UTC)
    remaining = (expires_at - datetime.now(UTC)).total_seconds()
    return remaining <= skew


async def get_valid_access_token(db: AsyncSession, conn: FrameioConnection) -> str:
    """Return a usable access token, refreshing via IMS first if it is expired/near-expiry."""
    cipher = _cipher()
    if not needs_refresh(conn):
        if conn.access_token_encrypted is None:
            raise oauth.FrameioOAuthError("No access token stored")
        return cipher.decrypt(conn.access_token_encrypted)

    if conn.refresh_token_encrypted is None:
        raise oauth.FrameioOAuthError("No refresh token; reconnect required")

    refresh_token = cipher.decrypt(conn.refresh_token_encrypted)
    tokens = await oauth.refresh_tokens(refresh_token)
    conn.access_token_encrypted = cipher.encrypt(tokens.access_token)
    if tokens.refresh_token:
        conn.refresh_token_encrypted = cipher.encrypt(tokens.refresh_token)
    conn.token_expires_at = tokens.expires_at
    await db.commit()
    await db.refresh(conn)
    log.info("frameio.connection.refreshed", user_id=str(conn.user_id))
    return tokens.access_token
