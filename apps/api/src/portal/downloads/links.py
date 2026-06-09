"""Download-link helpers: source resolution + branding, kept off FastAPI for easy testing.

A download link points at a Frame.io source — a single file, a folder (optionally recursive),
or a curated selection of files — all under one account. Resolving a source yields the concrete
list of downloadable files (name, size, type, thumbnail). Token generation and link state
(ok/expired/revoked) are shared with upload links.
"""

from dataclasses import dataclass
from typing import Any

from portal.db.models import DownloadLink
from portal.frameio.client import DownloadFile, FrameioClient


@dataclass(frozen=True)
class ResolvedSource:
    account_id: str
    files: list[DownloadFile]


def validate_source(source: dict[str, Any]) -> None:
    """Ensure a source dict is well-formed; raises ValueError otherwise."""
    account_id = source.get("account_id")
    if not isinstance(account_id, str) or not account_id:
        raise ValueError("source.account_id is required")
    stype = source.get("type")
    if stype == "file":
        if not source.get("file_id"):
            raise ValueError("file source requires file_id")
    elif stype == "folder":
        if not source.get("folder_id"):
            raise ValueError("folder source requires folder_id")
    elif stype == "selection":
        ids = source.get("file_ids")
        if not isinstance(ids, list) or not ids:
            raise ValueError("selection source requires a non-empty file_ids list")
    else:
        raise ValueError(f"unknown source type: {stype!r}")


async def resolve_source(client: FrameioClient, source: dict[str, Any]) -> ResolvedSource:
    """Resolve a link's source into concrete downloadable files (metadata + thumbnails)."""
    account_id = str(source["account_id"])
    stype = source.get("type")

    if stype == "file":
        files = [await client.get_file(account_id, str(source["file_id"]))]
    elif stype == "selection":
        files = [await client.get_file(account_id, str(fid)) for fid in source["file_ids"]]
    elif stype == "folder":
        folder_id = str(source["folder_id"])
        listing = (
            await client.list_files_recursive(account_id, folder_id)
            if source.get("recursive")
            else await client.list_files(account_id, folder_id)
        )
        # Map folder listings directly (one call) instead of a get_file per item — fetching a
        # thumbnail per file would be N calls against the rate-limited folders endpoint. Folder
        # sources therefore show no thumbnails; file/selection sources (small N) get full metadata.
        files = [
            DownloadFile(
                id=item.id,
                name=item.name,
                file_size=item.file_size,
                media_type=item.media_type,
                download_url=None,
                thumbnail_url=None,
            )
            for item in listing
        ]
    else:
        raise ValueError(f"unknown source type: {stype!r}")

    return ResolvedSource(account_id=account_id, files=files)


def file_in_source(resolved: ResolvedSource, file_id: str) -> bool:
    """Guard: a recipient may only download files that belong to the link's source."""
    return any(f.id == file_id for f in resolved.files)


def branding(link: DownloadLink) -> dict[str, str | None]:
    """Self-contained branding for a download link (no destination fallback in v1)."""
    return {
        "display_name": link.brand_display_name or "Download",
        "subtitle": link.brand_subtitle,
        "logo_url": link.brand_logo_url,
        "accent_color": link.brand_accent_color,
    }
