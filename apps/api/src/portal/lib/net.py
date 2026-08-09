"""Resolving the real client address behind Portal's reverse-proxy chain.

The API is never reached directly: the socket peer is always the innermost proxy (the container's
Caddy on loopback, or the docker bridge gateway when a host proxy forwards in). Anything that wants
the *recipient's* address — rate-limit keying, the download audit log — must go through
`client_ip`, never `request.client.host`.
"""

from __future__ import annotations

from fastapi import Request

from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("lib.net")


def client_ip(request: Request) -> str:
    """Resolve the real client IP from the trusted hop of X-Forwarded-For.

    Each trusted proxy appends the address it received the request from, so with N trusted hops the
    genuine client is the Nth entry from the right; everything to its left is client-supplied and
    untrusted. Reading the leftmost entry instead would let a client forge X-Forwarded-For — landing
    in a fresh rate-limit bucket per request, and writing a made-up address into the download log.

    Returns "unknown" only when there is no usable address at all.
    """
    hops = get_settings().trusted_proxy_hops
    xff = request.headers.get("x-forwarded-for")
    if xff and hops > 0:
        parts = [p.strip() for p in xff.split(",") if p.strip()]
        if len(parts) >= hops:
            return parts[-hops]
        # Fewer hops than configured means the chain is shorter than TRUSTED_PROXY_HOPS claims —
        # usually a host proxy that doesn't forward X-Forwarded-For. Falling through would record
        # the proxy's own address as the client, so say so rather than logging a useless IP.
        log.warning(
            "net.xff_shorter_than_trusted_hops",
            trusted_proxy_hops=hops,
            forwarded_for_entries=len(parts),
        )
    return request.client.host if request.client else "unknown"
