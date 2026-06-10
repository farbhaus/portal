"""ARQ worker.

Runs the background jobs queued by the API. Step 10 registers ``process_webhook_event``, which
turns a persisted Frame.io webhook event into sync jobs. Step 11 adds the sync-job download tasks
and the periodic reconciliation cron.
"""

import uuid
from typing import Any

from arq.connections import RedisSettings

from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.lib.logging import configure_logging, get_logger
from portal.sync.events import process_event

log = get_logger("worker")


async def process_webhook_event(ctx: dict[str, Any], event_id: str) -> int:
    """Process a single ingested webhook event into sync jobs."""
    async with get_sessionmaker()() as db:
        return await process_event(db, uuid.UUID(event_id))


async def startup(ctx: dict[str, Any]) -> None:
    configure_logging(get_settings().log_level)
    log.info("worker.startup")


async def shutdown(ctx: dict[str, Any]) -> None:
    log.info("worker.shutdown")


class WorkerSettings:
    functions: list[Any] = [process_webhook_event]
    on_startup = startup
    on_shutdown = shutdown
    redis_settings = RedisSettings.from_dsn(get_settings().redis_url)
