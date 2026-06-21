"""Reusable Pydantic field types for request models."""

from __future__ import annotations

import re
from typing import Annotated

from pydantic import AfterValidator

_HEX_COLOR = re.compile(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$")


def _validate_hex_color(value: str | None) -> str | None:
    """Accept ``#rgb``/``#rrggbb`` (or empty → None), else raise.

    Branding accents are interpolated into inline ``style`` on the public pages, so a non-colour
    value would be CSS injection.
    """
    if value is None:
        return None
    cleaned = value.strip()
    if not cleaned:
        return None
    if not _HEX_COLOR.match(cleaned):
        raise ValueError("must be a hex colour like #f59e0b")
    return cleaned


# An optional brand accent colour, validated as a hex value.
HexColor = Annotated[str | None, AfterValidator(_validate_hex_color)]
