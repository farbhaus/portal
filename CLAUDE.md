# Portal — project guide for Claude

Self-hosted file transfer for film/video post-production, backed by **Frame.io**. Portal gives
clients branded **upload** (`/u/{token}`) and **download** (`/d/{token}`) links into specific
Frame.io folders (no Frame.io account needed on their end). An upload link can be marked **Sync
to FioTransfer**; the external FioTransfer client then syncs it on the user's own machine — Portal
copies nothing to local/NAS storage itself (its sync rules were removed in migration
`f2a3b4c5d6e7`). Open-source (AGPL-3.0), successor to Silosync's SILOLink/SILOWatch.

**Core principle: file bytes never flow through the Portal server.** Uploads go browser-direct to
Frame.io storage via presigned URLs; downloads are browser redirects to short-lived signed URLs.
Portal handles auth, branding, links, and orchestration — never gigabytes.

## Repo layout

Monorepo, shipped as **one all-in-one container** (`farbhaus/portal`):

- `apps/api-rs/` — **the Portal service** (Rust 1.98, axum + sqlx/Postgres + redis). One binary,
  `portal-api`: the HTTP API plus the static web app on one origin; `portal-api worker` is the
  housekeeping loop (abandoned uploads, expiry reminders, retention; `worker.rs`),
  `portal-api migrate` / `seed` run from the entrypoint.
  Source in `apps/api-rs/src/`:
  - `routes/` HTTP endpoints · `auth.rs` + `auth/` admin login (passkeys/WebAuthn, TOTP; the
    `Admin` extractor also takes the optional `PORTAL_API_TOKEN` bearer, while `SessionAdmin` —
    browser login only — guards password, two-factor and passkey changes) ·
    `session.rs` Starlette-compatible signed session cookies · `frameio/` Frame.io client +
    OAuth/connection · `storage/` backend-agnostic storage interface · `downloads/` download-link
    sources · `verify/` email verification · `notify/` email · `services/` (incl. `upload_paths.rs`,
    the subfolder-template renderer) · `common/` client IP, lockout, rate limit · `crypto.rs` ·
    `config.rs` (env vars) · `spa.rs` SPA shell, admin page gate, branded `/u` `/d` shells ·
    `migrate.rs` + `schema.sql` + `migrations/` the schema (`baseline.sql`: the oldest schema it
    upgrades, for the check) · `seed.rs`.
  The service was ported from a Python (FastAPI) implementation, since removed; comments still
  say "port of `portal.…`", and some formats stay Python-compatible on purpose (session cookies,
  `TokenCipher`, timestamps in `pyfmt.rs`) so existing data and sessions keep working.
- `mcp/` — `portal-mcp`, a Rust stdio MCP server (rmcp) over the admin API, authenticating with
  `PORTAL_API_TOKEN`. Its own crate and `Cargo.lock`, not in the image; CI runs clippy + tests on
  it. Path ids must pass its UUID check (`path_id`) so a `../` can't reach another endpoint.
- `apps/web/` — SvelteKit frontend, built with `adapter-static` (`ssr = false`) into static files
  the Rust service serves; every load is a same-origin `fetch("/api/…")`. `src/routes/` includes
  `d/` (download), `u/` (upload), `login/`, `(admin)/`; `src/lib/` shared + `src/lib/components`.
- Container plumbing: `Dockerfile` (multi-stage: web build → Rust build → Caddy built from source
  with a patched Go, pinned by `CADDY_VERSION` / `CADDY_DEP_PINS` → Debian runtime),
  `docker/container/` (Caddyfile, supervisord.conf, entrypoint + start scripts),
  `docker-compose.yml` (+ `docker/docker-compose.dev.yml` overlay).

All container state lives in the **`portal_data` volume** (`/data`): the Postgres cluster, Redis
AOF, Caddy's certs/config (`/data/caddy`), and generated secrets (`/data/secrets`).

## Common commands (`make` — wraps compose)

