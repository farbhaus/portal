#!/bin/sh
# Rust service: wait for the supervised Postgres, then serve API + static SPA on one origin.
# Schema + admin seed already ran in the entrypoint.
set -e
until pg_isready -h 127.0.0.1 -p 5432 -U portal -d portal >/dev/null 2>&1; do
	echo "[api] waiting for postgres..."
	sleep 1
done
exec portal-api
