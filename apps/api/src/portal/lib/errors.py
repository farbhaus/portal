from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from portal.lib.logging import get_logger

log = get_logger("errors")


class PortalError(Exception):
    """Base class for Portal application errors."""

    status_code = 500
    public_message = "Internal server error"

    def __init__(self, message: str | None = None) -> None:
        super().__init__(message or self.public_message)
        self.public_message = message or self.public_message


class AuthError(PortalError):
    status_code = 401
    public_message = "Authentication required"


class NotFoundError(PortalError):
    status_code = 404
    public_message = "Not found"


def install_exception_handlers(app: FastAPI) -> None:
    @app.exception_handler(PortalError)
    async def _portal_error(_: Request, exc: PortalError) -> JSONResponse:
        return JSONResponse(status_code=exc.status_code, content={"detail": exc.public_message})

    @app.exception_handler(Exception)
    async def _unhandled(request: Request, exc: Exception) -> JSONResponse:
        # Never leak internals to clients; full detail goes to structured logs.
        log.exception("unhandled.error", path=request.url.path)
        return JSONResponse(status_code=500, content={"detail": "Internal server error"})
