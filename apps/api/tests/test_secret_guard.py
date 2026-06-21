"""The startup guard that refuses to boot with the .env.example placeholder secrets."""

import pytest

from portal.lib.config import Settings, require_secure_secrets


def test_rejects_default_session_secret() -> None:
    s = Settings(session_secret="change-me-session-secret")
    with pytest.raises(RuntimeError) as exc:
        require_secure_secrets(s)
    assert "session_secret" in str(exc.value)


def test_rejects_default_admin_password() -> None:
    s = Settings(admin_password="change-me")
    with pytest.raises(RuntimeError) as exc:
        require_secure_secrets(s)
    assert "admin_password" in str(exc.value)


def test_escape_hatch_allows_defaults() -> None:
    s = Settings(
        session_secret="change-me-session-secret",
        token_encryption_key="change-me-token-encryption-key",
        admin_password="change-me",
        allow_insecure_defaults=True,
    )
    require_secure_secrets(s)  # must not raise


def test_strong_secrets_pass() -> None:
    # The test environment (conftest) already supplies non-default secrets.
    require_secure_secrets(Settings())
