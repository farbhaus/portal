"""Portal's wrapper around the official `frameio` V4 Python SDK.

All Frame.io API access flows through here (briefing: "one Frame.io client module"). The
wrapper is:

- auth-aware  — supplies a fresh access token per request via `async_token`, so Step 2's
  refresh-on-expiry logic stays the single source of truth (the SDK awaits the callback on
  every request: frameio.core.client_wrapper).
- retry/rate-limit-aware — the SDK retries with backoff and honors 429 Retry-After; we set
  `max_retries` and log `x-ratelimit-remaining` from every response.
- logged — an httpx response event hook logs method, url, status, duration and rate-limit
  headers for every underlying API call (NFR: log every Frame.io call).

Higher-level features (uploads, downloads, webhooks) build on this via the StorageBackend in
`portal.storage`. This module additionally exposes the account-structure *pickers*
(accounts / workspaces / projects / folders) used to configure destinations.
"""

from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass

import httpx
from frameio import (
    AsyncFrameio,
    FileCreateLocalUploadParamsData,
    FolderCreateParamsData,
    WebhookCreateParamsData,
)

from portal.db.session import get_sessionmaker
from portal.frameio import connection as conn_svc
from portal.frameio.oauth import FrameioOAuthError
from portal.lib.logging import get_logger

log = get_logger("frameio.client")

# Total attempts the SDK makes per call before giving up (covers transient 429/5xx).
MAX_RETRIES = 4
_REQ_OPTS = {"max_retries": MAX_RETRIES}
PICKER_PAGE_SIZE = 100
# Safety cap so a pathological account can't make a picker call iterate forever.
PICKER_MAX_ITEMS = 1000
_HTTP_TIMEOUT = 30.0


class FrameioError(Exception):
    """A Frame.io API call failed."""


class FrameioNotConnected(FrameioError):
    """No usable Frame.io connection (not connected, or token refresh failed)."""


class FrameioRateLimited(FrameioError):
    """Frame.io returned 429 (and the SDK's retries were exhausted). Transient — retry later."""


class FrameioNotFound(FrameioError):
    """Frame.io returned 404 — the entity no longer exists. Terminal, not worth retrying."""


class FrameioForbidden(FrameioError):
    """Frame.io returned 403 — typically the original isn't finalized yet. Transient; wait."""


@dataclass(frozen=True)
class PickerItem:
    id: str
    name: str


@dataclass(frozen=True)
class ProjectItem:
    id: str
    name: str
    # Entry point for the folder picker — browse a project's tree from here.
    root_folder_id: str | None


@dataclass(frozen=True)
class FileItem:
    id: str
    name: str
    file_size: int | None
    media_type: str | None


@dataclass(frozen=True)
class ChunkUrl:
    size: int
    url: str


@dataclass(frozen=True)
class UploadTarget:
    file_id: str
    media_type: str | None
    chunks: list[ChunkUrl]


@dataclass(frozen=True)
class UploadStatus:
    complete: bool
    failed: bool


@dataclass(frozen=True)
class DownloadFile:
    id: str
    name: str
    file_size: int | None
    media_type: str | None
    download_url: str | None
    thumbnail_url: str | None


@dataclass(frozen=True)
class FileDetail:
    """Metadata the sync worker needs: name + size to download, parent/project to place & filter."""

    id: str
    name: str
    file_size: int | None
    parent_id: str | None
    project_id: str | None
    status: str | None


@dataclass(frozen=True)
class WebhookSubscription:
    id: str
    secret: str


async def _provide_token() -> str:
    """Return a valid bearer token for the primary connection, refreshing if needed."""
    async with get_sessionmaker()() as db:
        conn = await conn_svc.get_primary_connection(db)
        if conn is None:
            raise FrameioNotConnected("Frame.io is not connected")
        try:
            return await conn_svc.get_valid_access_token(db, conn)
        except FrameioOAuthError as exc:
            raise FrameioNotConnected(str(exc)) from exc


async def _on_request(request: httpx.Request) -> None:
    request.extensions["portal_start"] = time.monotonic()


