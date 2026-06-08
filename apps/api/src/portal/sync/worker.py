"""ARQ worker scaffold.

No sync tasks exist yet (Step 9). This boots a healthy worker connected to Redis so the
container topology is in place. Tasks register in `functions` as they are built.
"""

from typing import Any

from arq.connections import RedisSettings

from portal.lib.config import get_settings
from portal.lib.logging import configure_logging, get_logger

log = get_logger("worker")


async def noop(ctx: dict[str, Any]) -> str:
    """Placeholder task so the worker has a registered function. Removed in Step 9."""
    return "ok"


async def startup(ctx: dict[str, Any]) -> None:
    configure_logging(get_settings().log_level)
    log.info("worker.startup")


async def shutdown(ctx: dict[str, Any]) -> None:
    log.info("worker.shutdown")


class WorkerSettings:
    functions: list[Any] = [noop]
    on_startup = startup
    on_shutdown = shutdown
    redis_settings = RedisSettings.from_dsn(get_settings().redis_url)
