from portal.lib.crypto import TokenCipher


def test_encrypt_decrypt_round_trip() -> None:
    cipher = TokenCipher("a-secret-key")
    plaintext = "frame.io-refresh-token-xyz"
    token = cipher.encrypt(plaintext)
    assert token != plaintext
    assert cipher.decrypt(token) == plaintext


def test_nonce_makes_ciphertext_unique() -> None:
    cipher = TokenCipher("a-secret-key")
    assert cipher.encrypt("same") != cipher.encrypt("same")


def test_base64_32byte_key_accepted() -> None:
    import base64
    import os

    key = base64.urlsafe_b64encode(os.urandom(32)).decode()
    cipher = TokenCipher(key)
    assert cipher.decrypt(cipher.encrypt("hello")) == "hello"
