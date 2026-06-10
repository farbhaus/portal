"""Backend-agnostic storage interface.

The upload-link and sync engines depend on `StorageBackend`, never directly on Frame.io, so
phase 2's S3-compatible backends are an additive change (briefing: "storage abstraction from
day one"). The method set is the one required for phase 2:

    create_upload_session / get_upload_credentials / complete_upload
    list_files_in_destination / get_download_url / delete_object
    subscribe_to_changes

Bytes never flow through Portal: uploads use backend-issued credentials (Frame.io upload URLs
now, S3 presigned URLs later); downloads use backend-issued URLs. These value objects are the
seam — they carry whatever the backend needs without leaking backend types upward.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, ClassVar


@dataclass(frozen=True)
class DestinationConfig:
    """Type-tagged destination config as stored on `destinations.config` (JSONB).

    Frame.io shape: {"type": "frameio", "account_id", "workspace_id", "project_id",
    "folder_id"}. Phase 2 S3 shape: {"type": "s3", "bucket", "prefix", ...}.
    """

    type: str
    data: dict[str, Any]

    def require(self, key: str) -> str:
        value = self.data.get(key)
        if not isinstance(value, str) or not value:
            raise KeyError(f"destination config missing '{key}' for type '{self.type}'")
        return value


@dataclass(frozen=True)
class RemoteFile:
    id: str
    name: str
    size: int | None = None
    media_type: str | None = None
    created_at: datetime | None = None
    # Where the file sits in the backend's tree — the sync engine uses these to filter events
    # to a rule's folder and to template the local path.
    parent_id: str | None = None
    project_id: str | None = None


@dataclass(frozen=True)
class UploadSession:
    """A backend-side handle for one file upload, created before bytes are sent."""

    id: str
    destination: DestinationConfig
    filename: str
    size: int
    backend_data: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class UploadChunk:
    """One direct-to-storage PUT target. `part` is 1-based; the client slices the file into
    consecutive `size`-byte ranges in order."""

    part: int
    size: int
    url: str


@dataclass(frozen=True)
class UploadCredentials:
    """How the uploader's browser sends bytes directly to the backend.

    `chunks` are the ordered PUT targets (Frame.io presigned S3 URLs now, S3 presigned
    multipart part URLs later); `headers` the request headers each PUT must send verbatim;
    `expires_at` when they go stale.
    """

    upload_session_id: str
    chunks: list[UploadChunk]
    headers: dict[str, str] = field(default_factory=dict)
    expires_at: datetime | None = None


class UploadNotReady(Exception):
    """complete_upload() called before the backend finished assembling the file."""


@dataclass(frozen=True)
class DownloadURL:
    url: str
    expires_at: datetime | None = None


@dataclass(frozen=True)
class ChangeSubscription:
    """A registered way to learn about new files (Frame.io webhook; S3 events/polling later)."""

    id: str
    backend_data: dict[str, Any] = field(default_factory=dict)


class StorageBackend(ABC):
    backend_type: ClassVar[str]

    @abstractmethod
    async def list_files_in_destination(self, destination: DestinationConfig) -> list[RemoteFile]:
        """List files directly in a destination (not recursive)."""

    @abstractmethod
    async def list_files_recursive(self, destination: DestinationConfig) -> list[RemoteFile]:
        """List all files under a destination, descending into subfolders (reconciliation)."""

    @abstractmethod
    async def get_file(self, destination: DestinationConfig, file_id: str) -> RemoteFile:
        """Fetch one file's metadata (name, size, parent, project)."""

    @abstractmethod
    async def create_upload_session(
        self,
        destination: DestinationConfig,
        *,
        filename: str,
        size: int,
        media_type: str | None = None,
    ) -> UploadSession:
        """Reserve a place for an incoming file and return a handle."""

    @abstractmethod
    async def get_upload_credentials(self, session: UploadSession) -> UploadCredentials:
        """Return direct-to-backend upload targets for a session (no bytes via Portal)."""

    @abstractmethod
    async def complete_upload(self, session: UploadSession) -> RemoteFile:
        """Finalize an upload once the uploader has sent all bytes."""

    @abstractmethod
    async def get_download_url(
        self, destination: DestinationConfig, file_id: str
    ) -> DownloadURL:
        """A direct download URL for a file (sync workers pull from here, not via Portal)."""

    @abstractmethod
    async def delete_object(self, destination: DestinationConfig, file_id: str) -> None:
        """Delete a file from the backend."""

    @abstractmethod
    async def subscribe_to_changes(
        self, destination: DestinationConfig, *, callback_url: str
    ) -> ChangeSubscription:
        """Register for new-file notifications (Frame.io webhook now; S3 events/polling later)."""

    @abstractmethod
    async def unsubscribe_from_changes(
        self, destination: DestinationConfig, subscription_id: str
    ) -> None:
        """Tear down a change subscription created by ``subscribe_to_changes``."""
