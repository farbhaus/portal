"""Redis-backed fixed-window rate limiting for public (unauthenticated) endpoints.

Keyed by (kind, token, client IP). Behind the host Caddy the socket peer is the proxy, so we read
the real client from X-Forwarded-For. Per-file upload/download endpoints are deliberately left
unlimited — they fire many times within one legitimate transfer.
"""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import Literal, cast

from fastapi import Depends, HTTPException, Request, params

from portal.lib.config import get_settings
from portal.sync.queue import get_arq_pool

Kind = Literal["default", "strict"]


def _client_ip(request: Request) -> str:
    xff = request.headers.get("x-forwarded-for")
    if xff:
        return xff.split(",")[0].strip()
    return request.client.host if request.client else "unknown"


def rate_limit(kind: Kind) -> Callable[[Request], Awaitable[None]]:
    """Build a FastAPI dependency enforcing the per-minute budget for `kind`."""

    async def dependency(request: Request) -> None:
        settings = get_settings()
        if not settings.rate_limit_enabled:
            return
        per_minute = (
            settings.rate_limit_strict_per_minute
            if kind == "strict"
            else settings.rate_limit_default_per_minute
        )
        token = request.path_params.get("token", "-")
        key = f"rl:{kind}:{token}:{_client_ip(request)}"
        redis = await get_arq_pool()
        count = await redis.incr(key)
        if count == 1:
            await redis.expire(key, 60)
        if count > per_minute:
            raise HTTPException(status_code=429, detail="Too many requests. Please slow down.")

    return dependency


def limit(kind: Kind) -> params.Depends:
    """Convenience wrapper for use in a route's `dependencies=[...]`."""
    return cast(params.Depends, Depends(rate_limit(kind)))
