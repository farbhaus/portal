
"""Capture raw HTTP responses for the shared matrix against a running service.

  python capture.py <base_url> <out.json> [--request-id]

Bodies are stored hex-encoded so nothing is lost to text mangling.
"""

import json
import sys

import httpx

from matrix import MATRIX, VOLATILE_HEADERS


def capture(base_url: str, out_path: str) -> None:
    out = []
    with httpx.Client(base_url=base_url, timeout=30.0) as client:
        for method, path, kw in MATRIX:
            headers = dict(kw.get("headers") or {})
            # Pin the request id so generated ids don't pollute the diff.
            headers.setdefault("X-Request-Id", "00000000000000000000000000000000")
            body = kw.get("json")
            r = client.request(method, path, headers=headers, json=body)
            out.append(
                {
                    "method": method,
                    "path": path,
                    "status": r.status_code,
                    "headers": {
                        k.lower(): v
                        for k, v in r.headers.items()
                        if k.lower() not in VOLATILE_HEADERS
                    },
                    "all_headers": {k.lower(): v for k, v in r.headers.items()},
                    "body_hex": r.content.hex(),
                }
            )
    with open(out_path, "w") as fh:
        json.dump(out, fh, indent=2, sort_keys=False)
    print(f"captured {len(out)} responses -> {out_path}")


if __name__ == "__main__":
    capture(sys.argv[1], sys.argv[2])
