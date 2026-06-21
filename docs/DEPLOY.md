# Deploying Portal

Portal ships as a single **all-in-one container** (`farbhaus/portal`) bundling the api, sync
worker, web server, Postgres and Redis. It runs behind a **host reverse proxy** that owns 80/443
and terminates TLS; Portal itself binds only `127.0.0.1:${PORTAL_PORT}` (default 18080), and the
host proxy forwards your domain to that loopback port. See the topology diagram in the
[README](../README.md#deployment-topology) and [host-caddy.example](../host-caddy.example).

This runbook assumes:
- DNS for your domain points at the host.
- A host reverse proxy (Caddy, nginx, Traefik, …) is installed and can terminate TLS.
- Docker 24+ and Compose v2 are installed.

Throughout, replace `portal.example.com` with your domain.

## 1. Get the deploy files

```bash
git clone https://github.com/farbhaus/portal.git
cd portal
```

(You only need `docker-compose.yml` + `.env.example` from the repo root — the compose file
references the published image, so no build is required.)

## 2. Create the `.env`

```bash
cp .env.example .env
```

Edit `.env`. The all-in-one image bundles the databases and generates its own crypto secrets on
first boot, so the only values you must set are:

```ini
BASE_URL=https://portal.example.com     # your public HTTPS URL
ADMIN_EMAIL=you@example.com
ADMIN_PASSWORD=<a strong, unique password>

PORTAL_PORT=18080                        # loopback port the host proxy forwards to
PORTAL_VERSION=latest                    # or pin a release, e.g. v1.0.0
SESSION_COOKIE_SECURE=true               # true in production (HTTPS via the host proxy)
```

Notes:
- **Frame.io and SMTP credentials are NOT set here** — you enter them in Portal's **Settings**
  page after first login (they're stored encrypted in the database). See
  [ADOBE_DEVELOPER_CONSOLE_SETUP.md](ADOBE_DEVELOPER_CONSOLE_SETUP.md) for obtaining the
  Frame.io OAuth credentials.
- `SESSION_SECRET` / `TOKEN_ENCRYPTION_KEY` are generated and persisted in the data volume on
  first boot; leave them blank unless you want to manage them yourself. They stay stable across
  restarts (they live in the same volume as the data they protect).
- If you use **sync rules**, set `SYNC_HOST_ROOT` to the host directory where synced files should
  land (default `/mnt`); otherwise you can remove that bind mount from `docker-compose.yml`.

## 3. Add the host reverse-proxy entry

Point your host proxy at Portal's loopback port. For Caddy, add to the **host** Caddyfile
(typically `/etc/caddy/Caddyfile`):

```caddy
portal.example.com {
	reverse_proxy 127.0.0.1:18080
}
```

Then reload it: `sudo systemctl reload caddy`. Caddy obtains/renews TLS automatically once DNS
resolves. nginx/Traefik equivalents just proxy `portal.example.com` → `127.0.0.1:18080`. If you
don't yet run a proxy, [host-caddy.example](../host-caddy.example) is a minimal starting
point.

## 4. Start Portal

```bash
docker compose up -d        # or: make deploy
```

This pulls `farbhaus/portal`, then on first boot the container initializes its Postgres cluster,
generates secrets, runs DB migrations, seeds the admin, and starts all services behind its
internal router on `127.0.0.1:18080`.

Check health:

```bash
docker compose ps                                  # portal: healthy
curl -s https://portal.example.com/api/health      # {"status":"ok","db":true,"redis":true,...}
```

## 5. First login + connect Frame.io

1. Open `https://portal.example.com` → log in with `ADMIN_EMAIL` / `ADMIN_PASSWORD`.
2. Go to **Settings → connect Frame.io** → approve at Adobe → you're returned with the account
   shown as connected. (Requires the Adobe redirect URI to match — see
   [ADOBE_DEVELOPER_CONSOLE_SETUP.md](ADOBE_DEVELOPER_CONSOLE_SETUP.md).)
3. While in Settings you can also set SMTP (for email notifications) and upload your brand logo.

If the connect fails, check `docker compose logs portal` (Portal never shows provider internals in
the UI). The most common cause is a redirect-URI mismatch.

## Updating

```bash
cd portal
docker compose pull          # fetch the newer image (or bump PORTAL_VERSION in .env)
docker compose up -d
```

Or just `make update`. Migrations run automatically on startup; your data persists in the
`portal_data` volume.

## Useful commands

```bash
docker compose ps                  # status
docker compose logs -f portal      # logs (JSON from the api; supervisord-managed services)
docker compose down                # stop (keeps the portal_data volume / your data)
docker compose down -v             # stop and DELETE the data volume (destructive)
```

## Backups

Everything lives in the `portal_data` Docker volume (Postgres cluster, Redis AOF, secrets). Back
that volume up, or dump the database from inside the container:

```bash
docker compose exec portal pg_dump -U portal portal > portal-backup.sql
```
