#!/bin/sh
# Worker program: wait for Postgres + Redis, then run the arq worker (sync downloads + crons).
set -e

until pg_isready -h 127.0.0.1 -p 5432 -U portal -d portal >/dev/null 2>&1; do
	sleep 1
done
until redis-cli -h 127.0.0.1 ping >/dev/null 2>&1; do
	sleep 1
done

exec arq portal.sync.worker.WorkerSettings
