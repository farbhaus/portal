"""Parallel byte-range download of a Frame.io file to a local path.

The sync worker is the one place bytes legitimately flow through Portal — it pulls the original
from Frame.io's signed storage URL and writes it to a local destination (NAS/disk). To keep memory
flat regardless of file size (NFR: RSS must not scale with file size), each range response is
streamed to disk with positional writes (``os.pwrite``); nothing buffers a whole chunk, let alone
the whole file. The SHA-256 is computed by re-reading the finished file.

The signed URL points at S3/CDN storage (not the Frame.io API), so we use a plain httpx client
with no auth hook, and rely on HTTP Range support for parallelism.
"""

from __future__ import annotations

import asyncio
import hashlib
import os
from dataclasses import dataclass
from pathlib import Path

import httpx

CHUNK_SIZE = 16 * 1024 * 1024  # byte-range size per parallel request
STREAM_BUFFER = 1024 * 1024  # read granularity while streaming a range to disk
DEFAULT_CONCURRENCY = 4
_READ_TIMEOUT = httpx.Timeout(60.0, connect=30.0, read=300.0)


@dataclass(frozen=True)
class DownloadResult:
    bytes_written: int
    sha256: str


def _ranges(size: int, chunk: int) -> list[tuple[int, int]]:
    return [(start, min(start + chunk, size) - 1) for start in range(0, size, chunk)]


def _sha256_of(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as fh:
        while data := fh.read(STREAM_BUFFER):
            digest.update(data)
            total += len(data)
    return total, digest.hexdigest()


async def download_to_file(
    url: str,
    size: int | None,
    tmp_path: Path,
    *,
    concurrency: int = DEFAULT_CONCURRENCY,
    chunk_size: int = CHUNK_SIZE,
    client: httpx.AsyncClient | None = None,
) -> DownloadResult:
    """Download ``url`` to ``tmp_path``; return bytes written and SHA-256.

    Uses parallel ranged GETs when ``size`` is known and large enough; otherwise a single
    streaming GET. The caller owns placing/renaming the finished temp file.
    """
    own_client = client is None
    http = client or httpx.AsyncClient(timeout=_READ_TIMEOUT, follow_redirects=True)
    try:
        tmp_path.parent.mkdir(parents=True, exist_ok=True)
        if size is None or size <= chunk_size:
            await _download_single(http, url, tmp_path)
        else:
            await _download_ranged(http, url, size, tmp_path, concurrency, chunk_size)
    finally:
        if own_client:
            await http.aclose()

    total, sha = await asyncio.to_thread(_sha256_of, tmp_path)
    if size is not None and total != size:
        raise OSError(f"size mismatch: expected {size}, wrote {total}")
    return DownloadResult(bytes_written=total, sha256=sha)


async def _download_single(http: httpx.AsyncClient, url: str, tmp_path: Path) -> None:
    flags = os.O_CREAT | os.O_WRONLY | os.O_TRUNC
    fd = await asyncio.to_thread(os.open, str(tmp_path), flags, 0o644)
    try:
        offset = 0
        async with http.stream("GET", url) as resp:
            resp.raise_for_status()
            async for data in resp.aiter_bytes(STREAM_BUFFER):
                await asyncio.to_thread(os.pwrite, fd, data, offset)
                offset += len(data)
    finally:
        await asyncio.to_thread(os.close, fd)


async def _download_ranged(
    http: httpx.AsyncClient,
    url: str,
    size: int,
    tmp_path: Path,
    concurrency: int,
    chunk_size: int,
) -> None:
    flags = os.O_CREAT | os.O_WRONLY | os.O_TRUNC
    fd = await asyncio.to_thread(os.open, str(tmp_path), flags, 0o644)
    try:
        await asyncio.to_thread(os.ftruncate, fd, size)
        sem = asyncio.Semaphore(concurrency)

        async def fetch(start: int, end: int) -> None:
            async with sem:
                headers = {"Range": f"bytes={start}-{end}"}
                offset = start
                async with http.stream("GET", url, headers=headers) as resp:
                    resp.raise_for_status()
                    async for data in resp.aiter_bytes(STREAM_BUFFER):
                        await asyncio.to_thread(os.pwrite, fd, data, offset)
                        offset += len(data)

        await asyncio.gather(*(fetch(s, e) for s, e in _ranges(size, chunk_size)))
    finally:
        await asyncio.to_thread(os.close, fd)
