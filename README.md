# Portal

**Self-hosted file transfer for film & video post-production, backed by [Frame.io](https://frame.io).**

Portal gives you branded **upload** and **download** links that let clients and external
collaborators move files in and out of specific Frame.io destinations — no Frame.io account
required on their end — plus **sync rules** that automatically pull new files down to local or
network storage. It's the open-source, self-hosted successor to Silosync's SILOLink/SILOWatch,
built so you own the front door to your Frame.io account.

A core design principle: **file bytes never flow through the Portal server.** Uploads go
browser-direct to Frame.io's storage via presigned URLs; downloads are browser redirects to
short-lived signed URLs. Portal handles auth, branding, links and orchestration — not gigabytes.

> Portal is an independent project and is **not affiliated with, endorsed by, or sponsored by
> Adobe or Frame.io.** "Frame.io" and "Adobe" are trademarks of their respective owners.

## Features

- **Upload links** (`/u/{token}`) — branded landing page; anyone can drag-drop files (folder
  structure preserved, resumable) into a configured Frame.io project/folder. Per-link expiry,
  password, size cap, allowed extensions, uploader-info fields, optional email verification, and
  an optional target subfolder + path template. Email notification on completion.
- **Download links** (`/d/{token}`) — branded page to hand a file, a selection, or a whole folder
  to an anonymous recipient, downloaded straight from Frame.io via signed URLs. Optional preview
  thumbnails, password, max-downloads, viewer fields.
- **Sync rules** — watch a Frame.io folder and auto-download new files to a local/NAS path, with
  path templating, conflict policy, parallel ranged downloads (flat memory on huge files),
  retries/dead-lettering, and a reconcile cron. Triggered immediately by Portal uploads.
- **In-Portal Frame.io explorer** — browse accounts/workspaces/projects/folders; multi-select
  download/copy/move/delete; create projects/folders; create native Frame.io share links.
- **Secure admin** — passkey (WebAuthn) passwordless login, optional TOTP 2FA, signed/httponly
  session cookies, rate-limited public endpoints.
- **Fully brandable** — upload your logo and accent color; shown on every public page and login.

## Deployment topology

Portal is one **all-in-one container** (api + sync worker + SvelteKit web + Postgres + Redis,
managed internally by supervisord) that listens only on a **loopback port**. A reverse proxy you
already run on the host terminates TLS and forwards your domain to that port:

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
      │   all state in the /data volume                 │
      └─────────────────────────────────────────────────┘
```

This keeps responsibilities clean — Portal does HTTP routing, your host does TLS — and lets Portal
coexist with other services on the same box without claiming ports 80/443. If you don't already
run a reverse proxy, [host-caddy.example](host-caddy.example) is a two-line starting point.

## Quickstart

You need Docker + Compose v2, a domain pointed at your host, and a host reverse proxy. Then:

```bash
git clone https://github.com/farbhaus/portal.git
cd portal
make deploy                 # creates .env on first run — edit it, then run again
```

`make deploy` wraps `docker compose up -d` (and seeds `.env` from `.env.example` on first run);
`make update` pulls a newer image, `make logs`/`make status`/`make backup` do the obvious things.
Run `make` to list them. Point your host proxy at `127.0.0.1:18080` (see [host-caddy.example](host-caddy.example)),
then log in at your domain and connect Frame.io from **Settings**. The full walkthrough —
including the [Adobe Developer Console setup](docs/ADOBE_DEVELOPER_CONSOLE_SETUP.md) needed for
Frame.io OAuth — is in **[docs/DEPLOY.md](docs/DEPLOY.md)**.

## Documentation

- [docs/DEPLOY.md](docs/DEPLOY.md) — full deployment runbook
- [docs/ADOBE_DEVELOPER_CONSOLE_SETUP.md](docs/ADOBE_DEVELOPER_CONSOLE_SETUP.md) — Frame.io OAuth credentials
- [docs/FRAMEIO_OAUTH.md](docs/FRAMEIO_OAUTH.md) / [docs/FRAMEIO_CLIENT.md](docs/FRAMEIO_CLIENT.md) — how Portal talks to Frame.io
- [CONTRIBUTING.md](CONTRIBUTING.md) — local dev setup and how to contribute
- [SECURITY.md](SECURITY.md) — reporting a vulnerability

## License

Portal is licensed under the **GNU Affero General Public License v3.0** ([LICENSE](LICENSE)).
The AGPL's network clause means anyone who runs a modified Portal as a service must make their
source available — self-hosting and modification are fully permitted, closed-source SaaS forks
are not.

Copyright © 2026 FARBHAUS.
