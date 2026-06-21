"""Accessor for the single-row app_settings store (global branding)."""

from __future__ import annotations

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import AppSettings
from portal.notify.email import EmailBrand


async def get_app_settings(db: AsyncSession) -> AppSettings | None:
    return (await db.execute(select(AppSettings).limit(1))).scalar_one_or_none()


async def get_or_create_app_settings(db: AsyncSession) -> AppSettings:
    row = await get_app_settings(db)
    if row is None:
        row = AppSettings()
        db.add(row)
        await db.flush()
    return row


def logo_url(settings_row: AppSettings | None, base_url: str) -> str | None:
    """Public URL for the uploaded logo (cache-busted by its updated timestamp), or None."""
    if settings_row is None or not settings_row.logo_png:
        return None
    version = int(settings_row.logo_updated_at.timestamp()) if settings_row.logo_updated_at else 0
    return f"{base_url.rstrip('/')}/api/branding/logo?v={version}"


async def resolve_branding(
    db: AsyncSession, base_url: str
) -> tuple[str | None, str | None, str | None]:
    """App-wide branding for public pages: (logo_url, brand_display_name, brand_accent_color)."""
    row = await get_app_settings(db)
    return (
        logo_url(row, base_url),
        row.brand_display_name if row else None,
        row.brand_accent_color if row else None,
    )


async def email_brand(db: AsyncSession, base_url: str) -> EmailBrand:
    """Branding for outgoing emails, falling back to the EmailBrand defaults when unset."""
    row = await get_app_settings(db)
    defaults = EmailBrand()
    return EmailBrand(
        name=(row.brand_display_name if row and row.brand_display_name else defaults.name),
        accent_color=(
            row.brand_accent_color if row and row.brand_accent_color else defaults.accent_color
        ),
        logo_url=logo_url(row, base_url),
    )
