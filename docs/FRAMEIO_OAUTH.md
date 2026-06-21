# Frame.io OAuth (Adobe IMS)

How Portal authenticates to Frame.io. Frame.io's V4 API authenticates users through **Adobe
Identity Management Services (IMS)** using OAuth 2.0. Portal uses the **Web App
(authorization-code) flow** — a server-side app that holds a client secret and stores the
resulting tokens encrypted at rest.

> Sourced from the live Frame.io docs (June 2026):
> - https://next.developer.frame.io/platform/v4/docs/guides/authentication/overview
> - https://next.developer.frame.io/platform/v4/docs/getting-started
> - https://next.developer.frame.io/platform/v4/docs/quick-start
> Re-verify against the live docs if Adobe/Frame.io change the endpoints.

## Endpoints

| Purpose            | Method | URL                                                  |
| ------------------ | ------ | ---------------------------------------------------- |
| Authorize          | GET    | `https://ims-na1.adobelogin.com/ims/authorize/v2`    |
| Token / Refresh    | POST   | `https://ims-na1.adobelogin.com/ims/token/v3`        |
| Frame.io API base  | —      | `https://api.frame.io/v4/`                           |
| Identity (verify)  | GET    | `https://api.frame.io/v4/me`                         |

Token-endpoint client authentication is **HTTP Basic**: `Authorization: Basic base64(client_id:client_secret)`.

## Scopes

```
openid,AdobeID,email,profile,offline_access,additional_info.roles
```

- `AdobeID` — **required for Frame.io API access.** Adobe auto-adds it during interactive
  consent, so the *first* access token works even without requesting it — but it is **not**
  added on token refresh. If `AdobeID` is not requested explicitly, every refreshed access
  token is rejected by the Frame.io API with `401 "Invalid or missing authorization token"`
  (~1h after connect, when the first token expires). Always request it.
- `offline_access` — **required** to receive a refresh token. Without it there is no refresh
  token and the admin would have to re-consent each time the access token expires (~1h).
- `openid email profile` — identity / profile.
- `additional_info.roles` — role information.

**Beyond `AdobeID`, authorization is role-based, not scope-based.** What the connected user can
*do* is governed by their Frame.io roles/permissions, not by per-operation scopes. The access
token (~1h lifetime) is refreshed transparently; the refresh token persists the connection.

## Flow

1. **Connect** — Portal builds the authorize URL and redirects the admin's browser:
   ```
   https://ims-na1.adobelogin.com/ims/authorize/v2
     ?client_id={CLIENT_ID}
     &redirect_uri={REDIRECT_URI}
     &scope=openid,AdobeID,email,profile,offline_access,additional_info.roles
     &response_type=code
     &state={RANDOM}
   ```
   `state` is a random value stored in the admin's session and verified on return (CSRF defense).

2. **Callback** — Adobe redirects to `{REDIRECT_URI}?code=...&state=...`. Portal verifies
   `state`, then exchanges the code:
   ```
   POST https://ims-na1.adobelogin.com/ims/token/v3
   Authorization: Basic base64(client_id:client_secret)
   Content-Type: application/x-www-form-urlencoded

   grant_type=authorization_code&code={CODE}&redirect_uri={REDIRECT_URI}
   ```
   Response: `{ "access_token", "refresh_token", "expires_in", "token_type": "bearer", ... }`.
   `expires_in` is ~86399s (~24h) in practice — **always trust the returned value**, don't hardcode.

3. **Identify** — Portal calls `GET https://api.frame.io/v4/me` with `Authorization: Bearer
   {access_token}` to capture the connected user's id and email, and stores them on the
   connection for display.

4. **Store** — access and refresh tokens are encrypted with AES-GCM
   (`portal.lib.crypto.TokenCipher`, keyed by `TOKEN_ENCRYPTION_KEY`) and saved on
   `frameio_connections`, along with `token_expires_at = now + expires_in`.

5. **Refresh** — before using the access token, Portal checks `token_expires_at` with a safety
   skew (5 min). If due, it refreshes:
   ```
   POST https://ims-na1.adobelogin.com/ims/token/v3
   Authorization: Basic base64(client_id:client_secret)

   grant_type=refresh_token&refresh_token={REFRESH_TOKEN}
   ```
   The new `access_token` (and `refresh_token`, if a new one is returned) and `expires_in` are
   re-encrypted and persisted. If the refresh token itself has expired, the admin must reconnect.

6. **Disconnect** — Portal deletes the stored connection (tokens). Adobe does not document a
   revoke endpoint we rely on here; deleting locally stops all use, and the tokens expire on
   Adobe's side per their lifetimes.

## Redirect URI

Configured via `FRAMEIO_REDIRECT_URI`, defaulting to `{BASE_URL}/api/frameio/oauth/callback`. The
**exact** value must be registered in the Adobe Developer Console integration (see
[ADOBE_DEVELOPER_CONSOLE_SETUP.md](ADOBE_DEVELOPER_CONSOLE_SETUP.md)). Portal supports
both a loopback URL for local testing and an HTTPS domain for real use; register each one you
intend to use. Adobe may reject plain-IP or non-HTTPS redirect URIs — if loopback is refused,
test against the HTTPS domain.

## Configuration (`.env`)

| Variable                | Meaning                                                            |
| ----------------------- | ----------------------------------------------------------------- |
| `FRAMEIO_CLIENT_ID`     | Adobe Developer Console Web App client id                          |
| `FRAMEIO_CLIENT_SECRET` | Web App client secret                                              |
| `FRAMEIO_SCOPES`        | Default `openid,AdobeID,email,profile,offline_access,additional_info.roles` (AdobeID required) |
| `FRAMEIO_REDIRECT_URI`  | Defaults to `{BASE_URL}/api/frameio/oauth/callback`                      |

## Errors & rate limits (for later steps)

Frame.io API errors come back as an `errors` array of `{detail, source, title}`. Rate-limit
headers are `x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-window` (ms); on HTTP
429, back off ≥1s and double on repeat. The client wrapper in Step 3 implements this; Step 2
only performs the OAuth token calls and a single `/v4/me`.
