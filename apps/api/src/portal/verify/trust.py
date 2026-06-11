"""Device-bound trust: a verified email is remembered in an encrypted cookie on that browser.

The cookie is the source of truth for "skip the code" — it can't be forged (AES-GCM with the
server key) and it's per-device, so typing a known client's address on another machine still needs a
code. The window slides: re-issue on each real upload/download so frequent clients stay trusted and
infrequent ones eventually re-verify.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta

from fastapi import Request, Response

from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher

COOKIE_NAME = "portal_verified"


def _cipher() -> TokenCipher:
    return TokenCipher(get_settings().token_encryption_key)


def is_trusted(request: Request, email: str) -> bool:
    raw = request.cookies.get(COOKIE_NAME)
    if not raw:
        return False
    try:
        data = json.loads(_cipher().decrypt(raw))
    except Exception:
        return False
    if not isinstance(data, dict):
        return False
    if str(data.get("email", "")).lower() != email.strip().lower():
        return False
    exp = data.get("exp")
    return isinstance(exp, int | float) and exp > datetime.now(UTC).timestamp()


def issue_trust(response: Response, email: str) -> None:
    settings = get_settings()
    days = settings.verify_trust_days
    exp = (datetime.now(UTC) + timedelta(days=days)).timestamp()
    token = _cipher().encrypt(json.dumps({"email": email.strip().lower(), "exp": exp}))
    response.set_cookie(
        COOKIE_NAME,
        token,
        max_age=days * 86400,
        httponly=True,
        secure=settings.session_cookie_secure,
        samesite="lax",
        path="/",
    )
