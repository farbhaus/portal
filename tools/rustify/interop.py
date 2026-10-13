
"""Cross-service session-cookie interop + byte-identity check.

  python interop.py <python_url> <rust_url> <email> <password>

1. Log in to the Python service, present its cookie to the Rust service, and vice versa.
   Success = the receiving service accepts the cookie (no 401 for a protected route).
2. Log in to both in the same second and compare the Set-Cookie values byte for byte.
"""

import sys

import httpx


def login_cookie(base, email, password):
    with httpx.Client(base_url=base, timeout=30.0) as c:
        r = c.post("/api/auth/login", json={"email": email, "password": password})
        r.raise_for_status()
        return r.cookies.get("portal_session")


def protected_ok(base, cookie):
    """A protected route returns 401 without a session; anything else means auth passed."""
    with httpx.Client(base_url=base, timeout=30.0) as c:
        r = c.get("/api/upload-links", headers={"Cookie": f"portal_session={cookie}"})
        return r.status_code != 401, r.status_code


def main():
    py, rust, email, pw = sys.argv[1:5]
    # The signed timestamp is whole seconds, so byte-identity only holds when both logins land
    # in the same second; retry until that happens (or give up and report the mismatch).
    py_cookie = rust_cookie = ""
    for _ in range(30):
        py_cookie = login_cookie(py, email, pw)
        rust_cookie = login_cookie(rust, email, pw)
        if py_cookie == rust_cookie:
            break

    ok1, s1 = protected_ok(rust, py_cookie)
    ok2, s2 = protected_ok(py, rust_cookie)
    print(f"Rust accepts Python cookie : {ok1} (status {s1})")
    print(f"Python accepts Rust cookie : {ok2} (status {s2})")
    print(f"Cookies byte-identical     : {py_cookie == rust_cookie}")
    sys.exit(0 if (ok1 and ok2) else 1)


if __name__ == "__main__":
    main()
