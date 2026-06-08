#!/bin/sh
set -e

case "$1" in
  api)
    # Apply migrations before serving. The admin user is seeded on app startup (lifespan).
    alembic upgrade head
    exec uvicorn portal.main:app --host 0.0.0.0 --port 8000 --workers "${UVICORN_WORKERS:-1}"
    ;;
  worker)
    exec arq portal.sync.worker.WorkerSettings
    ;;
  *)
    exec "$@"
    ;;
esac
