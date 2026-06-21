#!/bin/sh
# API program: wait for the supervised Postgres to accept connections, then serve. Migrations and
# the admin seed already ran in the entrypoint (before supervisord), so this is just uvicorn.
set -e

until pg_isready -h 127.0.0.1 -p 5432 -U portal -d portal >/dev/null 2>&1; do
	echo "[api] waiting for postgres..."
	sleep 1
done

exec uvicorn portal.main:app --host 127.0.0.1 --port 8000 --workers "${UVICORN_WORKERS:-1}"
