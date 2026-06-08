"""Adobe IMS OAuth client for Frame.io (Web App / authorization-code flow).

Endpoints and behavior are documented in docs/FRAMEIO_OAUTH.md. Authorization is role-based
on the Frame.io side, so the scopes here only cover identity + offline_access (refresh token).
"""

from dataclasses import dataclass
from datetime import UTC, datetime, timedelta

import httpx
from authlib.integrations.httpx_client import AsyncOAuth2Client

from portal.lib.config import get_settings
from portal.lib.logging import get_logger

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


def _client() -> AsyncOAuth2Client:
    settings = get_settings()
    return AsyncOAuth2Client(
        client_id=settings.frameio_client_id,
        client_secret=settings.frameio_client_secret,
        scope=settings.frameio_scopes,
        redirect_uri=settings.resolved_frameio_redirect_uri,
        token_endpoint_auth_method="client_secret_basic",
    )


def _to_token_set(token: dict[str, object]) -> TokenSet:
    raw_expires = token.get("expires_in", 0)
    expires_in = int(raw_expires) if isinstance(raw_expires, (int, float, str)) else 0
    expires_at = datetime.now(UTC) + timedelta(seconds=expires_in)
    return TokenSet(
        access_token=str(token["access_token"]),
        refresh_token=str(token["refresh_token"]) if token.get("refresh_token") else None,
        expires_at=expires_at,
        scopes=str(token.get("scope") or get_settings().frameio_scopes),
    )


def build_authorize_url(state: str) -> str:
    """Authorization URL the admin's browser is redirected to."""
    client = _client()
    url, _ = client.create_authorization_url(IMS_AUTHORIZE_URL, state=state)
    return str(url)


async def exchange_code(code: str) -> TokenSet:
    async with _client() as client:
        try:
            token = await client.fetch_token(IMS_TOKEN_URL, code=code)
        except Exception as exc:  # authlib raises various OAuthError subclasses
            log.warning("frameio.oauth.exchange_failed", error=str(exc))
            raise FrameioOAuthError("Token exchange failed") from exc
    return _to_token_set(dict(token))


async def refresh_tokens(refresh_token: str) -> TokenSet:
    async with _client() as client:
        try:
            token = await client.refresh_token(IMS_TOKEN_URL, refresh_token=refresh_token)
        except Exception as exc:
            log.warning("frameio.oauth.refresh_failed", error=str(exc))
            raise FrameioOAuthError("Token refresh failed") from exc
    return _to_token_set(dict(token))


async def fetch_identity(access_token: str) -> Identity:
    """Resolve the connected user via GET /v4/me (verifies the token too)."""
    async with httpx.AsyncClient(timeout=15) as client:
        resp = await client.get(
            f"{FRAMEIO_API_BASE}/me",
            headers={"Authorization": f"Bearer {access_token}"},
        )
    if resp.status_code != 200:
        log.warning("frameio.oauth.me_failed", status=resp.status_code)
        raise FrameioOAuthError("Could not fetch Frame.io identity")
    body = resp.json()
    data = body.get("data", body) if isinstance(body, dict) else {}
    return Identity(
        frameio_user_id=str(data["id"]) if data.get("id") is not None else None,
        email=data.get("email"),
    )
