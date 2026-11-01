# Contributing to Portal

Thanks for your interest in Portal. This guide covers the repo layout, local development, and the
checks a change needs to pass.

## Repo layout

```
apps/api-rs/ the Portal service (Rust, axum + sqlx): API, the static web app, the sync worker
  src/  application        schema.sql + migrations/  the schema the image applies
apps/web/    SvelteKit 5 frontend (admin UI + public /u and /d pages), Tailwind v4, built to
             static files the Rust service serves
mcp/         portal-mcp, a stdio MCP server over the admin API (its own crate; not in the image)
tools/schema_check.sh  checks that baseline.sql + migrations/ reproduce schema.sql
docker/      dev compose + the all-in-one image's build assets (supervisord, entrypoint, …)
DEPLOY.md           deployment runbook (CLAUDE.md has the architecture + Frame.io notes)
Dockerfile          the all-in-one farbhaus/portal image (build context = repo root)
docker-compose.yml  production single-service deploy; .env.example alongside it
```

## Local development

`make dev` is the production deploy with a `--build` overlay — it builds the all-in-one image
from source and runs it from the same `docker-compose.yml` + `.env` (so create `.env` first, e.g.
`cp .env.example .env`):

```bash
make dev      # docker compose -f docker-compose.yml -f docker/docker-compose.dev.yml up -d --build
```

For day-to-day work on a single service you can also run the backend and frontend directly
(below), which is faster than rebuilding the image. Run `make` to see all targets.

### Portal service (apps/api-rs)

Requires Rust 1.98 (the Dockerfile's toolchain) with clippy, plus `pkg-config` and the OpenSSL
headers (`libssl-dev`).

```bash
cd apps/api-rs
cargo clippy --locked --all-targets -- -D warnings   # lint (warnings fail CI)
cargo test --locked                                  # unit tests
cargo test --locked -- --ignored                     # + the live-DNS check
```

To run it against a local Postgres + Redis: `DATABASE_URL=postgresql://… REDIS_URL=redis://…
SESSION_SECRET=… TOKEN_ENCRYPTION_KEY=… ADMIN_PASSWORD=… cargo run -- migrate`, then `cargo run --
seed` and `WEB_DIR=../web/build cargo run` (API + web app on `HOST:PORT`, default
`127.0.0.1:8000`; build the web app first) and `cargo run -- worker` (the sync worker).

### Schema changes

Migrations are hand-written SQL in `apps/api-rs/migrations/`: see
[apps/api-rs/migrations/README.md](apps/api-rs/migrations/README.md). In short — write
`<to>.sql` (one transaction that also moves `alembic_version`), register it in `src/migrate.rs`,
and run `tools/schema_check.sh --write` to regenerate `schema.sql`. CI fails if `baseline.sql`
plus the migrations stops reproducing `schema.sql`.

### Frontend (apps/web)

Requires Node 22.

```bash
cd apps/web
npm install
npm run check                 # svelte-check (must be 0 errors)
npm run build                 # production build
npm run dev                   # local dev server; proxies /api to PORTAL_API_URL
                              # (default http://127.0.0.1:18080, the `make dev` container)
```

## Before you open a PR

All of these must be green:

```bash
make check    # clippy (apps/api-rs, mcp) + svelte-check (apps/web)
make test     # Rust tests (apps/api-rs, mcp) + schema check (throwaway Postgres)
```

- Branch from and target **`dev`** (releases promote `dev` → `main`).
- Match the surrounding code's style and comment density; keep changes focused.
- Update docs when you change deploy/config behavior.

## License & sign-off

Portal is licensed under **AGPL-3.0** ([LICENSE](LICENSE)); contributions are accepted under the
same license. By submitting a contribution you certify you have the right to do so (Developer
Certificate of Origin). Please keep your commits signed off (`git commit -s`).
