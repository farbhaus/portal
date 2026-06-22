#!/usr/bin/env bash
#
# deploy.sh — one-click production deployment for Portal.
#
# From a clean host to a running HTTPS deployment in one command (run with bash, not sh). It takes
# the public domain and the admin email (the second is required — it's your login):
#
#   ./deploy.sh portal.example.com you@example.com                 standalone — Portal gets its own
#                                                  Let's Encrypt cert and owns ports 80/443.
#   ./deploy.sh --behind-proxy portal.example.com you@example.com  Portal binds a loopback port; a
#                                                  reverse proxy you already run forwards to it.
#
# It installs Docker + Compose if missing, writes a minimal .env (generating a strong ADMIN_PASSWORD,
# shown once — change it after first login), opens the firewall for standalone, pulls the published
# farbhaus/portal image, and waits until the container is healthy. The container derives the rest
# (Caddy's TLS, proxy-hop count) from BASE_URL, and generates its own crypto secrets on first boot —
# so this script sets only your domain, the admin login, and the port knobs for the chosen mode.
#
# Flags:
#   --behind-proxy HOST   deploy behind an external TLS proxy (loopback mode). Skips the firewall
#                         and the 80/443 free-port check.
#   --port N              loopback port for --behind-proxy mode (default 18080).
#   --admin-email EMAIL   admin login email (required; alternatively pass it as the 2nd argument).
#   --update              reuse .env, pull the newest image, recreate (data + live transfers survive).
#   --init-env            write/refresh .env and exit (no image pull, nothing started).
#   --yes / -y            skip confirmation prompts.
#
# Re-running is safe: a complete existing .env is reused as-is (BASE_URL is never overwritten, so a
# redeploy can't silently change the host). NOTE: ADMIN_PASSWORD seeds the database on the FIRST
# boot only; on a host that already has a /data volume a freshly generated password here will NOT
# replace the stored one — manage the admin password from Portal's Settings instead.
#
set -euo pipefail
umask 077          # the generated ADMIN_PASSWORD lands in .env — keep it non-world-readable

# --- locate repo root -------------------------------------------------------
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

[[ -f .env.example ]] || { echo "FATAL: .env.example not found next to deploy.sh — run from the repo checkout." >&2; exit 1; }
[[ -f docker-compose.yml ]] || { echo "FATAL: docker-compose.yml not found next to deploy.sh." >&2; exit 1; }

# --- args -------------------------------------------------------------------
ASSUME_YES=0
UPDATE=0
INIT_ENV_ONLY=0
BEHIND_PROXY=""    # --behind-proxy HOST: external TLS proxy fronts the container (loopback mode)
PORTAL_PORT_ARG="" # --port N (behind-proxy loopback port)
ADMIN_EMAIL_ARG="" # --admin-email
DOMAIN=""
POSITIONAL=()      # non-flag args: [domain] [admin-email]
while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y) ASSUME_YES=1 ;;
    --update) UPDATE=1 ;;
    --init-env) INIT_ENV_ONLY=1 ;;
    --behind-proxy)
      shift
      [[ -n "${1:-}" && "$1" != -* ]] || { echo "FATAL: --behind-proxy needs the public host, e.g. --behind-proxy portal.example.com" >&2; exit 1; }
      BEHIND_PROXY="$1" ;;
    --behind-proxy=*) BEHIND_PROXY="${1#*=}" ;;
    --port)
      shift
      [[ -n "${1:-}" && "$1" != -* ]] || { echo "FATAL: --port needs a number, e.g. --port 18080" >&2; exit 1; }
      PORTAL_PORT_ARG="$1" ;;
    --port=*) PORTAL_PORT_ARG="${1#*=}" ;;
    --admin-email)
      shift
      [[ -n "${1:-}" && "$1" != -* ]] || { echo "FATAL: --admin-email needs an address, e.g. --admin-email you@example.com" >&2; exit 1; }
      ADMIN_EMAIL_ARG="$1" ;;
    --admin-email=*) ADMIN_EMAIL_ARG="${1#*=}" ;;
    -*) echo "FATAL: unknown option: $1" >&2; exit 1 ;;
    *) POSITIONAL+=("$1") ;;
  esac
  shift
done

