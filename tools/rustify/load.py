
"""Closed-loop load generator used for both the Python and Rust services.

  python load.py <base_url> [--seconds 60] [--concurrency 32] [--paths PATH ...]

Reports requests/second and latency percentiles as JSON so before/after can be diffed.
"""

import argparse
import asyncio
import json
import time

import httpx


async def worker(client, paths, deadline, stats, idx):
    # Round-robin the path list so every connection sees the same mix.
    i = idx
    while time.perf_counter() < deadline:
        path = paths[i % len(paths)]
        i += 1
        t0 = time.perf_counter()
        try:
            r = await client.get(path)
            ok = r.status_code < 500
        except Exception:
            ok = False
        dt = time.perf_counter() - t0
        stats.append((dt, ok))


async def run(base_url, seconds, concurrency, paths):
    deadline = time.perf_counter() + seconds
    stats = []
    limits = httpx.Limits(max_connections=concurrency + 4, max_keepalive_connections=concurrency)
    async with httpx.AsyncClient(base_url=base_url, timeout=30.0, limits=limits) as client:
        # warmup
        for p in paths:
            try:
                await client.get(p)
            except Exception:
                pass
        t0 = time.perf_counter()
        await asyncio.gather(*(worker(client, paths, deadline, stats, i) for i in range(concurrency)))
        wall = time.perf_counter() - t0
    lat = sorted(d for d, _ in stats)
    n = len(lat)

    def pct(p):
        if not n:
            return 0.0
        return lat[min(n - 1, int(p * n))] * 1000

    return {
        "base_url": base_url,
        "seconds": round(wall, 2),
        "concurrency": concurrency,
        "requests": n,
        "errors": sum(1 for _, ok in stats if not ok),
        "rps": round(n / wall, 1),
        "p50_ms": round(pct(0.50), 3),
        "p95_ms": round(pct(0.95), 3),
        "p99_ms": round(pct(0.99), 3),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base_url")
    ap.add_argument("--seconds", type=float, default=60)
    ap.add_argument("--concurrency", type=int, default=32)
    ap.add_argument("--paths", nargs="+", default=["/api/health"])
    a = ap.parse_args()
    print(json.dumps(asyncio.run(run(a.base_url, a.seconds, a.concurrency, a.paths)), indent=2))


if __name__ == "__main__":
    main()
