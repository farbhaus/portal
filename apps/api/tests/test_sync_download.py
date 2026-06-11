import asyncio
import hashlib
import os
from collections.abc import Callable
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


class _RangedTransport(httpx.AsyncBaseTransport):
    """Async transport that serves ranges of BLOB, with optional per-range delay / failure.

    ``delay_for`` lets a test scramble completion order; ``fail_start`` makes one range 500.
    """

    def __init__(
        self,
        *,
        delay_for: Callable[[int], float] | None = None,
        fail_start: int | None = None,
    ) -> None:
        self._delay_for = delay_for or (lambda _start: 0.0)
        self._fail_start = fail_start

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        rng = request.headers.get("Range")
        if rng is None:
            return httpx.Response(200, content=BLOB)
        start_s, end_s = rng.removeprefix("bytes=").split("-")
        start, end = int(start_s), int(end_s)
        if self._fail_start is not None and start == self._fail_start:
            return httpx.Response(500, content=b"boom")
        await asyncio.sleep(self._delay_for(start))
        return httpx.Response(206, content=BLOB[start : end + 1])


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


async def test_ranged_hash_correct_despite_out_of_order_completion(tmp_path: Path) -> None:
    # Lower-offset ranges finish LAST, so the reorder buffer must hold completed ranges and still
    # hash in byte order. If hashing followed completion order the digest would be wrong.
    out = tmp_path / "ooo.bin"
    delay_for = lambda start: (len(BLOB) - start) / len(BLOB) * 0.05  # noqa: E731
    transport = _RangedTransport(delay_for=delay_for)
    async with httpx.AsyncClient(transport=transport) as c:
        result = await download_to_file(
            "https://x/file", len(BLOB), out, chunk_size=512 * 1024, concurrency=4, client=c
        )
    assert result.bytes_written == len(BLOB)
    assert result.sha256 == hashlib.sha256(BLOB).hexdigest()
    assert out.read_bytes() == BLOB


async def test_ranged_failure_propagates(tmp_path: Path) -> None:
    out = tmp_path / "fail.bin"
    transport = _RangedTransport(fail_start=1024 * 1024)  # the second 1 MiB range 500s
    async with httpx.AsyncClient(transport=transport) as c:
        with pytest.raises(BaseExceptionGroup):
            await download_to_file(
                "https://x/file", len(BLOB), out, chunk_size=1024 * 1024, concurrency=4, client=c
            )
