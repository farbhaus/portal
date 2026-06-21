"""TOTP (RFC 6238) helpers + one-time recovery codes.

The secret is generated here and stored AES-GCM-encrypted on the user row; verification allows a
+/-1 step window for clock skew. Recovery codes are returned to the admin once and stored only as
sha256 hashes, consumed on use.
"""

from __future__ import annotations

import hashlib
import secrets
from io import BytesIO

import pyotp
import qrcode
import qrcode.image.svg

ISSUER = "Portal"
RECOVERY_CODE_COUNT = 10


def generate_secret() -> str:
    return pyotp.random_base32()


def provisioning_uri(secret: str, account: str, issuer: str = ISSUER) -> str:
    return pyotp.TOTP(secret).provisioning_uri(name=account, issuer_name=issuer)


def _normalize(code: str) -> str:
    return code.strip().replace(" ", "").replace("-", "").lower()


def verify_code(secret: str, code: str) -> bool:
    return pyotp.TOTP(secret).verify(_normalize(code), valid_window=1)


def qr_svg(uri: str) -> str:
    """Return an SVG QR for the otpauth URI (stdlib factory — no Pillow)."""
    img = qrcode.make(uri, image_factory=qrcode.image.svg.SvgPathImage)
    buf = BytesIO()
    img.save(buf)
    return buf.getvalue().decode("utf-8")


def _hash_code(code: str) -> str:
    return hashlib.sha256(_normalize(code).encode("utf-8")).hexdigest()


def generate_recovery_codes() -> tuple[list[str], list[str]]:
    """Return (plaintext codes to show once, sha256 hashes to store)."""
    codes = [f"{secrets.token_hex(3)}-{secrets.token_hex(3)}" for _ in range(RECOVERY_CODE_COUNT)]
    return codes, [_hash_code(c) for c in codes]


def consume_recovery_code(code: str, hashes: list[str]) -> list[str] | None:
    """If `code` matches a stored hash, return the remaining hashes; otherwise None."""
    target = _hash_code(code)
    if target not in hashes:
        return None
    return [h for h in hashes if h != target]
