"""Frame.io V4 webhook signature verification and payload parsing.

Per the Webhooks guide (https://next.developer.frame.io/platform/docs/guides/webhooks):

- Delivery carries two headers: ``X-Frameio-Request-Timestamp`` (epoch seconds) and
  ``X-Frameio-Signature`` (``v0=<hex>``).
- The signature is ``HMAC-SHA256(secret, "v0:<timestamp>:<raw body>")``, hex-encoded and
  prefixed with ``v0=``. The signing secret is returned only once, when the webhook is created.
- Verifiers must reject deliveries whose timestamp is more than ~5 minutes from now (replay
  protection).

Scope note (deviates from the briefing): Frame.io webhooks are scoped to a *workspace*, not a
folder. A subscription fires for every project in the workspace, so folder-level matching is the
processor's job (see ``portal.sync.events``), not the subscription's.
"""

from __future__ import annotations

import hashlib
import hmac
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

# HTTP headers carrying the signature material.
TIMESTAMP_HEADER = "X-Frameio-Request-Timestamp"
SIGNATURE_HEADER = "X-Frameio-Signature"

# Replay window: reject deliveries whose timestamp is further than this from now.
DEFAULT_TOLERANCE_SECONDS = 300

# File-arrival events that should trigger a sync. ``file.upload.completed`` fires once the bytes
# are fully uploaded; ``file.ready`` once transcodes finish. Either is a usable trigger; we accept
# both and dedupe downstream so a single file produces one sync job, not two.
SYNC_TRIGGER_EVENTS = frozenset({"file.upload.completed", "file.ready"})


@dataclass(frozen=True)
class WebhookPayload:
    """The fields Portal cares about, lifted out of a Frame.io webhook delivery body."""

    type: str | None
    account_id: str | None
    workspace_id: str | None
    project_id: str | None
    resource_id: str | None
    resource_type: str | None
    user_id: str | None


def verify_signature(
    secret: str,
    timestamp: str,
    body: bytes,
    signature: str,
    *,
    now: datetime | None = None,
    tolerance_seconds: int = DEFAULT_TOLERANCE_SECONDS,
) -> bool:
    """Return True iff ``signature`` is a valid, in-window Frame.io signature for ``body``.

    ``body`` is the raw request body (bytes), exactly as received — re-serializing parsed JSON
    would change whitespace and break the HMAC.
    """
    if not secret or not signature or not timestamp:
        return False

    try:
        sent_at = int(timestamp)
    except ValueError:
        return False

    current = int((now or datetime.now(UTC)).timestamp())
    if abs(current - sent_at) > tolerance_seconds:
        return False

    basestring = b"v0:" + timestamp.encode() + b":" + body
    expected = "v0=" + hmac.new(secret.encode(), basestring, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature)


def parse_webhook_payload(raw: dict[str, Any]) -> WebhookPayload:
    """Lift the relevant ids out of a delivery body (nested ``{id}`` objects, flat ``type``)."""

    def _id(key: str) -> str | None:
        value = raw.get(key)
        if isinstance(value, dict):
            ident = value.get("id")
            return ident if isinstance(ident, str) else None
        return None

    resource = raw.get("resource")
    resource = resource if isinstance(resource, dict) else {}
    rtype = raw.get("type")
    return WebhookPayload(
        type=rtype if isinstance(rtype, str) else None,
        account_id=_id("account"),
        workspace_id=_id("workspace"),
        project_id=_id("project"),
        resource_id=resource.get("id") if isinstance(resource.get("id"), str) else None,
        resource_type=resource.get("type") if isinstance(resource.get("type"), str) else None,
        user_id=_id("user"),
    )
