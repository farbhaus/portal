import bcrypt


def hash_password(password: str) -> str:
    return bcrypt.hashpw(password.encode("utf-8"), bcrypt.gensalt()).decode("ascii")


def verify_password(password: str, password_hash: str) -> bool:
    try:
        return bcrypt.checkpw(password.encode("utf-8"), password_hash.encode("ascii"))
    except ValueError:
        return False


# A fixed hash to compare against when no account matches, so login pays one bcrypt verify whether
# or not the email exists — removes the timing signal that would reveal which emails are real.
_DUMMY_HASH = hash_password("portal-login-timing-equalizer")


def verify_dummy(password: str) -> None:
    """Run one throwaway bcrypt verify to keep login timing constant for unknown accounts."""
    verify_password(password, _DUMMY_HASH)
