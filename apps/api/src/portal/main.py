from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

from fastapi import FastAPI
from starlette.middleware.sessions import SessionMiddleware

from portal.frameio.client import close_frameio_client
from portal.lib.config import get_settings
from portal.lib.errors import install_exception_handlers
from portal.lib.logging import RequestIdMiddleware, configure_logging, get_logger
from portal.routes import auth, destinations, frameio, health, public, upload_links

log = get_logger("main")


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    # Admin seeding and migrations run once in the container entrypoint, not here, so they
    # don't race across uvicorn workers.
    settings = get_settings()
    configure_logging(settings.log_level)
    log.info("startup.complete", base_url=settings.base_url)
    yield
    await close_frameio_client()


def create_app() -> FastAPI:
    settings = get_settings()
    configure_logging(settings.log_level)

    app = FastAPI(
        title="Portal API",
        version="0.1.0",
        openapi_url="/api/openapi.json",
        docs_url="/api/docs",
        redoc_url=None,
        lifespan=lifespan,
    )

    app.add_middleware(
        SessionMiddleware,
        secret_key=settings.session_secret,
        session_cookie=settings.session_cookie_name,
        https_only=settings.session_cookie_secure,
        same_site="lax",
    )
    app.add_middleware(RequestIdMiddleware)

    install_exception_handlers(app)

    app.include_router(health.router, prefix="/api")
    app.include_router(auth.router, prefix="/api")
    app.include_router(frameio.router, prefix="/api")
    app.include_router(destinations.router, prefix="/api")
    app.include_router(upload_links.router, prefix="/api")
    app.include_router(public.router, prefix="/api")
    return app


app = create_app()
