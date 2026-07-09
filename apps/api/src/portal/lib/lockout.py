"""Global per-token lockout for link password attempts.

The per-IP strict rate limit (portal.lib.ratelimit) slows one client down, but a distributed
attacker gets a fresh per-minute budget with every IP, so a weak link password would still fall
to grinding. This counter is global per token: every wrong password from anywhere counts toward
the same fixed window, and past the cap all password checks for that token answer 429 — even
with the correct password — until the window expires. Only holders of the (unguessable) link URL
can trip it, so the griefing surface is limited to the link's own audience.
"""

from fastapi import HTTPException

from portal.lib.config import get_settings
from portal.sync.queue import get_arq_pool

_PREFIX = "pwlock"


def _key(kind: str, token: str) -> str:
    return f"{_PREFIX}:{kind}:{token}"


async def check_password_lockout(kind: str, token: str) -> None:
    """Raise 429 when this token's password-failure budget is already exhausted."""
    settings = get_settings()
    if not settings.rate_limit_enabled:
        return
    redis = await get_arq_pool()
    count = await redis.get(_key(kind, token))
    if count is not None and int(count) >= settings.password_lockout_max_failures:
        raise HTTPException(
            status_code=429,
            detail="Too many incorrect password attempts — try again later",
        )


async def register_password_failure(kind: str, token: str) -> None:
    """Count one wrong password toward the token's fixed-window budget."""
    settings = get_settings()
    if not settings.rate_limit_enabled:
        return
    redis = await get_arq_pool()
    key = _key(kind, token)
    count = await redis.incr(key)
    if count == 1:
        await redis.expire(key, settings.password_lockout_window_seconds)
