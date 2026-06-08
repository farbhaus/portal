from portal.auth.passwords import hash_password, verify_password


def test_hash_and_verify() -> None:
    h = hash_password("correct horse battery staple")
    assert h != "correct horse battery staple"
    assert verify_password("correct horse battery staple", h)


def test_wrong_password_rejected() -> None:
    h = hash_password("right")
    assert not verify_password("wrong", h)


def test_malformed_hash_returns_false() -> None:
    assert not verify_password("anything", "not-a-bcrypt-hash")
