# Portal — thin wrappers around the all-in-one image (production + dev).
#
#   make deploy    start Portal (creates .env on first run; ./deploy.sh <domain> is the one-click path)
#   make update    back up the database, then pull the newest image and recreate
#   make down      stop Portal (keeps the data volume)
#   make logs      follow logs
#   make status    per-process supervisord state inside the container
#   make backup    dump the database to portal-backup-<timestamp>.sql
#   make shell     open a shell in the container
#   make dev       same as deploy, but build the image from source instead of pulling it
#   make build     build the all-in-one image locally
#   make check     run static checks (clippy, svelte-check)
#   make test      run the Rust tests and the schema check
#
# `deploy`/`update` pull the published image; `dev` adds a --build overlay
# (docker/docker-compose.dev.yml) so the same compose builds from source. See DEPLOY.md.

DEV := docker compose -f docker-compose.yml -f docker/docker-compose.dev.yml

.DEFAULT_GOAL := help
.PHONY: help deploy update down restart logs status backup shell dev build check test

help:  ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-9s\033[0m %s\n", $$1, $$2}'

# --- Production (all-in-one image) --------------------------------------------

deploy:  ## Start Portal (pulls the image; creates .env on first run)
	@test -f .env || { cp .env.example .env; \
		echo "Created .env from .env.example — set BASE_URL, ADMIN_EMAIL and ADMIN_PASSWORD, then run 'make deploy' again."; \
		echo "(Or run ./deploy.sh <domain> for a fully automated one-click deploy.)"; \
		exit 1; }
	docker compose up -d

# An update can migrate the schema one way (a 2.x database won't start a 1.x image), so it backs up
# first: restoring that dump is the way back. SKIP_BACKUP=1 updates without one.
update:  ## Back up the database, then pull the newest image and recreate (data + live transfers survive)
	@if [ -n "$(SKIP_BACKUP)" ]; then echo "Skipping the backup (SKIP_BACKUP is set)."; \
	elif docker compose ps --status running --services 2>/dev/null | grep -qx portal; then $(MAKE) --no-print-directory backup; \
	else echo "Portal isn't running, so there's no database to back up. Start it with 'make deploy' and re-run," >&2; \
		echo "or run 'make update SKIP_BACKUP=1' to update without a backup." >&2; exit 1; fi
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

# Owner-only: the dump holds client names, emails and transfer history.
backup:  ## Dump the database to portal-backup-<timestamp>.sql
	@f=portal-backup-$$(date +%Y%m%d-%H%M%S).sql; umask 077; \
	if docker compose exec -T portal pg_dump -U portal portal > $$f && [ -s $$f ]; then \
		echo "Backed up the database to $$f ($$(du -h $$f | cut -f1))"; \
	else rm -f $$f; echo "Backup failed; nothing was changed." >&2; exit 1; fi

shell:  ## Open a shell in the container
	docker compose exec portal bash

# --- Development --------------------------------------------------------------

dev:  ## Same as deploy, but build the image from source instead of pulling it
	$(DEV) up -d --build

build:  ## Build the all-in-one image locally (farbhaus/portal:dev)
	docker build -t farbhaus/portal:dev .

check:  ## Run static checks (clippy, svelte-check)
	cd apps/api-rs && cargo clippy --locked --all-targets -- -D warnings
	cd mcp && cargo clippy --locked --all-targets -- -D warnings
	cd apps/web && npm run check

test:  ## Run the Rust tests and the schema check (throwaway Postgres)
	cd apps/api-rs && cargo test --locked
	cd mcp && cargo test --locked
	tools/schema_check.sh
