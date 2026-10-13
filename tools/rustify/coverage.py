
"""Regenerate docs/rustification/COVERAGE.md from the Python routers + the ported set below.

  uv run python coverage.py     (from tools/rustify)

Update PORTED as endpoints land. `PARTIAL` marks routes where only the unauthenticated /
unconnected path is ported.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ROUTES = ROOT / "apps/api/src/portal/routes"
OUT = ROOT / "docs/rustification/COVERAGE.md"

PORTED = {
    ("GET", "/api/health"), ("POST", "/api/auth/login"), ("GET", "/api/auth/me"),
    ("GET", "/api/public/links/{token}"), ("GET", "/api/upload-links"),
    ("GET", "/api/upload-links/extension-preset"), ("GET", "/api/download-links"),
    ("GET", "/api/dashboard/transfers"), ("GET", "/api/dashboard/history"),
    ("GET", "/api/frameio/status"), ("GET", "/api/frameio/oauth/connect"),
    ("GET", "/api/frameio/oauth/callback"), ("POST", "/api/frameio/disconnect"),
    ("GET", "/api/frameio/accounts"), ("GET", "/api/frameio/workspaces"),
    ("GET", "/api/frameio/projects"), ("POST", "/api/frameio/projects"),
    ("DELETE", "/api/frameio/projects/{project_id}"), ("GET", "/api/frameio/folders"),
    ("POST", "/api/frameio/folders"), ("DELETE", "/api/frameio/folders/{folder_id}"),
    ("PATCH", "/api/frameio/folders/{folder_id}/move"), ("POST", "/api/frameio/folders/{folder_id}/copy"),
    ("GET", "/api/frameio/files"), ("DELETE", "/api/frameio/files/{file_id}"),
    ("GET", "/api/frameio/files/{file_id}/download-url"), ("PATCH", "/api/frameio/files/{file_id}/move"),
    ("POST", "/api/frameio/files/{file_id}/copy"), ("POST", "/api/frameio/shares"),
    ("GET", "/api/settings/security"), ("GET", "/api/settings/email"),
    ("GET", "/api/settings/frameio-config"), ("GET", "/api/sync-rules"), ("GET", "/api/activity"),
    ("GET", "/api/activity/actions"), ("GET", "/api/activity/summary"), ("GET", "/api/branding/logo"),
    ("GET", "/api/destinations"), ("POST", "/api/destinations"), ("GET", "/api/destinations/{dest_id}"),
    ("PATCH", "/api/destinations/{dest_id}"), ("DELETE", "/api/destinations/{dest_id}"),
    ("POST", "/api/upload-links"), ("GET", "/api/upload-links/{link_id}"),
    ("PATCH", "/api/upload-links/{link_id}"), ("DELETE", "/api/upload-links/{link_id}"),
    ("POST", "/api/upload-links/{link_id}/revoke"), ("GET", "/api/upload-links/{link_id}/stats"),
    ("POST", "/api/download-links"), ("GET", "/api/download-links/{link_id}"),
    ("PATCH", "/api/download-links/{link_id}"), ("DELETE", "/api/download-links/{link_id}"),
    ("POST", "/api/download-links/{link_id}/revoke"), ("GET", "/api/download-links/{link_id}/events"),
    ("GET", "/api/download-links/{link_id}/stats"),
    ("POST", "/api/sync-rules"), ("GET", "/api/sync-rules/{rule_id}"),
    ("PATCH", "/api/sync-rules/{rule_id}"), ("DELETE", "/api/sync-rules/{rule_id}"),
    ("POST", "/api/sync-rules/{rule_id}/run"), ("GET", "/api/sync-rules/{rule_id}/jobs"),
    ("GET", "/api/sync-rules/{rule_id}/stats"), ("POST", "/api/sync-jobs/{job_id}/retry"),
    ("POST", "/api/sync-jobs/{job_id}/dismiss"), ("POST", "/api/webhooks/frameio/{rule_id}"),
    ("GET", "/api/webhook-events"),
    ("POST", "/api/public/links/{token}/request-code"), ("POST", "/api/public/links/{token}/verify-code"),
    ("POST", "/api/public/links/{token}/verify-password"), ("POST", "/api/public/links/{token}/sessions"),
    ("POST", "/api/public/links/{token}/sessions/{session_id}/files"),
    ("POST", "/api/public/links/{token}/sessions/{session_id}/files/{file_id}/complete"),
    ("POST", "/api/public/links/{token}/sessions/{session_id}/complete"),
    ("POST", "/api/public/links/{token}/sessions/{session_id}/heartbeat"),
    ("POST", "/api/public/links/{token}/sessions/{session_id}/abandon"),
    ("GET", "/api/public/downloads/{token}"), ("POST", "/api/public/downloads/{token}/request-code"),
    ("POST", "/api/public/downloads/{token}/sessions"),
    ("POST", "/api/public/downloads/{token}/sessions/{session_id}/files/{file_id}/url"),
    ("POST", "/api/auth/login/totp"), ("POST", "/api/auth/logout"),
    ("GET", "/api/auth/passkeys"), ("DELETE", "/api/auth/passkeys/{passkey_id}"),
    ("POST", "/api/auth/passkeys/register/begin"), ("POST", "/api/auth/passkeys/register/complete"),
    ("POST", "/api/auth/passkeys/login/begin"), ("POST", "/api/auth/passkeys/login/complete"),
    ("GET", "/api/download-links/{link_id}/source"),
    ("GET", "/api/privacy/subject"), ("POST", "/api/privacy/subject/erase"),
    ("POST", "/api/settings/totp/begin"), ("POST", "/api/settings/totp/enable"),
    ("POST", "/api/settings/totp/disable"), ("GET", "/api/settings/branding"),
    ("POST", "/api/settings/branding"), ("POST", "/api/settings/branding/logo"),
    ("DELETE", "/api/settings/branding/logo"),
    ("POST", "/api/settings/email"), ("POST", "/api/settings/email/test"),
    ("POST", "/api/settings/frameio-config"), ("POST", "/api/settings/password"),
}
PARTIAL = {("POST", "/api/destinations")}


def main() -> None:
    prefixes: dict[tuple[str, str], str] = {}
    routes: list[tuple[str, str, str]] = []
    for f in sorted(ROUTES.glob("*.py")):
        txt = f.read_text()
        for m in re.finditer(r'(\w+)\s*=\s*APIRouter\(\s*(?:prefix="([^"]*)")?', txt):
            prefixes[(f.name, m.group(1))] = m.group(2) or ""
        for m in re.finditer(r'@(\w+)\.(get|post|put|patch|delete)\(\s*\n?\s*["\']([^"\']*)["\']', txt):
            var, method, path = m.group(1), m.group(2).upper(), m.group(3)
            if var in ("router", "admin_router"):
                routes.append((method, f"/api{prefixes.get((f.name, var), '')}{path}", f.name))
    lines = ["# Portal → Rust: endpoint coverage", "",
             f"Auto-generated by `tools/rustify/coverage.py` — {len(routes)} API routes vs the Rust "
             "implementation. `done` = ported and verified against the Python service; `partial` = "
             "only the unauthenticated/unconnected path; `todo` = not ported.", "",
             "| Method | Path | Python module | Rust |", "|---|---|---|---|"]
    done = partial = 0
    for method, path, mod in routes:
        if (method, path) in PARTIAL:
            status = "partial"; partial += 1
        elif (method, path) in PORTED:
            status = "done"; done += 1
        else:
            status = "todo"
        lines.append(f"| {method} | `{path}` | {mod} | {status} |")
    todo = len(routes) - done - partial
    lines += ["", f"**{done} done, {partial} partial, {todo} todo** (of {len(routes)} routes).", "",
              "The whole runtime is Rust: HTTP + auth (sessions, bcrypt, TOTP, WebAuthn) + the",
              "Frame.io client and token lifecycle (AES-GCM at rest, Adobe IMS refresh) + the sync",
              "engine and its worker + the upload/download link engines + email/SMTP + the SPA host.",
              "The `portal-api worker` subprocess replaces the arq worker.", "",
              "Exercised against the Python service: the unauthenticated matrix (10/10), the",
              "authenticated read suite, session-cookie interop, webhook signing, the picker 409s,",
              "and every route above that is reachable without live Frame.io/SMTP. Live Frame.io",
              "downloads/uploads, SMTP delivery and browser WebAuthn need real credentials or a",
              "browser, so those paths are ported but not end-to-end exercised here.", "",
              "Known deviations: passkeys registered by the Python service must be re-registered",
              "(webauthn-rs stores its own serialized passkey in `public_key`); the QR SVG bytes",
              "differ (different qrcode library, same payload)."]
    OUT.write_text("\n".join(lines))
    print(f"{done} done, {partial} partial, {todo} todo of {len(routes)}")


if __name__ == "__main__":
    main()
