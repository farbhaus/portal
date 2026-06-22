"""Adobe IMS OAuth client for Frame.io (Web App / authorization-code flow).

Endpoints and behavior are documented in CLAUDE.md (Frame.io integration → OAuth). Authorization
is role-based on the Frame.io side, so the scopes here only cover identity + offline_access
(refresh token).
"""

from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import TYPE_CHECKING

import httpx
from authlib.integrations.httpx_client import AsyncOAuth2Client

from portal.lib.logging import get_logger

if TYPE_CHECKING:
    from portal.services.runtime_config import OAuthConfig

log = get_logger("frameio.oauth")

IMS_AUTHORIZE_URL = "https://ims-na1.adobelogin.com/ims/authorize/v2"
IMS_TOKEN_URL = "https://ims-na1.adobelogin.com/ims/token/v3"
FRAMEIO_API_BASE = "https://api.frame.io/v4"


@dataclass
class TokenSet:
    access_token: str
    refresh_token: str | None
    expires_at: datetime
    scopes: str


@dataclass
class Identity:
    frameio_user_id: str | None
    email: str | None


class FrameioOAuthError(Exception):
    """Raised when an Adobe IMS token operation fails."""


def _client(config: "OAuthConfig") -> AsyncOAuth2Client:
    return AsyncOAuth2Client(
        client_id=config.client_id,
        client_secret=config.client_secret,
        scope=config.scopes,
        redirect_uri=config.redirect_uri,
        token_endpoint_auth_method="client_secret_basic",
    )


def _to_token_set(token: dict[str, object], default_scopes: str) -> TokenSet:
    raw_expires = token.get("expires_in", 0)
    expires_in = int(raw_expires) if isinstance(raw_expires, (int, float, str)) else 0
    expires_at = datetime.now(UTC) + timedelta(seconds=expires_in)
    return TokenSet(
        access_token=str(token["access_token"]),
        refresh_token=str(token["refresh_token"]) if token.get("refresh_token") else None,
        expires_at=expires_at,
        scopes=str(token.get("scope") or default_scopes),
    )


def build_authorize_url(config: "OAuthConfig", state: str) -> str:
    """Authorization URL the admin's browser is redirected to."""
    client = _client(config)
    url, _ = client.create_authorization_url(IMS_AUTHORIZE_URL, state=state)
    return str(url)


async def exchange_code(config: "OAuthConfig", code: str) -> TokenSet:
    async with _client(config) as client:
        try:
            token = await client.fetch_token(IMS_TOKEN_URL, code=code)
        except Exception as exc:  # authlib raises various OAuthError subclasses
            log.warning("frameio.oauth.exchange_failed", error=str(exc))
            raise FrameioOAuthError("Token exchange failed") from exc
    log.info(
        "frameio.oauth.exchange_ok",
        granted_scope=token.get("scope"),
        token_type=token.get("token_type"),
        has_refresh=bool(token.get("refresh_token")),
    )
    return _to_token_set(dict(token), config.scopes)


async def refresh_tokens(config: "OAuthConfig", refresh_token: str) -> TokenSet:
    async with _client(config) as client:
        try:
            token = await client.refresh_token(IMS_TOKEN_URL, refresh_token=refresh_token)
        except Exception as exc:
            log.warning("frameio.oauth.refresh_failed", error=str(exc))
            raise FrameioOAuthError("Token refresh failed") from exc
    return _to_token_set(dict(token), config.scopes)


async def fetch_identity(access_token: str) -> Identity:
    """Resolve the connected user via GET /v4/me (verifies the token too)."""
    async with httpx.AsyncClient(timeout=15) as client:
        resp = await client.get(
            f"{FRAMEIO_API_BASE}/me",
            headers={"Authorization": f"Bearer {access_token}"},
        )
    if resp.status_code != 200:
        log.warning("frameio.oauth.me_failed", status=resp.status_code, body=resp.text[:500])
        raise FrameioOAuthError("Could not fetch Frame.io identity")
    body = resp.json()
    data = body.get("data", body) if isinstance(body, dict) else {}
    return Identity(
        frameio_user_id=str(data["id"]) if data.get("id") is not None else None,
        email=data.get("email"),
    )
