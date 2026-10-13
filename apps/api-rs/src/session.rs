
//! Drop-in reimplementation of Starlette's `SessionMiddleware`.
//!
//! Byte-for-byte equivalence is required on two observable things:
//!   * the `Vary: Cookie` header, which Starlette adds **only when the session was read**, and
//!   * the `Set-Cookie` value, which is `itsdangerous.TimestampSigner` over standard-base64 of
//!     `json.dumps(session)`.
//!
//! The signing details are copied from itsdangerous 2.2.0 (`signer.py`, `timed.py`,
//! `encoding.py`): separator `.`, salt `itsdangerous.Signer`, "django-concat" key derivation
//! `sha1(salt + b"signer" + secret)`, HMAC-SHA1, url-safe base64 without padding, and a
//! timestamp encoded as big-endian bytes with leading zeros stripped.

use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha1::Sha1;

const SALT: &[u8] = b"itsdangerous.Signer";
const SEP: u8 = b'.';
pub const SESSION_COOKIE: &str = "portal_session";
pub const MAX_AGE: i64 = 14 * 24 * 60 * 60;

#[derive(Default)]
pub struct SessionState {
    pub data: Map<String, Value>,
    pub accessed: bool,
    pub modified: bool,
    pub initial_empty: bool,
}

impl SessionState {
    pub fn get(&mut self, key: &str) -> Option<&Value> {
        self.accessed = true;
        self.data.get(key)
    }
    pub fn set(&mut self, key: &str, value: Value) {
        self.accessed = true;
        self.modified = true;
        self.data.insert(key.to_string(), value);
    }
    /// Remove a key, returning its previous value (Python's `session.pop(key)` semantics).
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.accessed = true;
        if self.data.contains_key(key) {
            self.modified = true;
        }
        self.data.remove(key)
    }
}

pub type SessionHandle = Arc<Mutex<SessionState>>;

fn derive_key(secret: &[u8]) -> [u8; 20] {
    use sha1::Digest;
    let mut h = Sha1::new();
    h.update(SALT);
    h.update(b"signer");
    h.update(secret);
    h.finalize().into()
}

fn sign_value(secret: &[u8], value: &[u8]) -> String {
    let key = derive_key(secret);
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(&key).expect("hmac key");
    mac.update(value);
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// `int_to_bytes`: big-endian, leading zero bytes stripped (itsdangerous `encoding.py`).
fn int_to_bytes(mut num: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while num > 0 {
        out.push((num & 0xff) as u8);
        num >>= 8;
    }
    out.reverse();
    out
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sign a session payload exactly as `TimestampSigner.sign` does.
pub fn sign(secret: &[u8], value: &[u8]) -> String {
    let ts = URL_SAFE_NO_PAD.encode(int_to_bytes(now_secs() as u64));
    let mut v2 = value.to_vec();
    v2.push(SEP);
    v2.extend_from_slice(ts.as_bytes());
    let sig = sign_value(secret, &v2);
    let mut out = String::from_utf8(v2).expect("base64 is utf-8");
    out.push('.');
    out.push_str(&sig);
    out
}

/// Unsign and verify, returning the payload (standard base64) or `None` on any failure —
/// matching Starlette's `except BadSignature -> empty session`.
pub fn unsign(secret: &[u8], signed: &str, max_age: Option<i64>) -> Option<Vec<u8>> {
    let bytes = signed.as_bytes();
    let idx = bytes.iter().rposition(|&b| b == SEP)?;
    let (v2, sig) = (&bytes[..idx], &bytes[idx + 1..]);
    if sig != sign_value(secret, v2).as_bytes() {
        return None;
    }
    let idx2 = v2.iter().rposition(|&b| b == SEP)?;
    let (payload, ts_b64) = (&v2[..idx2], &v2[idx2 + 1..]);
    if let Some(max_age) = max_age {
        let ts_bytes = URL_SAFE_NO_PAD.decode(ts_b64).ok()?;
        let mut padded = [0u8; 8];
        let start = 8usize.saturating_sub(ts_bytes.len());
        padded[start..].copy_from_slice(&ts_bytes[..ts_bytes.len().min(8)]);
        let ts = u64::from_be_bytes(padded) as i64;
        let age = now_secs() - ts;
        if age > max_age || age < 0 {
            return None;
        }
    }
    Some(payload.to_vec())
}

/// Build the `Set-Cookie` value Starlette writes when the session was modified.
pub fn set_cookie(secret: &[u8], data: &Map<String, Value>, secure: bool) -> String {
    // Python `json.dumps` defaults: ", " and ": " separators.
    let json = python_json(data);
    let payload = STANDARD.encode(json.as_bytes());
    let signed = sign(secret, payload.as_bytes());
    let mut flags = "httponly; samesite=lax".to_string();
    if secure {
        flags.push_str("; secure");
    }
    format!("{SESSION_COOKIE}={signed}; path=/; Max-Age={MAX_AGE}; {flags}")
}


/// `json.dumps(map)` with Python's default separators (", ", ": ") and ASCII escaping — so a
/// Rust-issued session cookie is byte-identical to the Python one for the same payload+second.
fn python_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) > 0x7e => {
                for unit in c.encode_utf16(&mut [0u16; 2]).iter().filter(|u| **u != 0) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

pub fn python_json(map: &Map<String, Value>) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in map.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push('"');
        s.push_str(&python_escape(k));
        s.push_str("\": ");
        match v {
            Value::String(x) => {
                s.push('"');
                s.push_str(&python_escape(x));
                s.push('"');
            }
            other => s.push_str(&other.to_string()),
        }
    }
    s.push('}');
    s
}

fn parse_cookie(cookie_header: &str, name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Middleware: decode the session cookie into the request, then apply Starlette's response
/// header rules (`Vary: Cookie` when read, `Set-Cookie` when modified).
pub async fn session_middleware(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let secret = state.settings.session_secret.clone();
    let secure = state.settings.session_cookie_secure;
    let decoded = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| parse_cookie(h, SESSION_COOKIE));

    let mut session = SessionState::default();
    if let Some(cookie) = decoded {
        if let Some(payload) = unsign(secret.as_bytes(), &cookie, Some(MAX_AGE)) {
            if let Ok(v) = STANDARD.decode(payload) {
                if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&v) {
                    session.data = map;
                    session.initial_empty = false;
                }
            }
        }
    }
    let handle: SessionHandle = Arc::new(Mutex::new(session));
    req.extensions_mut().insert(handle.clone());

    // SPA auth gate (SSR parity): guarded pages require a session before the shell is served.
    if let Some(redirect) = crate::spa::maybe_redirect(&state, req.uri().path(), &handle).await {
        return redirect;
    }

    let mut response = next.run(req).await;

    let s = handle.lock().unwrap();
    if s.accessed {
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("Cookie"));
    }
    if s.modified && !s.data.is_empty() {
        let value = set_cookie(secret.as_bytes(), &s.data, secure);
        if let Ok(hv) = HeaderValue::from_str(&value) {
            response.headers_mut().append(header::SET_COOKIE, hv);
        }
    }
    response
}
