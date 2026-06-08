# Frame.io client wrapper & storage interface

How Portal talks to Frame.io, and the abstraction that keeps the rest of the app from
depending on Frame.io directly.

## Layers

```
routes / upload engine / sync engine
        │  depend on ▼
   StorageBackend (portal.storage.base)          ── backend-agnostic interface
        │  implemented by ▼
   FrameioStorageBackend (portal.storage.frameio_backend)
        │  uses ▼
   FrameioClient (portal.frameio.client)         ── the one Frame.io client module
        │  wraps ▼
   frameio SDK (official V4 Python SDK)  ──auth──  portal.frameio.connection (Step 2 tokens)
```

Everything that touches Frame.io goes through `FrameioClient`. The upload-link and sync
engines depend on `StorageBackend`, not on Frame.io, so phase 2's S3 backends slot in without
rewriting them.

## The SDK

Official package: `frameio` (V4). Async client `AsyncFrameio`, resource namespaces
`accounts`, `workspaces`, `projects`, `folders`, `files`, `webhooks`, …. Auto-generated from
the V4 OpenAPI spec, so it ships typed models (`Account`, `Workspace`, `Project`, `Folder`,
`File`) and typed errors.

We do **not** let the SDK manage OAuth. Portal already owns the token lifecycle (Step 2,
Adobe IMS authorization-code flow with refresh). The SDK constructor accepts
`async_token: Callable[[], Awaitable[str]]`, which it awaits **on every request** to build the
`Authorization` header. Portal passes a callback that loads the primary connection and returns
a valid access token, refreshing it first if it is near expiry. One source of truth for tokens.

## FrameioClient (`portal.frameio.client`)

- **Auth-aware** — `_provide_token()` → `connection.get_valid_access_token()`; refresh on
  expiry happens transparently. Raises `FrameioNotConnected` if there is no connection.
- **Retry / rate-limit-aware** — the SDK retries with backoff and honors `429` `Retry-After`;
  we pass `max_retries` (`MAX_RETRIES = 4`). Every response's `x-ratelimit-remaining` is logged.
- **Logged** — an httpx response event hook emits `frameio.api.call` for every underlying HTTP
  request with method, url, status, duration_ms, and rate-limit-remaining (NFR: log every
  Frame.io call). The hook reads headers only, so it is safe for future streaming downloads.
- **One transport per worker** — `get_frameio_client()` returns a process-wide singleton
  holding a single `httpx.AsyncClient`; `close_frameio_client()` is called on app shutdown.
- **Errors** — SDK exceptions are translated to `FrameioError` (or `FrameioNotConnected`);
  Frame.io internals never leak upward.

### Pickers

Account-structure navigation used to configure destinations (Step 4):

| Method | SDK call | Returns |
|---|---|---|
| `list_accounts()` | `accounts.index()` | `PickerItem(id, name)` |
| `list_workspaces(account_id)` | `workspaces.index(account_id)` | `PickerItem` |
| `list_projects(account_id, workspace_id)` | `projects.index(...)` | `ProjectItem(id, name, root_folder_id)` |
| `list_subfolders(account_id, folder_id)` | `folders.list(...)` | `PickerItem` |
| `list_files(account_id, folder_id)` | `folders.index(..., type="file")` | `FileItem(id, name, file_size, media_type)` |

Pagination is cursor-based: responses carry `{data, links.next, total_count}`; `links.next`
feeds the `after` parameter. Pager-returning calls (`*.index`) are iterated with `async for`;
all calls are capped at `PICKER_MAX_ITEMS` as a safety stop. To browse a project's tree, take
`ProjectItem.root_folder_id` and call `list_subfolders` from there.

HTTP surface (admin-only, under `/api/frameio`): `GET /accounts`, `GET /workspaces?account_id=`,
`GET /projects?account_id=&workspace_id=`, `GET /folders?account_id=&folder_id=`. Not
connected → `409`; upstream failure → `502`.

## StorageBackend (`portal.storage.base`)

Backend-agnostic interface. Phase-2 method set:

| Method | Phase 1 (Frame.io) | Implemented in |
|---|---|---|
| `list_files_in_destination` | folder children (files) | **Step 3 (now)** |
| `create_upload_session` / `get_upload_credentials` / `complete_upload` | Frame.io upload URLs | Step 6 |
| `get_download_url` | Frame.io download URL | Step 9 |
| `subscribe_to_changes` | Frame.io webhook | Step 8 |
| `delete_object` | delete file | Phase 2 (transit-and-delete) |

Value objects (`DestinationConfig`, `RemoteFile`, `UploadSession`, `UploadCredentials`,
`DownloadURL`, `ChangeSubscription`) are the seam: they carry whatever a backend needs without
leaking backend types upward. `DestinationConfig` mirrors the type-tagged `destinations.config`
JSONB (`{"type": "frameio", "account_id", "workspace_id", "project_id", "folder_id"}`).

Bytes never flow through Portal: uploads use backend-issued credentials (Frame.io upload URLs
now, S3 presigned URLs later), downloads use backend-issued URLs.

`FrameioStorageBackend` implements `list_files_in_destination` today; the upload/download/
subscribe/delete methods raise `NotImplementedError` pointing at the build step that defines
them (those require reading the live Frame.io docs at that step, per the briefing).

## Phase 2 note

A second backend (e.g. `S3StorageBackend`) implements the same interface: presigned multipart
URLs for `get_upload_credentials`, S3 event notifications or polling for `subscribe_to_changes`,
presigned GET for `get_download_url`. No change to the upload or sync engines. See
`FUTURE_BACKENDS.md` (Step 13).
