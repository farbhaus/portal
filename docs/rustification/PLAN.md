# Portal → Rust: conversion plan, equivalence definition, and measurement protocol

Status: in progress (branch `rust-port`) — checkpoint 2: unauthenticated surface byte-identical
(10/10 matrix) + exact session-cookie interop. See `RESULTS.md`. Author: agent, at owner request
(goal: "convert this project to rust on a byte for byte accurate level, show performance
differences, RAM CPU and binary/docker image sizes before and after rustification").

## 1. What "byte-for-byte accurate" means here

Portal is a network service, not a file format. Its observable output is HTTP responses
(status line, headers, body bytes) produced from a Postgres/Redis state plus a little
external I/O (Frame.io, SMTP). So the only rigorous reading of "byte-for-byte accurate" is:

> Given the **same database and Redis state** and the **same request**, the Rust service emits
> the **identical response bytes** as the Python service: same status code, same header set
> (excluding host-assigned transport headers), same raw body bytes, same JSON key order,
> same whitespace, same number formatting, same error shapes.

To make that checkable rather than aspirational:

- Fixture data uses **fixed UUIDs, fixed timestamps and fixed secret values**, so responses that
  embed ids/timestamps are reproducible.
- Request ids are pinned via the `X-Request-Id` request header the app already honours.
- The differential harness replays a request matrix against both stacks and diffs the raw bytes.
- Volatile transport headers are excluded from the diff and listed explicitly:
  `date`, `server`, `x-request-id` (when generated), `content-length` (recomputed), and any
  `set-cookie` whose value is a signed/timestamped session token.

Everything else must match byte for byte.

## 2. Scope

**In scope** — the backend service (`apps/api`, FastAPI, ~9.5k LOC Python, 91 routes):

| Layer | Keep / port |
|---|---|
| HTTP API (`routes/`) | port to Rust |
| Auth (password, TOTP, WebAuthn, session cookies, lockout, rate limit) | port |
| DB models + Alembic schema | **keep the schema**; Rust uses the same migrations |
| Upload/download link engines | port |
| Sync engine + worker (arq → Rust queue worker) | port |
| Frame.io client / OAuth | port |
| Email/notify, verify (OTP by email) | port |
| `seed.py` first-boot admin seed | port |

**Frontend** — the SvelteKit web app (`apps/web`) is built to a **static SPA** (`adapter-static`,
`fallback: index.html`, `ssr = false`) and **served by the Rust process** on the same origin as
`/api/*`. This removes Node from the runtime: the same Rust server answers the API and returns
SvelteKit's own built HTML/JS/CSS byte-for-byte from disk. The route loads are universal (they
call the API with relative `fetch`), so SvelteKit's compiler output is untouched — Rust is a file
server for it, which is the only way "byte-for-byte" can apply to HTML.

The 19 `+page.server.ts`/`+layout.server.ts` files were converted to `+page.ts`/`+layout.ts`.
Node remains a **build-time** tool only.

**Container**: a Rust all-in-one image replaces the Python venv layer; Postgres/Redis/Node/Caddy
stay. This is what makes image-size comparison meaningful.

## 3. Equivalence protocol

1. `tools/rustify/capture.py` — drive the Python service (real uvicorn over HTTP) with a
   fixture-loaded DB and a request matrix; store raw responses (`status`, headers, body) under
   `docs/rustification/golden/`.
2. Same matrix against the Rust service → `docs/rustification/actual/`.
3. `tools/rustify/diff.py` — byte diff with the explicit exclusion list from §1; exit non-zero on
   any non-excluded difference. This is the pass/fail gate.
4. Ported pytest suite for behavioural coverage (the 36-file suite is the semantic spec).

## 4. Metrics protocol (the "show performance differences" part)

Measured on this machine (10 logical CPUs, 24 GiB RAM, macOS 27 arm64, Docker 28), same load
generator, same fixture DB, same endpoint mix, three repeats, median reported:

- **Docker image size**: `docker image inspect` size of the all-in-one image, before (Python) and
  after (Rust). Also `docker history` layer breakdown.
- **Binary / artefact size**: Rust release binary size; the Python analogue is reported honestly
  (no single binary — venv + bytecode size, and a PyInstaller onefile for reference).
- **Idle RAM**: container RSS (cgroup `memory.current`) and per-process RSS at steady state.
- **Under-load RAM**: peak RSS during a fixed 60 s constant-concurrency run.
- **CPU**: `docker stats` CPU% and cgroup `cpu.stat` usage during the load run; plus
  requests/second and p50/p95/p99 latency.
- **Cold start**: time from container start to `/api/health` returning 200.

Reported in `docs/rustification/RESULTS.md` with the exact commands used, so the numbers are
reproducible.

## 5. Assumptions and constants

- Working on branch `rust-port`; `main` is untouched.
- Behind-proxy binding only (`127.0.0.1:18080`); nothing binds 80/443 on this host.
- The Postgres schema is owned by the existing Alembic migrations; Rust does not fork it.
- Frame.io live calls are stubbed at the `FrameioClient` seam for the equivalence matrix (no real
  Adobe credentials available); the client's request-shaping is unit-tested against recorded
  fixtures instead. Values are reported, not hidden.
- Load generator: a purpose-built async client in `tools/rustify/load.py` (no external dep), to
  keep before/after identical.
