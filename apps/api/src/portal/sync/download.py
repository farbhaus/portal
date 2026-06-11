"""Parallel byte-range download of a Frame.io file to a local path.

The sync worker is the one place bytes legitimately flow through Portal — it pulls the original
from Frame.io's signed storage URL and writes it to a local destination (NAS/disk). To keep memory
flat regardless of file size (NFR: RSS must not scale with file size), ranges are written to disk
with positional writes (``os.pwrite``) and only a bounded window of them is held in memory.

The SHA-256 is computed *during* the download, in byte order, so the finished file is never
re-read — important when the destination is a network share (a re-read would double the NAS I/O).
The parallel path keeps byte-range concurrency for throughput and feeds a single hasher in index
order via a small reorder buffer.

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
    streaming GET. The hash is computed inline (no re-read). The caller owns placing/renaming the
    finished temp file.
    """
    own_client = client is None
    http = client or httpx.AsyncClient(timeout=_READ_TIMEOUT, follow_redirects=True)
    try:
        tmp_path.parent.mkdir(parents=True, exist_ok=True)
        if size is None or size <= chunk_size:
            total, sha = await _download_single(http, url, tmp_path)
        else:
            total, sha = await _download_ranged(http, url, size, tmp_path, concurrency, chunk_size)
    finally:
        if own_client:
            await http.aclose()

    if size is not None and total != size:
        raise OSError(f"size mismatch: expected {size}, wrote {total}")
    return DownloadResult(bytes_written=total, sha256=sha)


async def _download_single(http: httpx.AsyncClient, url: str, tmp_path: Path) -> tuple[int, str]:
    """Single streaming GET; bytes arrive in order so we hash them as they're written."""
    digest = hashlib.sha256()
    flags = os.O_CREAT | os.O_WRONLY | os.O_TRUNC
    fd = await asyncio.to_thread(os.open, str(tmp_path), flags, 0o644)
    try:
        offset = 0
        async with http.stream("GET", url) as resp:
            resp.raise_for_status()
            async for data in resp.aiter_bytes(STREAM_BUFFER):
                await asyncio.to_thread(os.pwrite, fd, data, offset)
                await asyncio.to_thread(digest.update, data)
                offset += len(data)
    finally:
        await asyncio.to_thread(os.close, fd)
    return offset, digest.hexdigest()


async def _download_ranged(
    http: httpx.AsyncClient,
    url: str,
    size: int,
    tmp_path: Path,
    concurrency: int,
    chunk_size: int,
) -> tuple[int, str]:
    """Parallel byte-range download with in-order hashing via a bounded reorder buffer.

    Ranges download concurrently and write to their offsets, but a single hasher consumes them
    strictly in index order. ``slots`` bounds how many ranges are downloaded-but-not-yet-hashed
    (so memory stays ~concurrency × chunk_size); a slot is released only when the hasher consumes
    that range. Slots are granted FIFO in index order and released in order, so the next index to
    hash always already holds a slot — no deadlock.
    """
    ranges = _ranges(size, chunk_size)
    digest = hashlib.sha256()
    buffers: dict[int, bytes] = {}
    ready = asyncio.Condition()
    slots = asyncio.Semaphore(max(1, concurrency))

    flags = os.O_CREAT | os.O_WRONLY | os.O_TRUNC
    fd = await asyncio.to_thread(os.open, str(tmp_path), flags, 0o644)
    try:
        await asyncio.to_thread(os.ftruncate, fd, size)

        async def fetch(idx: int, start: int, end: int) -> None:
            await slots.acquire()
            headers = {"Range": f"bytes={start}-{end}"}
            async with http.stream("GET", url, headers=headers) as resp:
                resp.raise_for_status()
                content = await resp.aread()
            await asyncio.to_thread(os.pwrite, fd, content, start)
            async with ready:
                buffers[idx] = content
                ready.notify_all()

        async def hasher() -> int:
            total = 0
            for idx in range(len(ranges)):
                async with ready:
                    while idx not in buffers:
                        await ready.wait()
                    content = buffers.pop(idx)
                await asyncio.to_thread(digest.update, content)
                total += len(content)
                slots.release()
            return total

        async with asyncio.TaskGroup() as tg:
            for idx, (start, end) in enumerate(ranges):
                tg.create_task(fetch(idx, start, end))
            hash_task = tg.create_task(hasher())
        total = hash_task.result()
    finally:
        await asyncio.to_thread(os.close, fd)
    return total, digest.hexdigest()