async def _on_response(response: httpx.Response) -> None:
    start = response.request.extensions.get("portal_start")
    duration_ms = round((time.monotonic() - start) * 1000) if start else None
    log.info(
        "frameio.api.call",
        method=response.request.method,
        url=str(response.request.url),
        status=response.status_code,
        duration_ms=duration_ms,
        ratelimit_remaining=response.headers.get("x-ratelimit-remaining"),
    )


class FrameioClient:
    """Async wrapper holding one SDK client and its httpx transport."""

    def __init__(self) -> None:
        self._httpx = httpx.AsyncClient(
            timeout=_HTTP_TIMEOUT,
            event_hooks={"request": [_on_request], "response": [_on_response]},
        )
        self._client = AsyncFrameio(
            token="",  # unused: async_token supplies the bearer per request
            async_token=_provide_token,
            httpx_client=self._httpx,
        )
        # Cache of (parent_folder_id, child_name) -> child_folder_id so repeated uploads into
        # the same dropped subfolder don't re-list/re-create it. A lock serializes creation to
        # avoid duplicate folders within a worker.
        self._folder_cache: dict[tuple[str, str], str] = {}
        self._folder_lock = asyncio.Lock()

    async def aclose(self) -> None:
        await self._httpx.aclose()

    @property
    def sdk(self) -> AsyncFrameio:
        """Underlying SDK client, for features beyond the pickers (uploads, files, webhooks)."""
        return self._client

    # ── pickers ───────────────────────────────────────────────────────────────

    async def list_accounts(self) -> list[PickerItem]:
        async def run() -> list[PickerItem]:
            pager = await self._client.accounts.index(
                page_size=PICKER_PAGE_SIZE, request_options=_REQ_OPTS
            )
            items: list[PickerItem] = []
            async for acct in pager:
                items.append(PickerItem(id=acct.id, name=acct.display_name or acct.id))
                if len(items) >= PICKER_MAX_ITEMS:
                    break
            return items

        return await self._guard("list_accounts", run)

    async def list_workspaces(self, account_id: str) -> list[PickerItem]:
        async def run() -> list[PickerItem]:
            pager = await self._client.workspaces.index(
                account_id, page_size=PICKER_PAGE_SIZE, request_options=_REQ_OPTS
            )
            items: list[PickerItem] = []
            async for ws in pager:
                items.append(PickerItem(id=ws.id, name=ws.name or ws.id))
                if len(items) >= PICKER_MAX_ITEMS:
                    break
            return items

        return await self._guard("list_workspaces", run)

    async def list_projects(self, account_id: str, workspace_id: str) -> list[ProjectItem]:
        async def run() -> list[ProjectItem]:
            pager = await self._client.projects.index(
                account_id, workspace_id, page_size=PICKER_PAGE_SIZE, request_options=_REQ_OPTS
            )
            items: list[ProjectItem] = []
            async for proj in pager:
                items.append(
                    ProjectItem(
                        id=proj.id,
                        name=proj.name or proj.id,
                        root_folder_id=getattr(proj, "root_folder_id", None),
                    )
                )
                if len(items) >= PICKER_MAX_ITEMS:
                    break
            return items

        return await self._guard("list_projects", run)

    async def list_subfolders(self, account_id: str, folder_id: str) -> list[PickerItem]:
        """Direct child folders of `folder_id` (start from a project's root_folder_id)."""

        async def run() -> list[PickerItem]:
            items: list[PickerItem] = []
            after: str | None = None
            while True:
                resp = await self._client.folders.list(
                    account_id,
                    folder_id,
                    page_size=PICKER_PAGE_SIZE,
                    after=after,
                    request_options=_REQ_OPTS,
                )
                for folder in resp.data or []:
                    items.append(PickerItem(id=folder.id, name=folder.name or folder.id))
                after = resp.links.next if resp.links else None
                if not after or len(items) >= PICKER_MAX_ITEMS:
                    break
            return items

        return await self._guard("list_subfolders", run)

    async def get_folder(self, account_id: str, folder_id: str) -> PickerItem:
        """Resolve a single folder — used to validate a picked destination and get its name."""

        async def run() -> PickerItem:
            resp = await self._client.folders.show(
                account_id, folder_id, request_options=_REQ_OPTS
            )
            folder = resp.data
            return PickerItem(id=folder.id, name=getattr(folder, "name", None) or folder.id)

        return await self._guard("get_folder", run)

    async def list_files(self, account_id: str, folder_id: str) -> list[FileItem]:
        """Direct child files (not subfolders) of a folder."""

        async def run() -> list[FileItem]:
            items: list[FileItem] = []
            after: str | None = None
            while True:
                resp = await self._client.folders.index(
                    account_id,
                    folder_id,
                    type="file",
                    page_size=PICKER_PAGE_SIZE,
                    after=after,
                    request_options=_REQ_OPTS,
                )
                for asset in resp.data or []:
                    items.append(
                        FileItem(
                            id=asset.id,
                            name=getattr(asset, "name", None) or asset.id,
                            file_size=getattr(asset, "file_size", None),
                            media_type=getattr(asset, "media_type", None),
                        )
                    )
                after = resp.links.next if resp.links else None
                if not after or len(items) >= PICKER_MAX_ITEMS:
                    break
            return items

        return await self._guard("list_files", run)

    # ── uploads (see docs: presigned S3 chunk PUTs) ───────────────────────────────

    async def create_local_upload(
        self, account_id: str, folder_id: str, *, name: str, file_size: int
    ) -> UploadTarget:
        """Create a file placeholder and get presigned S3 chunk-upload URLs."""

        async def run() -> UploadTarget:
            resp = await self._client.files.create_local_upload(
                account_id,
                folder_id,
                data=FileCreateLocalUploadParamsData(name=name, file_size=file_size),
                request_options=_REQ_OPTS,
            )
            f = resp.data
            return UploadTarget(
                file_id=f.id,
                media_type=f.media_type,
                chunks=[ChunkUrl(size=u.size, url=u.url) for u in (f.upload_urls or [])],
            )

        return await self._guard("create_local_upload", run)

    async def get_upload_status(self, account_id: str, file_id: str) -> UploadStatus:
        async def run() -> UploadStatus:
            resp = await self._client.files.show_file_upload_status(
                account_id, file_id, request_options=_REQ_OPTS
            )
            d = resp.data
            return UploadStatus(complete=bool(d.upload_complete), failed=bool(d.upload_failed))

        return await self._guard("get_upload_status", run)

    async def ensure_folder_path(
        self, account_id: str, root_folder_id: str, parts: list[str]
    ) -> str:
        """Resolve/create a nested subfolder path under root, returning the leaf folder id.

        Idempotent: matches existing subfolders by name before creating, so re-runs and dropped
        folder hierarchies don't duplicate. Cached per (parent, name).
        """

        async def run() -> str:
            async with self._folder_lock:
                current = root_folder_id
                for part in parts:
                    key = (current, part)
                    cached = self._folder_cache.get(key)
                    if cached is not None:
                        current = cached
                        continue
                    existing = await self.list_subfolders(account_id, current)
                    match = next((f for f in existing if f.name == part), None)
                    if match is not None:
                        current = match.id
                    else:
                        resp = await self._client.folders.create(
                            account_id,
                            current,
                            data=FolderCreateParamsData(name=part),
                            request_options=_REQ_OPTS,
                        )
                        current = resp.data.id
                    self._folder_cache[key] = current
                return current

        return await self._guard("ensure_folder_path", run)

    # ── downloads (Media Links: short-lived signed S3 URLs) ───────────────────────

    async def get_download_url(self, account_id: str, file_id: str) -> str:
        """Mint a short-lived signed download URL for a file (Media Links 'original')."""

        async def run() -> str:
            resp = await self._client.files.show(
                account_id, file_id, include="media_links.original", request_options=_REQ_OPTS
            )
            links = getattr(resp.data, "media_links", None)
            original = getattr(links, "original", None) if links else None
            url = getattr(original, "download_url", None) if original else None
            if not url:
                raise FrameioError(f"No download URL available for file {file_id}")
            return str(url)

        return await self._guard("get_download_url", run)

    async def get_file(self, account_id: str, file_id: str) -> DownloadFile:
        """File metadata + (best-effort) thumbnail for the recipient listing."""

        async def run() -> DownloadFile:
            resp = await self._client.files.show(
                account_id,
                file_id,
                include="media_links.thumbnail",
                request_options=_REQ_OPTS,
            )
            f = resp.data
            links = getattr(f, "media_links", None)
            thumbnail = getattr(links, "thumbnail", None) if links else None
            thumb = getattr(thumbnail, "download_url", None) if thumbnail else None
            return DownloadFile(
                id=f.id,
                name=getattr(f, "name", None) or f.id,
                file_size=getattr(f, "file_size", None),
                media_type=getattr(f, "media_type", None),
                download_url=None,
                thumbnail_url=thumb,
            )

        return await self._guard("get_file", run)

    async def get_file_detail(self, account_id: str, file_id: str) -> FileDetail:
        """File metadata for the sync worker (no media-link includes — cheaper)."""

        async def run() -> FileDetail:
            resp = await self._client.files.show(account_id, file_id, request_options=_REQ_OPTS)
            f = resp.data
            return FileDetail(
                id=f.id,
                name=getattr(f, "name", None) or f.id,
                file_size=getattr(f, "file_size", None),
                parent_id=getattr(f, "parent_id", None),
                project_id=getattr(f, "project_id", None),
                status=getattr(f, "status", None),
            )

        return await self._guard("get_file_detail", run)

    async def list_files_recursive(self, account_id: str, folder_id: str) -> list[FileItem]:
        """All files under a folder, descending into subfolders (capped)."""

        async def run() -> list[FileItem]:
            collected: list[FileItem] = []
            stack = [folder_id]
            seen: set[str] = set()
            while stack and len(collected) < PICKER_MAX_ITEMS:
                current = stack.pop()
                if current in seen:
                    continue
                seen.add(current)
                collected.extend(await self.list_files(account_id, current))
                subfolders = await self.list_subfolders(account_id, current)
                stack.extend(sf.id for sf in subfolders if sf.id not in seen)
            return collected[:PICKER_MAX_ITEMS]

        return await self._guard("list_files_recursive", run)

    # ── webhooks (workspace-scoped; secret returned once, on create) ──────────────

    async def create_webhook(
        self, account_id: str, workspace_id: str, *, name: str, url: str, events: list[str]
    ) -> WebhookSubscription:
        """Create a workspace webhook. The signing secret is only returned here — store it."""

        async def run() -> WebhookSubscription:
            resp = await self._client.webhooks.create(
                account_id,
                workspace_id,
                data=WebhookCreateParamsData(name=name, url=url, events=events),
                request_options=_REQ_OPTS,
            )
            d = resp.data
            secret = getattr(d, "secret", None)
            if not secret:
                raise FrameioError("Frame.io did not return a webhook signing secret")
            return WebhookSubscription(id=d.id, secret=str(secret))

        return await self._guard("create_webhook", run)

    async def delete_webhook(self, account_id: str, webhook_id: str) -> None:
        async def run() -> None:
            await self._client.webhooks.delete(account_id, webhook_id, request_options=_REQ_OPTS)

        await self._guard("delete_webhook", run)

    # ── internals ───────────────────────────────────────────────────────────────

    async def _guard[T](self, op: str, run: Callable[[], Awaitable[T]]) -> T:
        try:
            return await run()
        except FrameioNotConnected:
            raise
        except Exception as exc:
            status = getattr(exc, "status_code", None)
            log.warning(
                "frameio.client.error",
                op=op,
                error=str(exc),
                error_type=type(exc).__name__,
                status=status,
            )
            # 429s survive the SDK's own retries on the tightly-limited folder endpoints; flag
            # them as transient so callers can tell the user "try again" rather than "failed".
            if status == 429:
                raise FrameioRateLimited(f"Frame.io is rate-limiting: {op}") from exc
            if status == 404:
                raise FrameioNotFound(f"Frame.io entity not found: {op}") from exc
            if status == 403:
                raise FrameioForbidden(f"Frame.io forbidden (not ready?): {op}") from exc
            raise FrameioError(f"Frame.io API call failed: {op}") from exc


_singleton: FrameioClient | None = None


def get_frameio_client() -> FrameioClient:
    """Process-wide shared client (one httpx transport per worker)."""
    global _singleton
    if _singleton is None:
        _singleton = FrameioClient()
    return _singleton


async def close_frameio_client() -> None:
    global _singleton
    if _singleton is not None:
        await _singleton.aclose()
        _singleton = None
