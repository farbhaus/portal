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
      "sync": { "drive": {}, "exports": { "folder_id": "…" } }
    },
    "turbosort": "ACME_SPRING_SPOT"
  }'
```

The response includes `public_url` (the client-facing link).

### Sync with FioTransfer (`folder.sync`, `turbosort`)

"Sync this project's folders with FioTransfer". Portal only stores and returns it; the FioTransfer
client reads `GET /api/upload-links` and keeps a project folder on the user's machine,
`<root>/<turbosort>`, holding up to two one-way folders:

- **Imports** (`drive`): what the client sends comes down into `<turbosort>/<drive.local>`.
- **Exports** (`exports`): what the user puts in `<turbosort>/<exports.local>` goes up to
  `exports.folder_id`.

```json
"turbosort": "ACME_SPOT",
"folder": {
  "type": "frameio", "account_id": "…", "folder_id": "…", "turbosort": "ACME_SPOT",
  "sync": {
    "mode": "add_only",
    "drive":   { "local": "1_DRIVE" },
    "exports": { "local": "0_EXPORTS", "folder_id": "<the Frame.io folder Exports go up to>" }
  }
}
```

| Field | Meaning | If absent |
|---|---|---|
| `sync` | The link is synced | Not synced |
| `turbosort` (link field) | The project folder on the user's machine | The job folder the Imports sit in (see below) |
| `sync.drive` | Imports are on | Nothing comes down |
| `sync.drive.folder_id` | Where Imports come down from | The link's own folder |
| `sync.exports` | Exports are on | Nothing goes up |
| `sync.exports.folder_id` | Where Exports go up to | Required when `exports` is sent |
| `sync.drive.local`, `sync.exports.local` | The two folders' names on disk | The Imports/Exports names from Settings |
| `sync.mode` | Files are only ever added, never deleted | Always stored as `add_only` |

- Names are one folder name of at most 255 bytes: no `/`, `\`, `%`, control characters, `.` or
  `..`. The two `local` names must differ (ignoring case).
- **Settings → FioTransfer** sets the Imports and Exports names a lane gets when it doesn't name
  its own (`GET`/`POST /api/settings/fiotransfer`, `{imports_folder, exports_folder}`; default
  `1_DRIVE` and `0_EXPORTS`). Links keep the names they were saved with.
- A synced link always has a `turbosort`. Without one, Portal names it after the job folder the
  Imports sit in: the folder above the link's folder when that is named like the Imports setting
  (`ACME / 1_DRIVE` → `ACME`), else the first folder below the project, else the project.
- The Exports folder can't be the folder Imports come down from or one inside it: FioTransfer
  brings that folder down whole, so what goes up would come straight back down.
- On `PATCH`, a `folder` without a `sync` key keeps the stored block; `"sync": null` removes it
  and clears `turbosort` too, unless the same request sets one. A `turbosort` without a `sync` is
  one folder FioTransfer mirrors both ways (the oldest kind of link).
- With an Exports folder, Portal keeps a Send link on it (the folder and its subfolders) for you to
  share with the client: the upload link's `return_link` (`{id, public_url}`). Its settings follow
  the upload link — name, expiry, password, email code, the fields it asks for, page title and
  notification recipients — as do revoke and restore, and it goes when the upload link is deleted.
  Without an Exports folder it is revoked, keeping its URL for when one is set again. On the Send
  link, `return_for` names its upload link. `send_link: false` on the upload link turns it off;
  deleting the Send link does that too, so it doesn't come back.

Portal 2.2 and 2.3 wrote this block as `{}`, `download`, `upload` and `name`. Those are still
accepted and stored in the shape above: `download` as `drive`, `upload` as `exports`, `name` as
the link's `turbosort`, and `{}` (no lane named) as Imports with its defaults. A lane may be sent
under one of its two names only. Migration `d6e7f8a9b0c1` rewrote links saved that way.

## Other resources

The same bearer token works on the rest of the admin API, including:

- `/api/links`, `/api/download-links`
- `/api/frameio/accounts`, `/api/frameio/workspaces`, `/api/frameio/projects`,
  `/api/frameio/folders`, `/api/frameio/files`
- `/api/activity`, `/api/dashboard`, `/api/settings`

The web app's `/api` calls are the reference for field names and response shapes.
