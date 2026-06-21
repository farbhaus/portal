# All-in-one Portal image (farbhaus/portal). Bundles the api, the sync worker, the SvelteKit web
# server, Postgres, Redis and an internal Caddy router into one container supervised by
# supervisord. End-user deployment is a single service + one /data volume behind a host reverse
# proxy. For local development, `make dev` builds and runs this image from source.
#
# Build from the repo root:  docker build -t farbhaus/portal:dev .

# ---- web build -------------------------------------------------------------------------------
FROM node:22-trixie-slim AS web-build
WORKDIR /web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web/ ./
RUN npm run build && npm prune --omit=dev

# ---- api venv build --------------------------------------------------------------------------
FROM python:3.14-slim AS api-build
COPY --from=ghcr.io/astral-sh/uv:0.11 /uv /bin/uv
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    PYTHONUNBUFFERED=1
WORKDIR /app
COPY apps/api/pyproject.toml apps/api/uv.lock ./
RUN uv sync --frozen --no-install-project --no-dev
COPY apps/api/src ./src
COPY apps/api/alembic ./alembic
COPY apps/api/alembic.ini ./
RUN uv sync --frozen --no-dev

# ---- final image -----------------------------------------------------------------------------
FROM python:3.14-slim AS final

# Postgres 16 (PGDG repo for the right Debian codename), Redis, the Node runtime, supervisor and
# client tools. Caddy is a static binary copied from the official image.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg lsb-release \
        postgresql-common redis-server redis-tools supervisor nodejs; \
    /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y; \
    apt-get install -y --no-install-recommends postgresql-16; \
    rm -rf /var/lib/apt/lists/*
COPY --from=caddy:2 /usr/bin/caddy /usr/bin/caddy

ENV PATH="/app/.venv/bin:/usr/lib/postgresql/16/bin:$PATH" \
    DATABASE_URL="postgresql+asyncpg://portal@127.0.0.1:5432/portal" \
    REDIS_URL="redis://127.0.0.1:6379/0" \
    INTERNAL_API_URL="http://127.0.0.1:8000" \
    NODE_ENV="production" \
    HOST="127.0.0.1" \
    PORT="3000" \
    XDG_DATA_HOME="/data/caddy" \
    PYTHONUNBUFFERED=1

# Unprivileged user shared by the api and web processes (postgres/redis bring their own).
RUN groupadd --gid 10001 app \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin app

WORKDIR /app
COPY --from=api-build /app /app
COPY --from=web-build /web/build /app/web/build
COPY --from=web-build /web/node_modules /app/web/node_modules
COPY --from=web-build /web/package.json /app/web/package.json

COPY docker/container/Caddyfile /etc/caddy/Caddyfile
COPY docker/container/supervisord.conf /etc/supervisor/supervisord.conf
COPY docker/container/portal-entrypoint.sh docker/container/portal-start-api.sh docker/container/portal-start-worker.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/portal-entrypoint.sh /usr/local/bin/portal-start-api.sh /usr/local/bin/portal-start-worker.sh

VOLUME ["/data"]
EXPOSE 80
ENTRYPOINT ["/usr/local/bin/portal-entrypoint.sh"]
