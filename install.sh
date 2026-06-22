#!/usr/bin/env bash
#
# install.sh — zero-checkout bootstrap for a Portal deploy host.
#
# Fetches just the deploy artifacts (no source tree) into a target directory and hands off to
# deploy.sh. This needs the repo to be public (and the farbhaus/portal image published); until then,
# `git clone` the repo and run ./deploy.sh directly.
#
#   curl -fsSL https://raw.githubusercontent.com/farbhaus/portal/main/install.sh \
#     | bash -s -- portal.example.com
#
# Everything after `--` is forwarded to deploy.sh, so all of its flags work:
#   … | bash -s -- --behind-proxy portal.example.com
#   … | bash -s -- --admin-email you@example.com portal.example.com
#   … | bash -s -- --init-env portal.example.com
#
# Env knobs:
#   PORTAL_REF=main          git ref/tag to fetch the deploy files from
#   PORTAL_DIR=/opt/portal   where to place them
#   PORTAL_NO_RUN=1          fetch only, don't run deploy.sh (dry run)
set -euo pipefail

REPO="farbhaus/portal"
REF="${PORTAL_REF:-main}"
DIR="${PORTAL_DIR:-/opt/portal}"
RAW="https://raw.githubusercontent.com/${REPO}/${REF}"

# Only the files a deploy host needs: the compose file, the env template, the host proxy snippet,
# and the deploy script itself. (The compose file references the published image, so no source
# build is required.)
FILES=(docker-compose.yml .env.example host-caddy.example deploy.sh)

SUDO=""
[[ ${EUID:-$(id -u)} -ne 0 ]] && command -v sudo >/dev/null 2>&1 && SUDO="sudo"

echo "==> Fetching Portal deploy files (${REPO}@${REF}) into ${DIR}"
$SUDO mkdir -p "$DIR"
# Make the target writable by the invoking user so deploy.sh can write .env without sudo each step.
[[ -n "$SUDO" ]] && $SUDO chown -R "$(id -u):$(id -g)" "$DIR"

for f in "${FILES[@]}"; do
  echo "    - $f"
  curl -fsSL "${RAW}/${f}" -o "${DIR}/${f}"
done
chmod +x "${DIR}/deploy.sh"

if [[ "${PORTAL_NO_RUN:-0}" == "1" ]]; then
  echo "==> PORTAL_NO_RUN=1 — files fetched, not starting. Run: ${DIR}/deploy.sh <domain>"
  exit 0
fi

echo "==> Handing off to deploy.sh"
cd "$DIR"
exec ./deploy.sh "$@"
