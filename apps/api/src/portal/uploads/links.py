"""Upload-link helpers: token generation, extension normalization, state, branding merge.

Kept separate from the routes so the link/session lifecycle logic (and its tests) don't depend
on FastAPI. The uploader page and the Frame.io upload mechanism land in build step 6; this
module is the link-resolution core they build on.
"""

import secrets
from datetime import UTC, datetime
from typing import Literal

from portal.db.models import Destination, UploadLink

# Media-focused default extensions offered as a preset when an admin creates a link
# (post-production camera/audio/image formats). An empty/None allowed_extensions means
# "any extension"; this preset is only a convenience.
MEDIA_EXTENSIONS_PRESET: list[str] = [
    "r3d", "braw", "ari", "arx", "mxf", "mov", "mp4", "mts", "m4v", "avi",
    "prores", "dng", "cdng", "exr", "dpx", "tiff", "tif", "png", "jpg", "jpeg",
    "wav", "aif", "aiff", "mp3", "bwf", "zip",
]

LinkState = Literal["ok", "expired", "revoked"]

TOKEN_BYTES = 24  # ~32 url-safe chars; fits the 64-char column with room to spare.


def generate_token() -> str:
    return secrets.token_urlsafe(TOKEN_BYTES)


def normalize_extensions(extensions: list[str] | None) -> list[str] | None:
    """Lowercase, strip leading dots, dedupe (preserving order); None/empty -> None (any)."""
    if not extensions:
        return None
    seen: dict[str, None] = {}
    for ext in extensions:
        cleaned = ext.strip().lstrip(".").lower()
        if cleaned:
            seen.setdefault(cleaned, None)
    return list(seen) or None


def link_state(link: UploadLink, now: datetime | None = None) -> LinkState:
    now = now or datetime.now(UTC)
    if link.revoked_at is not None:
        return "revoked"
    if link.expires_at is not None:
        expires_at = link.expires_at
        if expires_at.tzinfo is None:
            expires_at = expires_at.replace(tzinfo=UTC)
        if expires_at <= now:
            return "expired"
    return "ok"


def merged_branding(link: UploadLink, destination: Destination) -> dict[str, str | None]:
    """Per-link branding overrides with destination branding as the fallback."""
    return {
        "display_name": link.brand_display_name or destination.display_name,
        "subtitle": link.brand_subtitle or destination.subtitle,
        "logo_url": link.brand_logo_url or destination.logo_url,
        "accent_color": link.brand_accent_color or destination.accent_color,
    }
