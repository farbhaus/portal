"""Thin wrapper around py_webauthn for passkey registration + authentication.

RP id/name/origin derive from `base_url` (overridable via config). Challenges are produced here as
base64url strings so callers can stash them in the signed session and hand them back on verify.
Pairs with @simplewebauthn/browser on the client.
"""

from __future__ import annotations

from typing import Any
from urllib.parse import urlsplit

from webauthn import (
    generate_authentication_options,
    generate_registration_options,
    options_to_json,
    verify_authentication_response,
    verify_registration_response,
)
from webauthn.helpers import base64url_to_bytes, bytes_to_base64url
from webauthn.helpers.structs import (
    AuthenticatorSelectionCriteria,
    PublicKeyCredentialDescriptor,
    ResidentKeyRequirement,
    UserVerificationRequirement,
)

from portal.lib.config import get_settings


def _rp_id() -> str:
    s = get_settings()
    return s.webauthn_rp_id or (urlsplit(s.base_url).hostname or "localhost")


def _origin() -> str:
    return get_settings().base_url.rstrip("/")


def _rp_name() -> str:
    return get_settings().webauthn_rp_name or "Portal"


def registration_options(
    *, user_id: bytes, user_name: str, exclude_ids: list[str]
) -> tuple[str, str]:
    """Return (options JSON for the browser, challenge base64url to stash)."""
    opts = generate_registration_options(
        rp_id=_rp_id(),
        rp_name=_rp_name(),
        user_id=user_id,
        user_name=user_name,
        user_display_name=user_name,
        authenticator_selection=AuthenticatorSelectionCriteria(
            resident_key=ResidentKeyRequirement.PREFERRED,
            user_verification=UserVerificationRequirement.PREFERRED,
        ),
        exclude_credentials=[
            PublicKeyCredentialDescriptor(id=base64url_to_bytes(cid)) for cid in exclude_ids
        ],
    )
    return options_to_json(opts), bytes_to_base64url(opts.challenge)


def verify_registration(*, credential: str, challenge_b64: str) -> Any:
    return verify_registration_response(
        credential=credential,
        expected_challenge=base64url_to_bytes(challenge_b64),
        expected_rp_id=_rp_id(),
        expected_origin=_origin(),
    )


def authentication_options() -> tuple[str, str]:
    """Discoverable (usernameless) login options. Return (options JSON, challenge base64url)."""
    opts = generate_authentication_options(
        rp_id=_rp_id(),
        user_verification=UserVerificationRequirement.PREFERRED,
    )
    return options_to_json(opts), bytes_to_base64url(opts.challenge)


def verify_authentication(
    *, credential: str, challenge_b64: str, public_key_b64: str, sign_count: int
) -> Any:
    return verify_authentication_response(
        credential=credential,
        expected_challenge=base64url_to_bytes(challenge_b64),
        expected_rp_id=_rp_id(),
        expected_origin=_origin(),
        credential_public_key=base64url_to_bytes(public_key_b64),
        credential_current_sign_count=sign_count,
        require_user_verification=False,
    )


def to_b64url(data: bytes) -> str:
    return bytes_to_base64url(data)
