"""Frame.io webhook ingestion (public) and the event log (admin).

Ingestion lives at ``/api/webhooks/frameio/{rule_id}``: the rule id in the path tells us which
sync rule's stored signing secret to verify against (Frame.io subscriptions are per-rule, each
with its own secret — created in Step 11 when a rule is enabled). The endpoint:

  1. reads the raw body (needed verbatim for the HMAC),
  2. verifies the signature against the rule's secret,
  3. persists the event (always — valid or not, for audit; ``webhook_events`` is append-only),
  4. on a valid signature, enqueues async processing and returns 200 fast.

Invalid signatures are persisted with ``signature_valid=False`` and answered 401 (no processing).
Frame.io internals are never echoed to the caller.
"""

import json
import uuid
from typing import Any

from fastapi import APIRouter, Depends, Request, Response
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import SyncRule, User, WebhookEvent
from portal.db.session import get_session
from portal.frameio.webhooks import (
    SIGNATURE_HEADER,
    TIMESTAMP_HEADER,
    parse_webhook_payload,
    verify_signature,
)
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.sync.queue import enqueue_webhook_processing

log = get_logger("routes.webhooks")

router = APIRouter(prefix="/webhooks", tags=["webhooks"])
admin_router = APIRouter(prefix="/webhook-events", tags=["webhooks"])


@router.post("/frameio/{rule_id}")
async def ingest_frameio(
    rule_id: uuid.UUID,
    request: Request,
    db: AsyncSession = Depends(get_session),
) -> Response:
    rule = await db.get(SyncRule, rule_id)
    if rule is None or not rule.webhook_secret:
        # Unknown rule or no secret yet: nothing to verify against. Don't persist unattributable
        # events. 404 is a terminal signal to Frame.io (it stops retrying after its attempts).
        return Response(status_code=404)

    body = await request.body()
    timestamp = request.headers.get(TIMESTAMP_HEADER, "")
    signature = request.headers.get(SIGNATURE_HEADER, "")
    valid = verify_signature(
        rule.webhook_secret,
        timestamp,
        body,
        signature,
        tolerance_seconds=get_settings().webhook_signature_tolerance_seconds,
    )

    try:
        raw: dict[str, Any] = json.loads(body)
        if not isinstance(raw, dict):
            raw = {"_raw": body.decode("utf-8", "replace")}
    except (ValueError, UnicodeDecodeError):
        raw = {"_unparseable": True}

    event = WebhookEvent(
        source="frameio",
        event_type=parse_webhook_payload(raw).type,
        raw=raw,
        signature_valid=valid,
    )
    db.add(event)
    await db.commit()
    await db.refresh(event)

    if not valid:
        log.warning("webhook.signature_invalid", rule_id=str(rule_id), event_id=str(event.id))
        return Response(status_code=401)

    await enqueue_webhook_processing(event.id)
    log.info("webhook.ingested", rule_id=str(rule_id), event_id=str(event.id))
    return Response(status_code=200)


class WebhookEventOut(BaseModel):
    id: str
    source: str
    event_type: str | None
    signature_valid: bool | None
    processed: bool
    processed_at: str | None
    error: str | None
    received_at: str


@admin_router.get("", response_model=list[WebhookEventOut])
async def list_events(
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    limit: int = 100,
) -> list[WebhookEventOut]:
    limit = max(1, min(limit, 500))
    result = await db.execute(
        select(WebhookEvent).order_by(WebhookEvent.received_at.desc()).limit(limit)
    )
    return [
        WebhookEventOut(
            id=str(e.id),
            source=e.source,
            event_type=e.event_type,
            signature_valid=e.signature_valid,
            processed=e.processed,
            processed_at=e.processed_at.isoformat() if e.processed_at else None,
            error=e.error,
            received_at=e.received_at.isoformat(),
        )
        for e in result.scalars().all()
    ]