# Resolve positionals after the loop (so order vs flags doesn't matter): the domain is the public
# host (or comes from --behind-proxy), and the admin email is the next positional. --admin-email
# always wins over a positional email.
if [[ -n "$BEHIND_PROXY" ]]; then
  DOMAIN="$BEHIND_PROXY"
  [[ ${#POSITIONAL[@]} -ge 1 && -z "$ADMIN_EMAIL_ARG" ]] && ADMIN_EMAIL_ARG="${POSITIONAL[0]}"
else
  [[ ${#POSITIONAL[@]} -ge 1 ]] && DOMAIN="${POSITIONAL[0]}"
  [[ ${#POSITIONAL[@]} -ge 2 && -z "$ADMIN_EMAIL_ARG" ]] && ADMIN_EMAIL_ARG="${POSITIONAL[1]}"
fi

# Privilege prefix for host-level changes (installing packages, firewall).
SUDO=""
[[ ${EUID:-$(id -u)} -ne 0 ]] && SUDO="sudo"

# --- helpers ----------------------------------------------------------------
die()  { echo "FATAL: $*" >&2; exit 1; }
info() { echo "==> $*"; }

# openssl rather than a `tr </dev/urandom | head` pipeline: that trips SIGPIPE, which under
# `set -o pipefail` + `set -e` silently aborts the whole script.
gen_password() { openssl rand -hex 16; }   # 32 hex chars — well past any sane minimum

# set_env KEY VALUE — replace the `KEY=...` line in .env in place, preserving comments / blank
# lines / ordering. Portable (no GNU-vs-BSD `sed -i`). Appends the key if absent.
set_env() {
  local key="$1" value="$2" tmp found=0 line
  tmp="$(mktemp)"
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == "${key}="* ]]; then
      printf '%s=%s\n' "$key" "$value" >>"$tmp"; found=1
    else
      printf '%s\n' "$line" >>"$tmp"
    fi
  done <.env
  [[ $found -eq 1 ]] || printf '%s=%s\n' "$key" "$value" >>"$tmp"
  mv "$tmp" .env
}

# get_env KEY — print the value of KEY from .env (empty if unset/commented). `|| true` guards the
# grep miss under `set -e`.
get_env() { { grep -E "^$1=" .env 2>/dev/null || true; } | head -1 | cut -d= -f2-; }

have_apt() { command -v apt-get >/dev/null 2>&1; }

install_apt() { info "Installing $* (apt)"; $SUDO apt-get update; $SUDO apt-get install -y "$@"; }

# install_docker — Docker Engine + Compose v2 plugin via the official convenience script.
install_docker() {
  have_apt || die "Docker not found and auto-install only supports apt-based distros. Install Docker Engine + the Compose v2 plugin: https://docs.docker.com/engine/install/"
  info "Installing Docker Engine + Compose plugin (get.docker.com)"
  curl -fsSL https://get.docker.com | $SUDO sh
  $SUDO systemctl enable --now docker
  local u="${SUDO_USER:-$USER}"
  [[ -n "$SUDO" && -n "$u" ]] && $SUDO usermod -aG docker "$u" || true
}

# http_ports_busy — true if something is already listening on :80 or :443 (a foreign web server).
# Standalone can't share those, so we fail fast rather than emit a cryptic Docker bind error.
http_ports_busy() {
  if command -v ss >/dev/null 2>&1; then
    ss -tln 'sport = :80 or sport = :443' 2>/dev/null | grep -q LISTEN
  elif command -v netstat >/dev/null 2>&1; then
    netstat -tln 2>/dev/null | grep -qE '[[:space:]][0-9.:*[]+:(80|443)[[:space:]].*LISTEN'
  else
    return 1
  fi
}

# stack_running — true if our own portal container is already up (so :80/:443 being held is a
# redeploy, not a foreign-service conflict).
stack_running() { $DOCKER ps --filter 'name=^portal$' --filter 'status=running' --format '{{.Names}}' 2>/dev/null | grep -qx portal; }

# wait_healthy [timeout_s] — block until the container's Docker healthcheck reports `healthy`.
wait_healthy() {
  local timeout="${1:-120}" elapsed=0 status
  info "Waiting for the container to become healthy (up to ${timeout}s)…"
  while (( elapsed < timeout )); do
    status="$($DOCKER inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' portal 2>/dev/null || echo missing)"
    case "$status" in
      healthy) info "Container is healthy."; return 0 ;;
      none)    info "Container has no healthcheck — skipping health wait."; return 0 ;;
    esac
    sleep 3; elapsed=$((elapsed + 3))
  done
  echo "FATAL: container did not become healthy within ${timeout}s. Recent logs:" >&2
  $DOCKER compose logs --tail 40 portal >&2 2>/dev/null || true
  return 1
}

# open_firewall — add allow rules for the web edge to whatever firewall is ALREADY active. Does NOT
# enable an inactive one (that risks an SSH lockout and is pointless — if nothing filters, the ports
# are already open). Only used in standalone mode (the container owns 80/443).
open_firewall() {
  if command -v ufw >/dev/null 2>&1 && $SUDO ufw status 2>/dev/null | grep -q "Status: active"; then
    info "Opening ports 80/443 via ufw"
    $SUDO ufw allow 22/tcp >/dev/null 2>&1 || true   # never lock out SSH
    $SUDO ufw allow 80/tcp  >/dev/null 2>&1 || true
    $SUDO ufw allow 443/tcp >/dev/null 2>&1 || true
    $SUDO ufw allow 443/udp >/dev/null 2>&1 || true
    $SUDO ufw reload >/dev/null 2>&1 || true
  elif command -v firewall-cmd >/dev/null 2>&1 && $SUDO firewall-cmd --state >/dev/null 2>&1; then
    info "Opening ports 80/443 via firewalld"
    $SUDO firewall-cmd --permanent --add-service=http  >/dev/null 2>&1 || true
    $SUDO firewall-cmd --permanent --add-service=https >/dev/null 2>&1 || true
    $SUDO firewall-cmd --permanent --add-port=443/udp  >/dev/null 2>&1 || true
    $SUDO firewall-cmd --reload >/dev/null 2>&1 || true
  else
    info "No active host firewall (ufw/firewalld) — skipping."
    echo "    If your VPS provider has a CLOUD firewall, open tcp 80, tcp 443 and udp 443 there."
  fi
}

# --- verify prerequisites ---------------------------------------------------
info "Checking prerequisites"
command -v curl    >/dev/null 2>&1 || install_apt curl ca-certificates
command -v openssl >/dev/null 2>&1 || install_apt openssl

# --init-env writes .env only (openssl-only); skip all Docker setup for it.
DOCKER="docker"
if [[ $INIT_ENV_ONLY -eq 0 ]]; then
  command -v docker >/dev/null 2>&1 || install_docker
  if command -v docker >/dev/null 2>&1 && ! docker compose version >/dev/null 2>&1; then
    have_apt && install_apt docker-compose-plugin
  fi
  # A freshly added docker group doesn't apply to this shell — fall back to sudo if the daemon
  # isn't reachable unprivileged.
  if ! docker info >/dev/null 2>&1; then
    if [[ -n "$SUDO" ]] && $SUDO docker info >/dev/null 2>&1; then
      DOCKER="$SUDO docker"
      info "Using '$DOCKER' (re-login, or 'newgrp docker', to drop sudo for docker)"
    fi
  fi
  command -v docker >/dev/null 2>&1 || die "docker still missing after install attempt — install it manually and re-run."
  $DOCKER compose version >/dev/null 2>&1 || die "Docker Compose v2 still unavailable — install the Compose plugin and re-run."
fi

# --- update mode ------------------------------------------------------------
# Pull the newest image and recreate, reusing .env. Secrets/data are untouched, so live transfers
# survive. One compose file serves both modes, so update needs no mode awareness.
if [[ $UPDATE -eq 1 ]]; then
  [[ -f .env ]] || die "--update needs an existing .env (run a normal deploy first)."
  info "Pulling newest image"
  $DOCKER compose pull
  info "Recreating the container"
  $DOCKER compose up -d
  wait_healthy || die "update failed — the container is not healthy (see logs above)."
  info "Update complete."
  exit 0
fi

# --- mode + domain ----------------------------------------------------------
if [[ -n "$BEHIND_PROXY" ]]; then MODE="proxy"; else MODE="standalone"; fi

if [[ -z "$DOMAIN" ]]; then
  # `|| true`: under `set -e`, read returns non-zero at EOF (e.g. `curl … | bash` has no TTY),
  # which would otherwise abort here instead of giving the clear message below.
  [[ -t 0 ]] && read -rp "Public domain (e.g. portal.example.com): " DOMAIN || true
fi
[[ -n "$DOMAIN" ]] || die "a public domain is required. Pass it as an argument: ./deploy.sh portal.example.com"

# Accept `localhost` (standalone local testing — Caddy's internal CA) or a bare FQDN. Reject a
# scheme/path/port and bare IPs (Let's Encrypt won't issue for an IP).
if [[ "$DOMAIN" == "localhost" ]]; then
  [[ "$MODE" == "standalone" ]] || die "'localhost' only makes sense for standalone local testing; drop --behind-proxy."
elif [[ "$DOMAIN" == *://* || "$DOMAIN" == */* || "$DOMAIN" =~ [[:space:]] ]]; then
  die "'$DOMAIN' is not a bare hostname. Use e.g. portal.example.com — no https://, no path, no port."
elif [[ "$DOMAIN" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ || "$DOMAIN" == *:* ]]; then
  die "bare IPs aren't supported (no Let's Encrypt TLS). Use 'localhost' for local testing or a real domain like portal.example.com."
elif [[ ! "$DOMAIN" =~ ^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+$ ]]; then
  die "'$DOMAIN' is not a bare hostname. Use e.g. portal.example.com — no https://, no path, no port."
fi

PORTAL_PORT="${PORTAL_PORT_ARG:-18080}"

# Standalone needs 80/443 free — unless our own stack already holds them (an idempotent redeploy).
if [[ "$MODE" == "standalone" && $INIT_ENV_ONLY -eq 0 ]] && http_ports_busy && ! stack_running; then
  die "ports 80/443 are already in use — standalone deploy needs them free to be the TLS edge.
       Free them, or if this host already runs a reverse proxy, deploy behind it instead:
       ./deploy.sh --behind-proxy $DOMAIN"
fi

# --- .env handling ----------------------------------------------------------
ADMIN_PASSWORD=""   # set only when we generate a fresh .env (printed once in the summary)

# env_complete — true if .env has the required values non-empty AND a real admin password (not the
# .env.example placeholder). Guards against reusing a half-written or untouched template.
env_complete() {
  local k
  for k in BASE_URL ADMIN_EMAIL ADMIN_PASSWORD; do
    grep -qE "^${k}=.+" .env || return 1
  done
  [[ "$(get_env ADMIN_PASSWORD)" != "change-me-to-a-strong-password" ]] || return 1
  return 0
}

if [[ -f .env ]] && ! env_complete; then
  info ".env exists but is incomplete (missing values or still the template password) — regenerating"
  rm -f .env
fi

if [[ -f .env ]]; then
  info "Reusing existing .env (admin password + secrets unchanged)"
  existing_base="$(get_env BASE_URL)"
  if [[ -n "$existing_base" && "$existing_base" != "https://$DOMAIN" ]]; then
    info "NOTE: .env already targets '$existing_base' — keeping it (ignoring '$DOMAIN')."
    info "      To switch host: edit BASE_URL in .env (and the mode knobs if changing how TLS is served), or start from a fresh .env."
  fi
else
  info "Generating .env for $DOMAIN ($MODE mode)"
  cp .env.example .env
  ADMIN_PASSWORD="$(gen_password)"
  # The admin email is REQUIRED — it's the login identity. Take it from --admin-email / the 2nd
  # positional, else prompt on a real terminal, else fail clearly (can't prompt under `curl … |
  # bash`, whose stdin is the pipe). A new strong password is generated and shown once below.
  admin_email="$ADMIN_EMAIL_ARG"
  if [[ -z "$admin_email" ]]; then
    if [[ -t 0 ]]; then
      while [[ -z "$admin_email" ]]; do
        read -rp "Admin email (your login for $DOMAIN): " admin_email || true
        if [[ -n "$admin_email" && ! "$admin_email" =~ ^[^@[:space:]]+@[^@[:space:]]+$ ]]; then
          echo "  '$admin_email' doesn't look like an email — try again."; admin_email=""
        fi
      done
    else
      die "an admin email is required. Pass it with --admin-email you@example.com, or as the second argument: ./deploy.sh $DOMAIN you@example.com"
    fi
  fi
  [[ "$admin_email" =~ ^[^@[:space:]]+@[^@[:space:]]+$ ]] || die "'$admin_email' is not a valid email address."
  set_env BASE_URL       "https://$DOMAIN"
  set_env ADMIN_EMAIL    "$admin_email"
  set_env ADMIN_PASSWORD "$ADMIN_PASSWORD"
  if [[ "$MODE" == "standalone" ]]; then
    # Portal is the public TLS edge: publish 80/443 and let the container provision Let's Encrypt
    # (the entrypoint derives Caddy's site address + 1 proxy hop from PORTAL_BEHIND_PROXY=false).
    set_env PORTAL_BEHIND_PROXY "false"
    set_env WEB_BIND            "0.0.0.0"
    set_env PORTAL_PORT         "80"
    set_env PORTAL_HTTPS_PORT   "443"
  else
    # Behind a host proxy: stay on the loopback port; the entrypoint derives the :80 HTTP listener
    # and 2 proxy hops. WEB_BIND keeps the default (127.0.0.1).
    set_env PORTAL_BEHIND_PROXY "true"
    set_env PORTAL_PORT         "$PORTAL_PORT"
  fi
  chmod 600 .env
fi

# --- init-env mode ----------------------------------------------------------
if [[ $INIT_ENV_ONLY -eq 1 ]]; then
  info ".env ready at $(pwd)/.env"
  if [[ -n "$ADMIN_PASSWORD" ]]; then
    echo
    echo "  ADMIN LOGIN (shown once — save it now):"
    echo "      email:    $(get_env ADMIN_EMAIL)"
    echo "      password: $ADMIN_PASSWORD"
    echo "  ⚠  Change this password after your first login (Settings → account)."
  fi
  echo "  Start when ready:  $DOCKER compose up -d"
  exit 0
fi

# --- pull image -------------------------------------------------------------
info "Pulling the Portal image"
$DOCKER compose pull \
  || die "could not pull the farbhaus/portal image. It's published on a version tag (v*) to Docker Hub — confirm the image exists and you can reach the registry, then re-run."

# --- firewall ---------------------------------------------------------------
# Only in standalone, where the container is the public edge. Behind a proxy the container binds
# 127.0.0.1, so there's nothing host-public to open (the front proxy owns 80/443 exposure).
[[ "$MODE" == "standalone" ]] && open_firewall

# --- deploy -----------------------------------------------------------------
info "Starting Portal"
$DOCKER compose up -d
wait_healthy || die "deploy failed — the container is not healthy (see logs above)."

# --- summary ----------------------------------------------------------------
data_exists=0
$DOCKER volume inspect portal_portal_data >/dev/null 2>&1 && data_exists=1 || true
echo
echo "============================================================"
echo " Portal deployed: https://$DOMAIN"
echo "============================================================"
if [[ -n "$ADMIN_PASSWORD" ]]; then
  echo
  echo "  ADMIN LOGIN (shown once — save it now):"
  echo "      email:    $(get_env ADMIN_EMAIL)"
  echo "      password: $ADMIN_PASSWORD"
  echo
  echo "  ⚠  This password was generated for first login — CHANGE IT after you sign in"
  echo "     (Settings → account), and consider adding a passkey or TOTP."
fi
if [[ "$MODE" == "standalone" ]]; then
  cat <<EOF

  Next steps (standalone — Portal provisions its own TLS):
   - Point DNS for $DOMAIN at this host. Caddy provisions Let's Encrypt on the first request.
   - Firewall: handled above (or listed there if no host firewall is active — open tcp 80/443 +
     udp 443 on your VPS provider's cloud firewall if you have one).
   - Log in at https://$DOMAIN, then connect Frame.io from Settings (see DEPLOY.md).
   - Update later:  ./deploy.sh --update
   - Check health:  $DOCKER compose ps

EOF
else
  cat <<EOF

  Next steps (behind your host reverse proxy):
   - Point your proxy for $DOMAIN at Portal's loopback port:
       $DOMAIN {
           reverse_proxy 127.0.0.1:$PORTAL_PORT
       }
     (Caddy syntax; nginx/Traefik equivalents just proxy $DOMAIN → 127.0.0.1:$PORTAL_PORT.)
   - The proxy terminates TLS; the container serves plain HTTP on loopback only.
   - Log in at https://$DOMAIN, then connect Frame.io from Settings (see DEPLOY.md).
   - Update later:  ./deploy.sh --update
   - Check health:  $DOCKER compose ps

EOF
fi
if [[ -n "$ADMIN_PASSWORD" && $data_exists -eq 1 ]]; then
  echo "  NOTE: a portal_data volume already existed. ADMIN_PASSWORD is seeded only on a FRESH"
  echo "  volume, so the password above may NOT be the active one — manage it from Settings."
  echo
fi
