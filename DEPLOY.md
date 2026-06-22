## Deployment topology

Portal is one **all-in-one container** (api + sync worker + SvelteKit web + Postgres + Redis,
managed internally by supervisord). It runs in either of two modes:

**Standalone** — the container's internal Caddy is the public TLS edge and provisions a Let's
Encrypt certificate itself. One container owns ports 80/443; nothing else to install:

```
            Internet (HTTPS)
                  │  :443  (Let's Encrypt, auto-provisioned)
      ┌───────────▼────────────────────────────────────┐
      │  farbhaus/portal  (one container)               │
      │   internal Caddy :443 → api :8000 / web :3000   │
      │   + sync worker, Postgres, Redis                │
      │   all state in the /data volume                 │
      └─────────────────────────────────────────────────┘
```

**Behind a host reverse proxy** — Portal listens only on a **loopback port** and a proxy you
already run terminates TLS and forwards your domain to it (so Portal can coexist with other
services without claiming 80/443):

```
            Internet (HTTPS)
                  │
      ┌───────────▼────────────┐
      │  Host reverse proxy    │   TLS termination, owns :443
      │  (Caddy / nginx / …)   │
      └───────────┬────────────┘
                  │  http → 127.0.0.1:${PORTAL_PORT}   (default 18080)
      ┌───────────▼────────────────────────────────────┐
      │  farbhaus/portal  (one container)               │
      │   internal Caddy :80 → api :8000 / web :3000    │
      │   + sync worker, Postgres, Redis                │
      └─────────────────────────────────────────────────┘
```

Pick standalone for a dedicated box (simplest, fully automated TLS); pick behind-a-proxy when the
host already runs one. [host-caddy.example](host-caddy.example) is a two-line starting point for the
latter.

## One-click deploy (recommended)

`deploy.sh` takes a clean host to a running deployment in one command. It takes your domain and the
admin email (required — it's your login). From a checkout:

```bash
git clone https://github.com/farbhaus/portal.git
cd portal
./deploy.sh portal.example.com you@example.com                 # standalone (container provisions TLS)
./deploy.sh --behind-proxy portal.example.com you@example.com  # loopback only; point your host proxy at it
```

Or zero-checkout, straight from the host (fetches just the deploy files, then runs `deploy.sh`):

```bash
curl -fsSL https://raw.githubusercontent.com/farbhaus/portal/main/install.sh | bash -s -- portal.example.com you@example.com
```

The script installs Docker + Compose if missing, writes `.env` (generating a strong
`ADMIN_PASSWORD`, printed once at the end — change it after first login), opens the firewall for
standalone, pulls the image, and waits for the container to report healthy. The admin email can also
be given with `--admin-email you@example.com`. Other useful flags: `--update` (pull newest image +
recreate; data survives), `--init-env` (write `.env` and stop), `-y` (skip prompts). Then jump to
[First login + connect Frame.io](#5-first-login--connect-frameio).

The rest of this runbook is the **manual** equivalent, if you'd rather drive compose yourself. It
assumes Docker 24+ / Compose v2 are installed and DNS for your domain points at the host.

## Standalone (Portal provisions its own TLS)

On a host with DNS pointed at it and ports 80/443 free and reachable:

```bash
cp .env.example .env
```

Edit `.env` — set your domain and admin login, and uncomment the four standalone TLS knobs:

```ini
BASE_URL=https://portal.example.com
ADMIN_EMAIL=you@example.com
ADMIN_PASSWORD=<a strong, unique password>

PORTAL_BEHIND_PROXY=false   # Portal is the public TLS edge (Caddy provisions Let's Encrypt)
WEB_BIND=0.0.0.0            # publish on all interfaces, not just loopback
PORTAL_PORT=80
PORTAL_HTTPS_PORT=443
```

Caddy's site address and the proxy-hop count are derived from `BASE_URL` + `PORTAL_BEHIND_PROXY`
inside the container — there's no separate domain or hop knob to keep in sync. Bring it up with the
ordinary command and check health:

```bash
docker compose up -d
docker compose ps                                  # portal: healthy
curl -s https://portal.example.com/api/health      # {"status":"ok","db":true,"redis":true,...}
```

Caddy obtains the certificate on first request and persists it (and the ACME account) in the
`portal_data` volume, so recreates don't re-issue. Skip to
[First login](#5-first-login--connect-frameio). The sections below cover the **behind-a-proxy**
path instead.

## Behind a host reverse proxy

This path assumes a host reverse proxy (Caddy, nginx, Traefik, …) is installed and can terminate
TLS.

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
  page after first login (they're stored encrypted in the database). See the **Adobe Developer
  Console setup** section in the [README](README.md#adobe-developer-console-setup-for-frameio-oauth)
  for obtaining the Frame.io OAuth credentials.
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
don't yet run a proxy, [host-caddy.example](host-caddy.example) is a minimal starting
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
   shown as connected. (Requires the Adobe redirect URI to match — see the **Adobe Developer Console
   setup** section in the [README](README.md#adobe-developer-console-setup-for-frameio-oauth).)
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
