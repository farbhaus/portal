"""Shared start-session gate for links with verify_email on — used by both public flows."""

from __future__ import annotations

from fastapi import HTTPException, Request, Response
from sqlalchemy.ext.asyncio import AsyncSession

from portal.verify.service import verify_code
from portal.verify.trust import is_trusted, issue_trust


async def enforce_verification(
    db: AsyncSession, request: Request, response: Response, email: str, code: str | None
) -> None:
    """Allow the session iff the device is already trusted for this email or a valid code is given.
    On success, (re)issue the device trust cookie. Raises HTTPException otherwise."""
    if not email:
        raise HTTPException(status_code=422, detail="Email is required")
    if not is_trusted(request, email):
        if not code:
            raise HTTPException(status_code=401, detail="Verification required")
        if not await verify_code(db, email, code):
            raise HTTPException(status_code=403, detail="That code is incorrect or expired")
    issue_trust(response, email)
