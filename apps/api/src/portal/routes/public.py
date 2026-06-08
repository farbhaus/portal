"""Public (unauthenticated) upload-link endpoints.

These back the branded uploader page at /u/{token}. They expose only what an anonymous
uploader needs — branding and upload constraints — and never Frame.io internals (account /
workspace / folder ids) or stack traces. The uploader page and the actual upload mechanism
land in build step 6; this is the token-resolution and password-gate core.
"""

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from portal.auth.passwords import verify_password
from portal.db.models import UploadLink
from portal.db.session import get_session
from portal.lib.errors import NotFoundError
from portal.uploads.links import link_state, merged_branding

router = APIRouter(prefix="/public/links", tags=["public"])


class PublicLinkOut(BaseModel):
    state: str  # "ok" | "expired" | "revoked"
    display_name: str
    subtitle: str | None
    logo_url: str | None
    accent_color: str | None
    password_required: bool
    # Only meaningful when state == "ok"; still returned so the page can render constraints.
    max_file_size: int | None
    allowed_extensions: list[str] | None
    uploader_fields_required: dict[str, bool]


class PasswordVerifyRequest(BaseModel):
    password: str


class PasswordVerifyResult(BaseModel):
    ok: bool


async def _get_link(db: AsyncSession, token: str) -> UploadLink:
    result = await db.execute(
        select(UploadLink)
        .where(UploadLink.token == token)
        .options(selectinload(UploadLink.destination))
    )
    link = result.scalar_one_or_none()
    if link is None:
        # Same response as any unknown token — don't reveal whether a token ever existed.
        raise NotFoundError("This upload link does not exist")
    return link


@router.get("/{token}", response_model=PublicLinkOut)
async def resolve_link(token: str, db: AsyncSession = Depends(get_session)) -> PublicLinkOut:
    link = await _get_link(db, token)
    branding = merged_branding(link, link.destination)
    fields = link.uploader_fields_required or {}
    return PublicLinkOut(
        state=link_state(link),
        display_name=branding["display_name"] or "Upload",
        subtitle=branding["subtitle"],
        logo_url=branding["logo_url"],
        accent_color=branding["accent_color"],
        password_required=link.password_hash is not None,
        max_file_size=link.max_file_size,
        allowed_extensions=link.allowed_extensions,
        uploader_fields_required={
            "name": bool(fields.get("name")),
            "email": bool(fields.get("email")),
            "message": bool(fields.get("message")),
        },
    )


@router.post("/{token}/verify-password", response_model=PasswordVerifyResult)
async def verify_link_password(
    token: str, body: PasswordVerifyRequest, db: AsyncSession = Depends(get_session)
) -> PasswordVerifyResult:
    link = await _get_link(db, token)
    # An open (no-password) link verifies trivially; a usable link must also be in "ok" state.
    if link.password_hash is None:
        return PasswordVerifyResult(ok=link_state(link) == "ok")
    ok = link_state(link) == "ok" and verify_password(body.password, link.password_hash)
    return PasswordVerifyResult(ok=ok)
