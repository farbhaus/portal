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

from portal.frameio.client import FrameioClient, FrameioError, get_frameio_client
from portal.storage.base import (
    ChangeSubscription,
    DestinationConfig,
    DownloadURL,
    RemoteFile,
    StorageBackend,
    UploadChunk,
    UploadCredentials,
    UploadNotReady,
    UploadSession,
)


def _split_path(path: str) -> tuple[list[str], str]:
    """Split a (possibly nested) upload path into (dir parts, filename), dropping unsafe parts."""
    parts = [p for p in path.replace("\\", "/").split("/") if p and p not in (".", "..")]
    if not parts:
        raise ValueError("empty upload path")
    return parts[:-1], parts[-1]


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
        """Create a Frame.io file (in a path-mirrored subfolder) and stash its chunk URLs.

        `filename` may include a relative path; subfolders are created to mirror it. The
        returned session's backend_data carries the file id, media_type and presigned chunk
        URLs that get_upload_credentials hands to the browser.
        """
        account_id = destination.require("account_id")
        root_folder_id = destination.require("folder_id")
        dirs, name = _split_path(filename)
        folder_id = (
            await self._client.ensure_folder_path(account_id, root_folder_id, dirs)
            if dirs
            else root_folder_id
        )
        target = await self._client.create_local_upload(
            account_id, folder_id, name=name, file_size=size
        )
        return UploadSession(
            id=target.file_id,
            destination=destination,
            filename=name,
            size=size,
            backend_data={
                "file_id": target.file_id,
                "media_type": target.media_type,
                "folder_id": folder_id,
                "chunks": [{"size": c.size, "url": c.url} for c in target.chunks],
            },
        )

    async def get_upload_credentials(self, session: UploadSession) -> UploadCredentials:
        chunks = session.backend_data.get("chunks", [])
        media_type = session.backend_data.get("media_type")
        # These two headers are exactly what Frame.io's presigned S3 PUT signs; both required.
        headers = {"x-amz-acl": "private"}
        if media_type:
            headers["Content-Type"] = str(media_type)
        return UploadCredentials(
            upload_session_id=session.id,
            chunks=[
                UploadChunk(part=i + 1, size=int(c["size"]), url=str(c["url"]))
                for i, c in enumerate(chunks)
            ],
            headers=headers,
        )

    async def complete_upload(self, session: UploadSession) -> RemoteFile:
        """Return the finalized file once Frame.io has assembled it; else raise UploadNotReady."""
        account_id = session.destination.require("account_id")
        file_id = str(session.backend_data["file_id"])
        status = await self._client.get_upload_status(account_id, file_id)
        if status.failed:
            raise FrameioError(f"Upload failed for file {file_id}")
        if not status.complete:
            raise UploadNotReady(file_id)
        return RemoteFile(id=file_id, name=session.filename, size=session.size)

    async def get_download_url(
        self, destination: DestinationConfig, file_id: str
    ) -> DownloadURL:
        account_id = destination.require("account_id")
        url = await self._client.get_download_url(account_id, file_id)
        # Frame.io's Media Links are short-lived; we don't get an explicit expiry back.
        return DownloadURL(url=url, expires_at=None)

    async def delete_object(self, destination: DestinationConfig, file_id: str) -> None:
        raise NotImplementedError("Deletion lands with the phase 2 transit-and-delete lifecycle")

    async def subscribe_to_changes(
        self, destination: DestinationConfig, *, callback_url: str
    ) -> ChangeSubscription:
        raise NotImplementedError("Webhook subscriptions are implemented in build step 8")
