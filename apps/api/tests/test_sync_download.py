import hashlib
import os
from pathlib import Path

import httpx
import pytest

from portal.sync.download import _ranges, download_to_file

BLOB = os.urandom(5 * 1024 * 1024 + 123)  # ~5 MiB, not a chunk multiple


def _handler(request: httpx.Request) -> httpx.Response:
    rng = request.headers.get("Range")
    if rng:
        spec = rng.removeprefix("bytes=")
        start_s, end_s = spec.split("-")
        start, end = int(start_s), int(end_s)
        return httpx.Response(206, content=BLOB[start : end + 1])
    return httpx.Response(200, content=BLOB)


def _client() -> httpx.AsyncClient:
    return httpx.AsyncClient(transport=httpx.MockTransport(_handler))


def test_ranges() -> None:
    assert _ranges(10, 4) == [(0, 3), (4, 7), (8, 9)]
    assert _ranges(8, 4) == [(0, 3), (4, 7)]


async def test_single_get_small_file(tmp_path: Path) -> None:
    out = tmp_path / "small.bin"
    async with _client() as c:
        result = await download_to_file(
            "https://x/file", len(BLOB), out, chunk_size=10**9, client=c
        )
    assert result.bytes_written == len(BLOB)
    assert result.sha256 == hashlib.sha256(BLOB).hexdigest()
    assert out.read_bytes() == BLOB


async def test_ranged_parallel_download(tmp_path: Path) -> None:
    out = tmp_path / "big.bin"
    async with _client() as c:
        result = await download_to_file(
            "https://x/file", len(BLOB), out, chunk_size=1024 * 1024, concurrency=4, client=c
        )
    assert result.bytes_written == len(BLOB)
    assert result.sha256 == hashlib.sha256(BLOB).hexdigest()
    assert out.read_bytes() == BLOB


async def test_unknown_size_streams_single(tmp_path: Path) -> None:
    out = tmp_path / "stream.bin"
    async with _client() as c:
        result = await download_to_file("https://x/file", None, out, client=c)
    assert result.bytes_written == len(BLOB)
    assert out.read_bytes() == BLOB


async def test_size_mismatch_raises(tmp_path: Path) -> None:
    out = tmp_path / "bad.bin"
    async with _client() as c:
        with pytest.raises(OSError):
            await download_to_file(
                "https://x/file", len(BLOB) + 999, out, chunk_size=10**9, client=c
            )
