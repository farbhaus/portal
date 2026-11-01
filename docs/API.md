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
      "folder_id": "…",
      "sync": {}
    }
  }'
```

The response includes `public_url` (the client-facing link).

### Sync to FioTransfer (`folder.sync`)

An upload link can carry a `sync` block inside its `folder`. Its presence is the "Sync to
FioTransfer" switch, and `{}` is enough. Portal only stores and returns it: the FioTransfer client
reads `GET /api/upload-links` and gives each synced link a folder on the user's disk. What the
client uploads comes down into its `IN` folder; what the user puts in `OUT` goes up to the return
folder.

```json
"folder": {
  "type": "frameio", "account_id": "…", "folder_id": "…",
  "sync": {
    "upload": { "folder_id": "<return folder: OUT goes up here>" }
  }
}
```

| Field | Meaning | If absent |
|---|---|---|
| `sync` | The link is synced | Not synced |
| `sync.upload.folder_id` | The return folder `OUT` goes up to | No `OUT` |
| `sync.download.folder_id` | Where `IN` comes from | The link's own folder |
| `sync.name` | The project folder's name on disk | The link's name |
| `sync.download.local`, `sync.upload.local` | Per-link names for `IN` / `OUT` | The names set in FioTransfer |

- Names are one folder name of at most 255 bytes: no `/`, `\`, `%`, control characters, `.` or `..`.
- The return folder must differ from the folder `IN` comes from, and the two `local` names must
  differ when both are set.
- Files are only ever added, on either side; nothing is deleted.
- On `PATCH`, a `folder` without a `sync` key keeps the stored block; `"sync": null` removes it.

Older links may carry the earlier lanes, which are still accepted and stored as sent: `drive`
(read as `download`), `exports` (read as `upload`) and `mode` (only `add_only`). A block may name
a direction once, under either key. The link-level `turbosort` field is the older project folder
name, read when `sync.name` is absent; the admin UI no longer sets it.

## Other resources

The same bearer token works on the rest of the admin API, including:

- `/api/links`, `/api/download-links`
- `/api/frameio/accounts`, `/api/frameio/workspaces`, `/api/frameio/projects`,
  `/api/frameio/folders`, `/api/frameio/files`
- `/api/activity`, `/api/dashboard`, `/api/settings`

The web app's `/api` calls are the reference for field names and response shapes.
