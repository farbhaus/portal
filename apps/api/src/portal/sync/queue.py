"""ARQ job enqueueing from the API process.

The API persists a webhook event then enqueues its processing here so the HTTP response stays
fast and the work runs in the ARQ worker. The pool is created lazily on first enqueue and closed
in the app lifespan.
"""

from __future__ import annotations

import uuid

from arq import create_pool
from arq.connections import ArqRedis, RedisSettings

from portal.lib.config import get_settings

_pool: ArqRedis | None = None

# Worker task name (matches the function registered in portal.sync.worker.WorkerSettings).
PROCESS_WEBHOOK_EVENT = "process_webhook_event"


async def get_arq_pool() -> ArqRedis:
    global _pool
    if _pool is None:
        _pool = await create_pool(RedisSettings.from_dsn(get_settings().redis_url))
    return _pool


async def enqueue_webhook_processing(event_id: uuid.UUID) -> None:
    pool = await get_arq_pool()
    await pool.enqueue_job(PROCESS_WEBHOOK_EVENT, str(event_id))


async def close_arq_pool() -> None:
    global _pool
    if _pool is not None:
        await _pool.aclose()
        _pool = None
