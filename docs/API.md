# Portal API

Machine-readable spec: **`GET /api/openapi.yaml`** (OpenAPI 3.0). Import it into Swagger Editor,
Postman, Insomnia, or any OpenAPI tool. The same file is at `apps/api-rs/openapi.yaml` in the repo.

An MCP server that wraps this API is in [`../mcp/`](../mcp/README.md).

Portal's admin API is the same JSON API the web app uses, under `/api`. Two ways to authenticate:

- **Session cookie** — log in with `POST /api/auth/login`, keep the `portal_session` cookie.
- **Bearer token** — set `PORTAL_API_TOKEN` on the server (at least 32 characters; Portal refuses
  to start with a shorter one) and send `Authorization: Bearer <token>`. The token acts as the
  active admin on the admin endpoints, with these limits:
  - It can't change how the admin signs in: `POST /api/settings/password`,
    `/api/settings/totp/*` and passkey registration/deletion answer `403` and need a browser login.
  - Every write it makes (anything but `GET`/`HEAD`) is recorded in the admin log as
    `api_token.used`, with the method and path.
  - Wrong tokens are counted per client IP; after `PASSWORD_LOCKOUT_MAX_FAILURES` misses in
    `PASSWORD_LOCKOUT_WINDOW_SECONDS` that IP gets `429` until the window passes.
  - A request that also carries a valid session cookie is let through on the session even if its
    bearer token is wrong, so a proxy that adds its own `Authorization` header doesn't lock the
    web app out. With `PORTAL_API_TOKEN` unset, bearer headers are ignored.

  Leave the variable unset to disable token auth.

All request and response bodies are JSON. Errors look like `{"detail":"..."}` with a 4xx/5xx
status. `GET /api/health` is public.

## Upload links

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/upload-links` | List upload links |
| `POST` | `/api/upload-links` | Create an upload link |
| `GET` | `/api/upload-links/{id}` | Read one |
| `PATCH` | `/api/upload-links/{id}` | Update fields (only keys present are applied) |
| `DELETE` | `/api/upload-links/{id}` | Delete |
| `POST` | `/api/upload-links/{id}/revoke` | Stop accepting uploads |
| `POST` | `/api/upload-links/{id}/restore` | Undo a revoke |

Create an upload link:

```bash
curl -X POST https://portal.example.com/api/upload-links \
  -H "Authorization: Bearer $PORTAL_API_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "ACME Spring Spot",
    "folder": {
      "type": "frameio",
      "account_id": "…",
      "workspace_id": "…",
      "project_id": "…",
      "folder_id": "…"
    },
    "turbosort": "20261028_ACME-SpringSpot"
  }'
```

The response includes `public_url` (the client-facing link) and `turbosort`.

`turbosort` is optional. When set, it names the project's local folder (one folder name: no `/`,
`\`, control characters, `.` or `..`); FioTransfer reads it from `GET /api/upload-links`. Portal
writes nothing into Frame.io for it. Sending `null` (or `""`) clears the field.

### FioTransfer sync (`folder.sync`)

An upload link can carry a sync block inside its `folder`. Portal only stores and returns it — the
FioTransfer client reads it from `GET /api/upload-links` and moves the files:

```json
"folder": {
  "type": "frameio", "account_id": "…", "folder_id": "…",
  "sync": {
    "mode": "add_only",
    "drive":   { "local": "1_DRIVE",   "folder_id": "<Frame.io folder the client's drive comes from>" },
    "exports": { "local": "0_EXPORTS", "folder_id": "<Frame.io folder exports go up to>" }
  }
}
```

- `drive` downloads that Frame.io folder into the local folder `local`; `exports` uploads the local
  folder `local` into that Frame.io folder. Either lane may be left out, but not both.
- `folder_id` is optional and means "the link's own folder"; `local` is one folder name, and the
  two lanes' `local` names must differ.
- `add_only` is the only mode: files are added in both directions and nothing is ever deleted.
- On `PATCH`, a `folder` without a `sync` key keeps the stored block; `"sync": null` removes it.

Not to be confused with the top-level `sync` array in `GET /api/links`, which lists the Portal
sync rules (Frame.io → NAS) that already cover a link's folder.

## Other resources

The same bearer token works on the rest of the admin API, including:

- `/api/links`, `/api/download-links`
- `/api/sync-rules` (Frame.io → NAS sync rules)
- `/api/frameio/accounts`, `/api/frameio/workspaces`, `/api/frameio/projects`,
  `/api/frameio/folders`, `/api/frameio/files`
- `/api/activity`, `/api/dashboard`, `/api/settings`

The web app's `/api` calls are the reference for field names and response shapes.
