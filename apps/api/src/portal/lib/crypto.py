import base64
import hashlib
import os

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

_NONCE_BYTES = 12


def _derive_key(key_material: str) -> bytes:
    """Derive a 32-byte AES key from arbitrary key material.

    Accepts a urlsafe-base64 32-byte key directly; otherwise hashes the material with
    SHA-256 so any non-empty string works as a key in dev. Production keys should be a
    real 32-byte random value (see .env.example).
    """
    try:
        decoded = base64.urlsafe_b64decode(key_material)
        if len(decoded) == 32:
            return decoded
    except (ValueError, base64.binascii.Error):  # type: ignore[attr-defined]
        pass
    return hashlib.sha256(key_material.encode("utf-8")).digest()


class TokenCipher:
    """AES-GCM encryption for tokens at rest. Output is base64(nonce || ciphertext)."""

    def __init__(self, key_material: str) -> None:
        self._aesgcm = AESGCM(_derive_key(key_material))

    def encrypt(self, plaintext: str) -> str:
        nonce = os.urandom(_NONCE_BYTES)
        ciphertext = self._aesgcm.encrypt(nonce, plaintext.encode("utf-8"), None)
        return base64.urlsafe_b64encode(nonce + ciphertext).decode("ascii")

    def decrypt(self, token: str) -> str:
        raw = base64.urlsafe_b64decode(token)
        nonce, ciphertext = raw[:_NONCE_BYTES], raw[_NONCE_BYTES:]
        return self._aesgcm.decrypt(nonce, ciphertext, None).decode("utf-8")
