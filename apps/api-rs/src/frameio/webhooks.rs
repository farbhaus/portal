
//! Frame.io webhook signature verification + payload parsing — port of `portal.frameio.webhooks`.

use chrono::Utc;
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

pub const TIMESTAMP_HEADER: &str = "x-frameio-request-timestamp";
pub const SIGNATURE_HEADER: &str = "x-frameio-signature";
pub const DEFAULT_TOLERANCE_SECONDS: i64 = 300;

pub const SYNC_TRIGGER_EVENTS: [&str; 2] = ["file.upload.completed", "file.ready"];

pub struct WebhookPayload {
    pub r#type: Option<String>,
    pub account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub resource_id: Option<String>,
    pub resource_type: Option<String>,
    pub user_id: Option<String>,
}

/// True iff `signature` is a valid, in-window Frame.io signature for `body`.
pub fn verify_signature(
    secret: &str,
    timestamp: &str,
    body: &[u8],
    signature: &str,
    now: Option<i64>,
    tolerance_seconds: i64,
) -> bool {
    if secret.is_empty() || signature.is_empty() || timestamp.is_empty() {
        return false;
    }
    let Ok(sent_at) = timestamp.parse::<i64>() else {
        return false;
    };
    let current = now.unwrap_or_else(|| Utc::now().timestamp());
    if (current - sent_at).abs() > tolerance_seconds {
        return false;
    }
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(b"v0:");
    mac.update(timestamp.as_bytes());
    mac.update(b":");
    mac.update(body);
    let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    // constant-time-ish compare
    expected.len() == signature.len()
        && expected
            .bytes()
            .zip(signature.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

pub fn parse_webhook_payload(raw: &Value) -> WebhookPayload {
    let id = |key: &str| -> Option<String> {
        raw.get(key)
            .and_then(|v| v.as_object())
            .and_then(|o| o.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let resource = raw.get("resource").and_then(|v| v.as_object());
    WebhookPayload {
        r#type: raw.get("type").and_then(|v| v.as_str()).map(|s| s.to_string()),
        account_id: id("account"),
        workspace_id: id("workspace"),
        project_id: id("project"),
        resource_id: resource.and_then(|r| r.get("id")).and_then(|v| v.as_str()).map(|s| s.to_string()),
        resource_type: resource.and_then(|r| r.get("type")).and_then(|v| v.as_str()).map(|s| s.to_string()),
        user_id: id("user"),
    }
}
