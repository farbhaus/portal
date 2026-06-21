# Portal — thin wrappers around the all-in-one image (production + dev).
#
#   make deploy    start Portal on a host (creates .env on first run)
#   make update    pull the newest image and recreate (data + live transfers survive)
#   make down      stop Portal (keeps the data volume)
#   make logs      follow logs
#   make status    per-process supervisord state inside the container
#   make backup    dump the database to portal-backup-<timestamp>.sql
#   make shell     open a shell in the container
#   make dev       build & run the all-in-one image from source, with dev settings
#   make build     build the all-in-one image locally
#   make check     run static checks (ruff, mypy, svelte-check)
#   make test      run the backend test suite (spins throwaway Postgres/Redis)
#
# Both production and `dev` run the single all-in-one image; `dev` builds it from source
# (docker/docker-compose.dev.yml) on a separate port/volume. See docs/DEPLOY.md.

DEV := docker compose -f docker/docker-compose.dev.yml

.DEFAULT_GOAL := help
.PHONY: help deploy update down restart logs status backup shell dev dev-down build check test

help:  ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-9s\033[0m %s\n", $$1, $$2}'

# --- Production (all-in-one image) --------------------------------------------

deploy:  ## Start Portal (pulls the image; creates .env on first run)
	@test -f .env || { cp .env.example .env; \
		echo "Created .env from .env.example — set BASE_URL, ADMIN_EMAIL and ADMIN_PASSWORD, then run 'make deploy' again."; \
		exit 1; }
	docker compose up -d

update:  ## Pull the newest image and recreate (data + live transfers survive)
	docker compose pull
	docker compose up -d

down:  ## Stop Portal (keeps the data volume)
	docker compose down

restart:  ## Restart Portal
	docker compose restart

logs:  ## Follow logs
	docker compose logs -f

status:  ## Per-process supervisord state inside the container
	docker compose exec portal supervisorctl status

backup:  ## Dump the database to portal-backup-<timestamp>.sql
	docker compose exec -T portal pg_dump -U portal portal > portal-backup-$$(date +%Y%m%d-%H%M%S).sql

shell:  ## Open a shell in the container
	docker compose exec portal bash

# --- Development --------------------------------------------------------------

dev:  ## Build & run the all-in-one image from source (dev settings, http://localhost:18099)
	$(DEV) up -d --build

dev-down:  ## Stop the dev container
	$(DEV) down

build:  ## Build the all-in-one image locally (farbhaus/portal:dev)
	docker build -t farbhaus/portal:dev .

check:  ## Run static checks (ruff, mypy, svelte-check)
	cd apps/api && uv run ruff check && uv run mypy src
	cd apps/web && npm run check

test:  ## Run the backend test suite (spins throwaway Postgres/Redis)
	docker run -d --rm -p 55432:5432 -e POSTGRES_USER=portal -e POSTGRES_PASSWORD=portal -e POSTGRES_DB=portal --name portal_test_pg postgres:16 >/dev/null && \
	docker run -d --rm -p 56379:6379 --name portal_test_redis redis:7 >/dev/null && \
	(cd apps/api && uv run pytest); rc=$$?; \
	docker rm -f portal_test_pg portal_test_redis >/dev/null; exit $$rc
