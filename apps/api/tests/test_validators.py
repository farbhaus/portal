"""The HexColor request type that gates branding accent colours (CSS-injection guard)."""

import pytest
from pydantic import TypeAdapter, ValidationError

from portal.lib.validators import HexColor

_adapter = TypeAdapter(HexColor)


def test_accepts_six_and_three_digit_hex() -> None:
    assert _adapter.validate_python("#f59e0b") == "#f59e0b"
    assert _adapter.validate_python("#FFF") == "#FFF"
    assert _adapter.validate_python("  #abcdef  ") == "#abcdef"


def test_blank_and_none_become_none() -> None:
    assert _adapter.validate_python(None) is None
    assert _adapter.validate_python("   ") is None


@pytest.mark.parametrize(
    "bad",
    ["red", "f59e0b", "#12g", "#1234567", "javascript:alert(1)", "#fff;background:url(x)"],
)
def test_rejects_non_hex(bad: str) -> None:
    with pytest.raises(ValidationError):
        _adapter.validate_python(bad)
