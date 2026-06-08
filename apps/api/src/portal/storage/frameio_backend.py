"""Frame.io-backed StorageBackend (phase 1's only backend).

Read operations are implemented here against the Frame.io client wrapper. The upload, download,
delete and subscribe operations are defined by the interface but land in their dedicated build
steps — they require decisions (upload mechanism, media-link/download semantics, webhook
subscription) that the briefing says to make by reading the live docs in those steps, not now:

    create/get_credentials/complete  → step 6 (upload mechanism)
    get_download_url                 → step 9 (sync engine)
    delete_object                    → phase 2 (transit-and-delete lifecycle)
    subscribe_to_changes             → step 8 (webhooks)
"""

from __future__ import annotations

from typing import ClassVar

from portal.frameio.client import FrameioClient, get_frameio_client
from portal.storage.base import (
    ChangeSubscription,
    DestinationConfig,
    DownloadURL,
    RemoteFile,
    StorageBackend,
    UploadCredentials,
    UploadSession,
)


class FrameioStorageBackend(StorageBackend):
    backend_type: ClassVar[str] = "frameio"

    def __init__(self, client: FrameioClient | None = None) -> None:
        self._client = client or get_frameio_client()

    async def list_files_in_destination(self, destination: DestinationConfig) -> list[RemoteFile]:
        account_id = destination.require("account_id")
        folder_id = destination.require("folder_id")
        files = await self._client.list_files(account_id, folder_id)
        return [
            RemoteFile(id=f.id, name=f.name, size=f.file_size, media_type=f.media_type)
            for f in files
        ]

    async def create_upload_session(
        self,
        destination: DestinationConfig,
        *,
        filename: str,
        size: int,
        media_type: str | None = None,
    ) -> UploadSession:
        raise NotImplementedError("Upload mechanism is implemented in build step 6")

    async def get_upload_credentials(self, session: UploadSession) -> UploadCredentials:
        raise NotImplementedError("Upload mechanism is implemented in build step 6")

    async def complete_upload(self, session: UploadSession) -> RemoteFile:
        raise NotImplementedError("Upload mechanism is implemented in build step 6")

    async def get_download_url(
        self, destination: DestinationConfig, file_id: str
    ) -> DownloadURL:
        raise NotImplementedError("Download URLs are implemented in build step 9 (sync engine)")

    async def delete_object(self, destination: DestinationConfig, file_id: str) -> None:
        raise NotImplementedError("Deletion lands with the phase 2 transit-and-delete lifecycle")

    async def subscribe_to_changes(
        self, destination: DestinationConfig, *, callback_url: str
    ) -> ChangeSubscription:
        raise NotImplementedError("Webhook subscriptions are implemented in build step 8")
