import pyotp

from portal.auth import totp


def test_verify_code() -> None:
    secret = totp.generate_secret()
    assert totp.verify_code(secret, pyotp.TOTP(secret).now())
    assert not totp.verify_code(secret, "000000")


def test_verify_code_tolerates_formatting() -> None:
    secret = totp.generate_secret()
    code = pyotp.TOTP(secret).now()
    assert totp.verify_code(secret, f" {code} ")


def test_recovery_codes_generate_and_consume() -> None:
    codes, hashes = totp.generate_recovery_codes()
    assert len(codes) == len(hashes) == totp.RECOVERY_CODE_COUNT
    assert len(set(hashes)) == totp.RECOVERY_CODE_COUNT  # unique

    remaining = totp.consume_recovery_code(codes[0], hashes)
    assert remaining is not None
    assert len(remaining) == len(hashes) - 1
    # A consumed code can't be used again.
    assert totp.consume_recovery_code(codes[0], remaining) is None
    # An unknown code is rejected.
    assert totp.consume_recovery_code("zzzz-zzzz", hashes) is None


def test_provisioning_uri_and_qr() -> None:
    secret = totp.generate_secret()
    uri = totp.provisioning_uri(secret, "admin@example.com")
    assert uri.startswith("otpauth://totp/")
    assert "<svg" in totp.qr_svg(uri)
