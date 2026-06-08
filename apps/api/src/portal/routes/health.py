from typing import Any

import redis.asyncio as aioredis
from fastapi import APIRouter, Depends
from fastapi.responses import JSONResponse
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession

from portal import __version__
from portal.db.session import get_session
from portal.lib.config import get_settings

router = APIRouter(tags=["health"])


async def _check_db(db: AsyncSession) -> bool:
    try:
        await db.execute(text("SELECT 1"))
        return True
    except Exception:
        return False


async def _check_redis() -> bool:
    client = aioredis.from_url(get_settings().redis_url)  # type: ignore[no-untyped-call]
    try:
        return bool(await client.ping())
    except Exception:
        return False
    finally:
        await client.aclose()


@router.get("/health")
async def health(db: AsyncSession = Depends(get_session)) -> JSONResponse:
    db_ok = await _check_db(db)
    redis_ok = await _check_redis()
    healthy = db_ok and redis_ok
    payload: dict[str, Any] = {
        "status": "ok" if healthy else "degraded",
        "db": db_ok,
        "redis": redis_ok,
        "version": __version__,
    }
    return JSONResponse(status_code=200 if healthy else 503, content=payload)
