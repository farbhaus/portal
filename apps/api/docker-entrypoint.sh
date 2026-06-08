#!/bin/sh
set -e

case "$1" in
  api)
    # Apply migrations and seed the admin once, before workers spawn (avoids a multi-worker
    # seed race on the unique admin email).
    alembic upgrade head
    python -m portal.seed
    exec uvicorn portal.main:app --host 0.0.0.0 --port 8000 --workers "${UVICORN_WORKERS:-1}"
    ;;
  worker)
    exec arq portal.sync.worker.WorkerSettings
    ;;
  *)
    exec "$@"
    ;;
esac