- `make dev` — build the all-in-one image from source and run it (dev overlay).
- `make build` — build `farbhaus/portal:dev` locally.
- `make check` — static checks: `cargo clippy --locked --all-targets -- -D warnings` (apps/api-rs
  and mcp); `npm run check` (apps/web).
- `make test` — `cargo test --locked` (apps/api-rs and mcp) and `tools/schema_check.sh` (throwaway
  Postgres container).

**Schema changes:** hand-written SQL — see `apps/api-rs/migrations/README.md`: write
`migrations/<to>.sql` (one transaction that also moves `alembic_version`, a historical table name),
register it in `src/migrate.rs`, run `tools/schema_check.sh --write`. CI fails if `baseline.sql` +
migrations stop reproducing `schema.sql`.
- `make deploy` / `make update` / `make logs` / `make status` / `make backup` / `make shell`.

## Deployment (two modes, one knob)

The operator sets only **`BASE_URL`** (their public HTTPS URL) plus `ADMIN_EMAIL` / `ADMIN_PASSWORD`.
Everything mode-related is derived in `docker/container/portal-entrypoint.sh` from `BASE_URL` and the
optional **`PORTAL_BEHIND_PROXY`** toggle:

- **Standalone** (`PORTAL_BEHIND_PROXY=false`) — the container's internal Caddy is the public TLS
  edge and provisions its own **Let's Encrypt** cert for `BASE_URL`'s host. `.env` also sets
  `WEB_BIND=0.0.0.0`, `PORTAL_PORT=80`, `PORTAL_HTTPS_PORT=443`. Caddy site address = the host;
  `TRUSTED_PROXY_HOPS` derives to 1.
- **Behind a host proxy** (default, `PORTAL_BEHIND_PROXY` unset/true) — Portal binds
  `127.0.0.1:${PORTAL_PORT}` (default 18080), HTTP only; a proxy you already run terminates TLS and
  forwards your domain. Caddy site address = `:80`; `TRUSTED_PROXY_HOPS` derives to 2 (host proxy +
  internal Caddy). Power users can still set `PORTAL_SITE_ADDRESS` / `TRUSTED_PROXY_HOPS` explicitly.

A single `docker-compose.yml` serves both; its defaults reproduce the behind-proxy/loopback binding
exactly. **`./deploy.sh <domain>`** (standalone) or `./deploy.sh --behind-proxy <domain>` is the
one-click path; `install.sh` is the `curl … | bash` bootstrap. Crypto secrets (`SESSION_SECRET`,
`TOKEN_ENCRYPTION_KEY`) are generated in-container on first boot, not by the scripts. Full runbook in
`DEPLOY.md`; the load-balancing topology/diagrams are in `README.md`.

> ⚠️ The dev host this repo lives on is **LIVE production serving real clients** — never
> stop/restart its services or test standalone (binds 80/443) here; use a throwaway VM.

## Frame.io integration

**Layered storage abstraction** keeps the rest of the app off Frame.io specifics, so future S3-style
backends slot in without touching the upload/download engines:

```
routes / upload engine / download links
   └─ storage value objects       (apps/api-rs/src/storage/mod.rs)        backend-agnostic seam
        └─ FrameioStorageBackend  (src/storage/frameio_backend.rs)
             └─ FrameioClient     (src/frameio/client.rs)   the one Frame.io client module
                  └─ Frame.io V4 REST via reqwest (the endpoints the official SDK calls)
```

- Everything Frame.io goes through `FrameioClient`: auth-aware (transparent token refresh;
  `FrameioError::NotConnected`), retry/rate-limit-aware (honors `429`/`Retry-After`, retries 5xx),
  one process-wide HTTP client, folder-metadata cache with a 10-minute TTL. Errors are
  `FrameioError` — Frame.io internals never leak upward.
- Storage value objects (`DestinationConfig`, `RemoteFile`, `UploadSession`, `UploadCredentials`,
  `DownloadURL`, …) are the seam. `DestinationConfig` mirrors the type-tagged Frame.io folder JSONB
  (`{"type":"frameio","account_id","workspace_id","project_id","folder_id","path"}`) that an upload
  link stores as `links.folder`. Upload and download links share one `links` table (`kind` =
  `upload` | `download`; migration `e1f2a3b4c5d6` merged the old `upload_links`/`download_links`
  and dropped `destinations`).
