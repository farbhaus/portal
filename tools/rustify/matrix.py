
"""Shared request matrix for the byte-for-byte differential.

Keep this in one place so the Python capture and the Rust capture exercise the exact same
requests in the exact same order. Fixture tokens are stable (see fixtures.sql).
"""

MATRIX = [
    ("GET", "/api/health", {}),
    ("GET", "/api/branding", {}),
    ("GET", "/api/dashboard/transfers", {}),
    ("GET", "/api/upload-links", {}),
    ("GET", "/api/public/links/doesnotexist", {}),
    ("GET", "/api/privacy/subject", {}),
    ("POST", "/api/auth/login", {"json": {"email": "admin@example.com", "password": "wrong"}}),
    ("GET", "/api/openapi.json", {}),
    ("GET", "/api/settings/security", {}),
    ("GET", "/api/frameio/status", {}),
]

# Headers that the transport/host injects and which therefore are not part of the app's
# contract. Everything else must match byte for byte.
VOLATILE_HEADERS = {"date", "server", "content-length", "x-request-id", "set-cookie"}
