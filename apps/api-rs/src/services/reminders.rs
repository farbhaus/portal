//! Optional reminder before a link expires: for links with `remind_before_expiry`, one email a day
//! ahead to the link's notification addresses (or the app's), so it can be extended in time.
//! Run by the worker every few minutes; `expiry_reminded_at` makes it once per expiry date.

use sqlx::Row;
use uuid::Uuid;

use crate::services::runtime_config::{email_brand, get_email_config};
use crate::state::AppState;

/// Send the reminders that are due; returns how many links were reminded.
pub async fn send_due(state: &AppState) -> usize {
    let config = get_email_config(&state.db, &state.settings).await;
    if !config.configured() {
        return 0;
    }
    let rows = sqlx::query(
        "SELECT id, kind, token, name, brand_display_name, expires_at, notify_emails
           FROM links
          WHERE remind_before_expiry AND revoked_at IS NULL AND expiry_reminded_at IS NULL
            AND expires_at > now() AND expires_at <= now() + interval '24 hours'",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let base = state.settings.base_url.trim_end_matches('/').to_string();
    let mut sent = 0;
    for r in rows {
        let id: Uuid = r.get("id");
        let kind: String = r.get("kind");
        let token: String = r.get("token");
        let name = r
            .get::<Option<String>, _>("name")
            .or_else(|| r.get::<Option<String>, _>("brand_display_name"))
            .unwrap_or_else(|| "A link".to_string());
        let expires: chrono::DateTime<chrono::Utc> = r.get("expires_at");
        let admin_url = format!("{base}/{}/{id}", if kind == "upload" { "upload-links" } else { "download-links" });
        let public_url = format!("{base}/{}/{token}", if kind == "upload" { "u" } else { "d" });
        let to: Vec<String> = r
            .get::<Option<serde_json::Value>, _>("notify_emails")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        let to = if to.is_empty() { vec![config.resolved_notify_email()] } else { to };
        let brand = email_brand(&state.db, &state.settings).await;
        let (subject, text, html) = crate::notify::render_expiry_reminder(&name, &expires, &admin_url, &public_url, &brand);
        let mut any = false;
        for addr in &to {
            any |= crate::notify::send_email(&config, addr, &subject, &text, Some(&html)).await.is_ok();
        }
        if any {
            let _ = sqlx::query("UPDATE links SET expiry_reminded_at = now() WHERE id = $1").bind(id).execute(&state.db).await;
            sent += 1;
        }
    }
    if sent > 0 {
        tracing::info!("reminders.expiry.sent count={sent}");
    }
    sent
}