- An upload link's `folder` may also carry `sync`: the "Sync to FioTransfer" switch (`{}` is
  enough). FioTransfer gives the link a folder on the user's disk; client uploads come down into
  `IN`, and `OUT` goes up to the return folder `sync.upload.folder_id`. Optional `name`,
  `download.folder_id` and `local` names override its defaults. Older links may store the earlier
  lanes (`drive` = download, `exports` = upload, `mode`) and a link-level `turbosort` (the older
  project folder name); they're kept as stored so FioTransfer's folders don't move. Validated by
  `validate_sync` in `routes/upload_links.rs`; the contract is in `docs/API.md`. Portal only
  stores and returns it for the external FioTransfer client — no Portal code moves files for it.
- Pickers (admin-only, under `/api/frameio`): list accounts/workspaces/projects/folders/files;
  cursor pagination via `links.next` → `after`, capped at `PICKER_MAX_ITEMS`.

### OAuth (Adobe IMS)

Frame.io V4 authenticates via **Adobe IMS**, OAuth 2.0 **authorization-code (Web App) flow**. The
Frame.io **client ID/secret are entered in Portal's Settings page and stored encrypted in the DB**
(not in `.env`); the *Adobe Developer Console setup* section in `README.md` covers obtaining them.
Endpoints/behavior live in `apps/api-rs/src/frameio/oauth.rs`.

- Authorize `https://ims-na1.adobelogin.com/ims/authorize/v2` · Token/refresh
  `https://ims-na1.adobelogin.com/ims/token/v3` (HTTP Basic `base64(client_id:client_secret)`) ·
  API base `https://api.frame.io/v4/` · identity `GET /v4/me`.
- Scopes: `openid,AdobeID,email,profile,offline_access,additional_info.roles`. **`AdobeID` is
  required and must be requested explicitly** — Adobe auto-adds it on interactive consent but NOT on
  refresh, so without it every *refreshed* token is rejected `401` ~1h after connect. `offline_access`
  is required for a refresh token. Beyond `AdobeID`, authorization is **role-based, not scope-based**.
- Tokens stored encrypted (AES-GCM, `TokenCipher` in `src/crypto.rs` — the original Python
  release's wire format, so existing secrets still decrypt — keyed by `TOKEN_ENCRYPTION_KEY`)
  on `frameio_connections` with `token_expires_at`; refreshed before use with a ~5-min skew. Always
  trust the returned `expires_in`; don't hardcode lifetimes.
- Redirect URI: `FRAMEIO_REDIRECT_URI`, default `{BASE_URL}/api/frameio/oauth/callback` — the exact
  value must be registered in the Adobe Developer Console.

## Going public / publishing (maintainer checklist)

The repo is currently **private** and the `farbhaus/portal` image is **not yet on Docker Hub**
(`.github/workflows/docker-image.yml` builds on every push, pushes only on a `v*` tag). When flipping
public:

1. **Re-enable gated CI** (disabled on the private free plan): restore the real triggers in
   `codeql.yml` and `image-scan.yml` (currently `workflow_dispatch` only); set
   `AUTOMERGE_ENABLED: 'true'` in `dependabot-automerge.yml`.
2. **GitHub settings**: enable "Allow auto-merge"; add branch protection on `main` requiring the CI
   checks; enable private vulnerability reporting (backs `SECURITY.md`).
3. **Docker Hub**: create the `farbhaus/portal` repo; add `DOCKERHUB_USERNAME` + `DOCKERHUB_TOKEN`
   action secrets; cut the first release `git tag v1.0.0 && git push origin v1.0.0` → publishes
   `:1.0.0` + `:latest`. The deploy scripts assume this published image (pull-only).
4. **Pre-flip scan**: confirm `.env` and real secrets are gitignored and absent from the tree;
   skim git history for anything sensitive; gitignore internal notes (`Briefing.md`, `To-Do.md`) if
   not publishing them.
