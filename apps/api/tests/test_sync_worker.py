import httpx

from portal.sync.worker import _unwrap_error


def test_unwrap_plain_exception() -> None:
    assert _unwrap_error(ValueError("boom")) == "ValueError: boom"


def test_unwrap_exception_group() -> None:
    # A TaskGroup wraps the real cause in an ExceptionGroup; the recorded error should name it.
    err = httpx.HTTPStatusError(
        "403", request=httpx.Request("GET", "https://x"), response=httpx.Response(403)
    )
    group = BaseExceptionGroup("unhandled errors in a TaskGroup", [err])
    out = _unwrap_error(group)
    assert "HTTPStatusError" in out
    assert "403" in out


def test_unwrap_nested_group() -> None:
    inner = BaseExceptionGroup("inner", [OSError("disk full")])
    outer = BaseExceptionGroup("outer", [inner])
    assert "OSError: disk full" in _unwrap_error(outer)
