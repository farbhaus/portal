# Deploying Portal to a Linux VPS

Portal runs as a docker-compose stack behind a **host reverse proxy** that owns 80/443 and
terminates TLS. Portal's own Caddy binds only to `127.0.0.1:${PORTAL_PORT}` (default 18080);
the host proxy forwards your domain to that loopback port. See [ARCHITECTURE / topology in the
README] and [host-caddy.example](../deploy/host-caddy.example).

This runbook assumes (all already true for the reference deployment):
- DNS for your domain points at the VPS.
- A host reverse proxy (Caddy) is installed and can terminate TLS.
- Docker 24+ and Compose v2 are installed.
- The repo can be cloned on the VPS.

Throughout, replace `portal.zemariacolor.com` with your domain.

## 1. Clone

```bash
git clone https://github.com/farbhaus/portal.git
cd portal
```

## 2. Create the production `.env`

```bash
cp deploy/.env.example deploy/.env
```

Edit `deploy/.env`. The production-specific values:

```ini
PORTAL_PORT=18080
BASE_URL=https://portal.zemariacolor.com      # https, public domain

# Postgres — use a strong password, and make DATABASE_URL match it
POSTGRES_USER=portal
POSTGRES_PASSWORD=<run: openssl rand -base64 24>
POSTGRES_DB=portal
DATABASE_URL=postgresql+asyncpg://portal:<same password>@portal_postgres:5432/portal

REDIS_URL=redis://portal_redis:6379/0

# Secrets — generate fresh, never reuse dev values
SESSION_SECRET=<python -c "import secrets;print(secrets.token_urlsafe(32))">
TOKEN_ENCRYPTION_KEY=<python -c "import secrets;print(secrets.token_urlsafe(32))">

# Admin (single admin v1) — change the password
ADMIN_EMAIL=you@example.com
ADMIN_PASSWORD=<a strong password>

# HTTPS now, so cookies must be Secure
SESSION_COOKIE_SECURE=true

UVICORN_WORKERS=2
LOG_LEVEL=INFO

# Frame.io / Adobe IMS — your Web App credentials.
FRAMEIO_CLIENT_ID=<from Adobe Developer Console>
FRAMEIO_CLIENT_SECRET=<from Adobe Developer Console>
FRAMEIO_SCOPES=openid,email,profile,offline_access,additional_info.roles
# Leave blank: it auto-resolves to ${BASE_URL}/api/frameio/oauth/callback, which must match
# the redirect URI registered in Adobe (see deploy/ADOBE_DEVELOPER_CONSOLE_SETUP.md).
FRAMEIO_REDIRECT_URI=
```

Notes:
- With `BASE_URL=https://portal.zemariacolor.com`, the redirect URI auto-resolves to
  `https://portal.zemariacolor.com/api/frameio/oauth/callback` — register exactly that in Adobe.
- **`TOKEN_ENCRYPTION_KEY` is permanent.** It encrypts the stored Frame.io tokens; changing it
  later forces a reconnect. Set it once and keep it (back it up with your other secrets).
- `deploy/.env` is gitignored — it never leaves the server.

## 3. Add the host Caddy entry

Add to the **host** Caddyfile (typically `/etc/caddy/Caddyfile`):

```caddy
portal.zemariacolor.com {
	reverse_proxy 127.0.0.1:18080
}
```

Reload the host proxy:

```bash
sudo systemctl reload caddy   # or: caddy reload --config /etc/caddy/Caddyfile
```

Caddy will obtain/renew the TLS certificate automatically once DNS resolves.

## 4. Bring up the stack

```bash
cd deploy
docker compose up -d --build
```

This builds the api/worker/web images, starts Postgres + Redis, runs DB migrations and seeds the
admin (one-shot, in the api entrypoint), then starts the API, worker, web, and Portal's internal
Caddy on `127.0.0.1:18080`.

Check health:

```bash
docker compose ps                       # all services healthy
curl -s https://portal.zemariacolor.com/api/health   # {"status":"ok","db":true,"redis":true,...}
```

## 5. First login + connect Frame.io

1. Open `https://portal.zemariacolor.com` → log in with `ADMIN_EMAIL` / `ADMIN_PASSWORD`.
2. Go to **Connections** → **Connect Frame.io** → approve at Adobe → you're returned with the
   account shown as connected.

If the connect fails, check `docker compose logs portal_api` (Portal never shows provider
internals in the UI). The most common cause is a redirect-URI mismatch — see
[ADOBE_DEVELOPER_CONSOLE_SETUP.md](../deploy/ADOBE_DEVELOPER_CONSOLE_SETUP.md).

## Updating to a new version

```bash
cd portal
git pull
cd deploy
docker compose up -d --build
```

Migrations run automatically on api startup. To view logs: `docker compose logs -f portal_api`.

## Useful commands

```bash
docker compose ps                  # status
docker compose logs -f portal_api  # API logs (JSON, includes request_id)
docker compose down                # stop (keeps named volumes / data)
docker compose down -v             # stop and DELETE data volumes (destructive)
```
