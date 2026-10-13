
//! AES-GCM encryption for secrets at rest — port of `portal.lib.crypto.TokenCipher`.
//! Output is base64url(nonce || ciphertext) with a 12-byte nonce, matching the Python format
//! exactly, so either implementation can decrypt what the other wrote.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};

const NONCE_BYTES: usize = 12;

/// Derive a 32-byte AES key: accept a urlsafe-base64 32-byte key directly, otherwise SHA-256 the
/// material (so any non-empty string works in dev, as in Python).
fn derive_key(key_material: &str) -> [u8; 32] {
    for engine in [&URL_SAFE, &URL_SAFE_NO_PAD, &STANDARD_NO_PAD] {
        if let Ok(decoded) = engine.decode(key_material) {
            if decoded.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                return key;
            }
        }
    }
    Sha256::digest(key_material.as_bytes()).into()
}

pub struct TokenCipher {
    cipher: Aes256Gcm,
}

impl TokenCipher {
    pub fn new(key_material: &str) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(&derive_key(key_material)).expect("aes key"),
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> String {
        let mut nonce_bytes = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce_bytes).expect("os random");
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("aes encrypt");
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ciphertext);
        URL_SAFE.encode(out)
    }

    pub fn decrypt(&self, token: &str) -> Result<String, CryptoError> {
        let raw = URL_SAFE
            .decode(token)
            .or_else(|_| URL_SAFE_NO_PAD.decode(token))
            .map_err(|_| CryptoError)?;
        if raw.len() < NONCE_BYTES {
            return Err(CryptoError);
        }
        let (nonce, ciphertext) = raw.split_at(NONCE_BYTES);
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| CryptoError)?;
        String::from_utf8(plain).map_err(|_| CryptoError)
    }
}

#[derive(Debug)]
pub struct CryptoError;
impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "decryption failed")
    }
}
impl std::error::Error for CryptoError {}
