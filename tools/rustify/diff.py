
"""Byte-for-byte diff of two captures produced by capture.py.

  python diff.py golden.json actual.json

Exit code 0 = identical (modulo the documented volatile headers). Exit 1 = differences.
"""

import json
import sys

from matrix import VOLATILE_HEADERS


def diff(a_path: str, b_path: str) -> bool:
    a = json.load(open(a_path))
    b = json.load(open(b_path))
    if len(a) != len(b):
        print(f"FAIL: matrix length {len(a)} != {len(b)}")
        return False
    n_diff = 0
    for ra, rb in zip(a, b):
        key = f"{ra['method']} {ra['path']}"
        problems = []
        if ra["status"] != rb["status"]:
            problems.append(f"status {ra['status']} != {rb['status']}")
        ha = {k: v for k, v in ra["all_headers"].items() if k not in VOLATILE_HEADERS}
        hb = {k: v for k, v in rb["all_headers"].items() if k not in VOLATILE_HEADERS}
        if ha != hb:
            problems.append(f"headers {ha} != {hb}")
        if ra["body_hex"] != rb["body_hex"]:
            ba = bytes.fromhex(ra["body_hex"])
            bb = bytes.fromhex(rb["body_hex"])
            problems.append(f"body differs:\n    py   = {ba[:300]!r}\n    rust = {bb[:300]!r}")
        if problems:
            n_diff += 1
            print(f"DIFF {key}")
            for p in problems:
                print("   ", p)
    if n_diff == 0:
        print(f"PASS: {len(a)} requests byte-for-byte identical")
        return True
    print(f"FAIL: {n_diff}/{len(a)} requests differ")
    return False


if __name__ == "__main__":
    ok = diff(sys.argv[1], sys.argv[2])
    sys.exit(0 if ok else 1)
