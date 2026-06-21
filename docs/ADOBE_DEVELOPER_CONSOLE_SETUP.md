# Adobe Developer Console setup for Frame.io OAuth

Portal connects to Frame.io through Adobe's identity system (Adobe IMS). To allow it, you
create an **OAuth Web App** credential in the Adobe Developer Console and paste two values
(`client_id`, `client_secret`) into Portal's `.env`. This is a one-time setup. You need an
Adobe account that has access to your Frame.io account.

The OAuth flow itself is described in [../docs/FRAMEIO_OAUTH.md](../docs/FRAMEIO_OAUTH.md).

## 1. Create a project + Frame.io API

1. Go to https://developer.adobe.com/console and sign in with your Adobe ID.
2. **Create new project** (or open an existing one).
3. **Add API** → choose the **Frame.io** API.
4. When prompted for the credential type, choose **OAuth Web App credential** (a server-side
   app that keeps a client secret). Do **not** choose Single-Page App or Server-to-Server.

## 2. Set the scopes

Ensure the credential requests these scopes (Portal's default `FRAMEIO_SCOPES`):

```
openid, email, profile, offline_access, additional_info.roles
```

`offline_access` is essential — it's what lets Portal receive a **refresh token** and stay
connected without you re-authorizing every day.

## 3. Register the redirect URI(s)

The redirect URI is where Adobe sends the browser back after you approve. It **must exactly
match** `FRAMEIO_REDIRECT_URI` in Portal (which defaults to `${BASE_URL}/api/frameio/callback`).
Add every URL you'll use:

- **Production (HTTPS domain):**
  `https://portal.example.com/api/frameio/callback`
- **Local testing (loopback):**
  `http://127.0.0.1:18080/api/frameio/callback`
  (Adobe may require `http://localhost:18080/api/frameio/callback` instead, or reject plain
  HTTP/IP entirely. If loopback is refused, test against your HTTPS domain — set `BASE_URL`
  accordingly so the default redirect URI matches.)

In the console, the "Redirect URI pattern" field is a regex; the "Default redirect URI" is a
single URL. Add each callback URL you registered above.

## 4. Enter the credentials in Portal

From the credential's overview page, copy the **Client ID** and **Client Secret**. In Portal, log
in as admin and go to **Settings** → the Frame.io section, and paste them there. They're stored in
the database (the secret encrypted), not in `.env`, so no restart or redeploy is needed.

Make sure `BASE_URL` in your `.env` matches the host you'll actually open in the browser, so the
default redirect URI (`${BASE_URL}/api/frameio/oauth/callback`) lines up with what you registered.

## 5. Connect

In **Settings**, click **Connect Frame.io**. You'll be sent to Adobe to sign in and approve, then
returned to Portal showing the connected account.

### Troubleshooting

- **"redirect_uri mismatch"** — the URL you registered doesn't byte-for-byte match
  `${BASE_URL}/api/frameio/oauth/callback` (scheme, host, port, path, trailing slash all matter).
- **Connected but stops working after ~a day** — the refresh token is missing; confirm
  `offline_access` is in the granted scopes.
- **Returned to Settings with an error** — check `docker compose logs portal`; Portal never shows
  provider internals in the UI by design.
