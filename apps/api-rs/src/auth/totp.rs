
//! TOTP (RFC 6238) + one-time recovery codes — port of `portal.auth.totp`.
//!
//! The secret is stored AES-GCM-encrypted on the user row; verification allows a +/-1 step window.
//! Recovery codes are returned once and stored only as sha256 hashes, consumed on use.

use base32::Alphabet;
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

pub const ISSUER: &str = "Portal";
pub const RECOVERY_CODE_COUNT: usize = 10;

/// `pyotp.random_base32()` — base32 of 32 random bytes, unpadded.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("os random");
    base32::encode(Alphabet::Rfc4648 { padding: false }, &bytes)
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `pyotp.TOTP(secret).provisioning_uri(name=account, issuer_name=ISSUER)`.
pub fn provisioning_uri(secret: &str, account: &str) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}",
        urlencode(ISSUER),
        urlencode(account),
        secret,
        urlencode(ISSUER)
    )
}

fn normalize(code: &str) -> String {
    code.trim().replace(' ', "").replace('-', "").to_lowercase()
}

fn hotp(secret_bytes: &[u8], counter: u64) -> u32 {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret_bytes).expect("hmac key");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let bin = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    bin % 1_000_000
}

fn totp_at(secret_bytes: &[u8], time: u64, step: u64) -> String {
    format!("{:06}", hotp(secret_bytes, time / step))
}

/// `pyotp.TOTP(secret).verify(code, valid_window=1)`.
pub fn verify_code(secret: &str, code: &str) -> bool {
    let Some(secret_bytes) = base32::decode(Alphabet::Rfc4648 { padding: false }, &secret.to_uppercase()) else {
        return false;
    };
    let normalized = normalize(code);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for offset in [-1i64, 0, 1] {
        let t = (now as i64 + offset * 30).max(0) as u64;
        if totp_at(&secret_bytes, t, 30) == normalized {
            return true;
        }
    }
    false
}

/// SVG QR for the otpauth URI. The bytes differ from Python's `qrcode` SvgPathImage (different
/// library), but decode to the same payload.
pub fn qr_svg(uri: &str) -> String {
    match qrcode::QrCode::new(uri.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(200, 200)
            .build(),
        Err(_) => String::new(),
    }
}

fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(normalize(code).as_bytes()))
}

/// Return (plaintext codes to show once, sha256 hashes to store).
pub fn generate_recovery_codes() -> (Vec<String>, Vec<String>) {
    let mut codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
    for _ in 0..RECOVERY_CODE_COUNT {
        let mut a = [0u8; 3];
        let mut b = [0u8; 3];
        getrandom::fill(&mut a).expect("os random");
        getrandom::fill(&mut b).expect("os random");
        codes.push(format!("{}-{}", hex::encode(a), hex::encode(b)));
    }
    let hashes = codes.iter().map(|c| hash_code(c)).collect();
    (codes, hashes)
}

/// If `code` matches a stored hash, return the remaining hashes; otherwise None.
pub fn consume_recovery_code(code: &str, hashes: &[String]) -> Option<Vec<String>> {
    let target = hash_code(code);
    if !hashes.contains(&target) {
        return None;
    }
    Some(hashes.iter().filter(|h| **h != target).cloned().collect())
}
