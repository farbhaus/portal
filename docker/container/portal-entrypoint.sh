#!/bin/sh
# First-boot init for the all-in-one Portal container, then hand off to supervisord.
#
# Everything stateful lives under /data (a single mounted volume): the Postgres cluster, the
# Redis AOF, Caddy's data dir, and the generated secrets. A fresh /data is fully self-setting-up
# — no migration from a previous deploy is required.
set -e

DATA=/data
PGDATA="$DATA/postgres"
SECRETS="$DATA/secrets"

mkdir -p "$PGDATA" "$DATA/redis" "$DATA/caddy" "$SECRETS"
chown postgres:postgres "$PGDATA"
chown redis:redis "$DATA/redis" 2>/dev/null || true
chmod 700 "$SECRETS"

# --- Secrets -----------------------------------------------------------------------------------
# For each crypto secret: an explicit env value wins (and is persisted); otherwise reuse the
# value persisted on a previous boot; otherwise generate one and persist it. This keeps cookies
# signable and stored Frame.io/SMTP tokens decryptable across restarts without the deployer
# having to manage these by hand. TOKEN_ENCRYPTION_KEY in particular must stay stable, which it
# does because it co-lives with the database it encrypts under /data.
gen() { python -c "import secrets; print(secrets.token_urlsafe(32))"; }
for name in SESSION_SECRET TOKEN_ENCRYPTION_KEY; do
	file="$SECRETS/$name"
	cur=$(eval "printf '%s' \"\${$name:-}\"")
	if [ -n "$cur" ]; then
		printf '%s' "$cur" >"$file"
	elif [ -s "$file" ]; then
		export "$name=$(cat "$file")"
	else
		val=$(gen)
		printf '%s' "$val" >"$file"
		export "$name=$val"
	fi
	chmod 600 "$file"
done

# --- Postgres init + migrations + seed --------------------------------------------------------
# Local + 127.0.0.1 connections use trust auth: the cluster is reachable only from inside this
# container (Postgres binds loopback and the container publishes only :80), so no password is
# needed. We run schema migrations and the admin seed HERE, against a temporary Postgres, before
# starting supervisord — so the supervised api and worker never race an unmigrated schema.
if [ ! -s "$PGDATA/PG_VERSION" ]; then
	echo "[entrypoint] initializing PostgreSQL data directory"
	su -s /bin/sh postgres -c "initdb -D '$PGDATA' -U postgres --auth-local=trust --auth-host=trust --encoding=UTF8"
fi

echo "[entrypoint] starting temporary Postgres for migrations"
su -s /bin/sh postgres -c "pg_ctl -D '$PGDATA' -o '-c listen_addresses=127.0.0.1' -w start"
# Create the portal role + database if they don't exist yet (first boot).
if ! su -s /bin/sh postgres -c "psql -h 127.0.0.1 -U postgres -d postgres -tAc \"SELECT 1 FROM pg_roles WHERE rolname='portal'\"" | grep -q 1; then
	su -s /bin/sh postgres -c "psql -h 127.0.0.1 -U postgres -d postgres -v ON_ERROR_STOP=1 \
		-c \"CREATE ROLE portal LOGIN;\" -c \"CREATE DATABASE portal OWNER portal;\""
fi

echo "[entrypoint] applying migrations + seeding admin"
( cd /app && alembic upgrade head && python -m portal.seed )

su -s /bin/sh postgres -c "pg_ctl -D '$PGDATA' -w stop"

echo "[entrypoint] starting supervisord"
exec supervisord -n -c /etc/supervisor/supervisord.conf
