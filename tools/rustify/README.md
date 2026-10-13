# tools/rustify

Verification and measurement tooling for the Python → Rust conversion of Portal.

- `matrix.py` — the shared request matrix and the list of transport headers excluded from the
  byte diff.
- `capture.py <base_url> <out.json>` — replay the matrix, store raw responses (hex bodies).
- `diff.py <golden.json> <actual.json>` — byte-for-byte diff; exit 1 on any difference.
- `load.py` — pure-Python load generator (kept for reference).
- The compiled load generator used for the real numbers lives in
  `apps/api-rs/src/bin/loadgen.rs` (same client for both services).

Run the Python tools with `uv run --with httpx python <tool>.py ...`.
