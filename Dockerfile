# All-in-one Portal image (farbhaus/portal). One Rust binary (apps/api-rs) serves the API and the
# SvelteKit web app (built to static files) and runs the sync worker; Postgres, Redis and an
# internal Caddy router run beside it in one container supervised by supervisord. End-user
# deployment is a single service + one /data volume behind a host reverse proxy. No Python or Node
# at runtime. For local development, `make dev` builds and runs this image from source.
#
# Build from the repo root:  docker build -t farbhaus/portal:dev .

# ---- web build (build-time only; the output is static files) -----------------------------------
FROM node:22-trixie-slim AS web-build
WORKDIR /web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web/ ./
RUN npm run build

# ---- rust build --------------------------------------------------------------------------------
FROM rust:1.99-slim AS rust-build
# webauthn-rs links OpenSSL, so the builder needs pkg-config + headers.
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
# Dependencies first, against a stub main: this layer only rebuilds when Cargo.toml/Cargo.lock
# change, so a code-only change recompiles just the portal-api crate.
COPY apps/api-rs/Cargo.toml apps/api-rs/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked --bin portal-api \
    && rm -rf src
COPY apps/api-rs/src ./src
# Compiled into the binary (src/migrate.rs): the head schema and the migration steps.
COPY apps/api-rs/schema.sql ./schema.sql
COPY apps/api-rs/openapi.yaml ./openapi.yaml
COPY apps/api-rs/migrations ./migrations
RUN touch src/main.rs && cargo build --release --locked --bin portal-api

# ---- caddy ---------------------------------------------------------------------------------------
# Built from source rather than copied from the caddy image: that image's binary is compiled with
# whatever Go was current at release and so trails Go's security releases (image scans flag its
# stdlib). Same release and the standard module set, compiled with a patched Go.
FROM golang:1.26-trixie AS caddy-build
ARG CADDY_VERSION=v2.11.7
# Security releases of Caddy's dependencies that postdate CADDY_VERSION (drop each pin once a Caddy
# release requires that version itself; the version check below catches a pin that would
# downgrade Caddy).
ARG CADDY_DEP_PINS="golang.org/x/net@v0.60.0"
WORKDIR /caddy
# The main package xcaddy generates — Caddy's command plus its standard modules — so the binary
# reports its version and resolves dependencies as a release build does (plus the pins above).
RUN printf 'package main\n\nimport (\n\tcaddycmd "github.com/caddyserver/caddy/v2/cmd"\n\t_ "github.com/caddyserver/caddy/v2/modules/standard"\n)\n\nfunc main() { caddycmd.Main() }\n' > main.go \
    && go mod init caddy \
    && go get github.com/caddyserver/caddy/v2@${CADDY_VERSION} ${CADDY_DEP_PINS} \
    && go mod tidy \
    && test "$(go list -m -f '{{.Version}}' github.com/caddyserver/caddy/v2)" = "${CADDY_VERSION}" \
    && CGO_ENABLED=0 go build -trimpath -ldflags "-s -w" -o /usr/local/bin/caddy .

# ---- final image -------------------------------------------------------------------------------
FROM debian:trixie-slim AS final

RUN set -eux; \
    apt-get update; \
    apt-get upgrade -y; \
    apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg lsb-release postgresql-common redis-server redis-tools supervisor; \
    /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y; \
    apt-get install -y --no-install-recommends postgresql-16; \
    # ssl-cert's placeholder key pair: Postgres here runs its own cluster with SSL off (loopback
    # only), so nothing uses it — and a private key in the image is a finding for every scan.
    rm -f /etc/ssl/private/ssl-cert-snakeoil.key /etc/ssl/certs/ssl-cert-snakeoil.pem; \
    rm -rf /var/lib/apt/lists/*
COPY --from=caddy-build /usr/local/bin/caddy /usr/bin/caddy

ENV PATH="/usr/lib/postgresql/16/bin:$PATH" \
    DATABASE_URL="postgresql://portal@127.0.0.1:5432/portal" \
    REDIS_URL="redis://127.0.0.1:6379/0" \
    HOST="127.0.0.1" \
    PORT="8000" \
    WEB_DIR="/app/web/build" \
    XDG_DATA_HOME="/data/caddy" \
    XDG_CONFIG_HOME="/data/caddy"

# Unprivileged user for the API (postgres/redis bring their own; the sync worker runs as root to
# write to host mounts — see supervisord.conf).
RUN groupadd --gid 10001 app \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin app

WORKDIR /app
COPY --from=rust-build /src/target/release/portal-api /usr/local/bin/portal-api
COPY --from=web-build /web/build /app/web/build

COPY docker/container/Caddyfile /etc/caddy/Caddyfile
COPY docker/container/supervisord.conf /etc/supervisor/supervisord.conf
COPY docker/container/portal-entrypoint.sh docker/container/portal-start-api.sh docker/container/portal-start-worker.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/portal-entrypoint.sh /usr/local/bin/portal-start-api.sh /usr/local/bin/portal-start-worker.sh

VOLUME ["/data"]
EXPOSE 80
ENTRYPOINT ["/usr/local/bin/portal-entrypoint.sh"]
