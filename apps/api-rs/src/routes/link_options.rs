
//! Options both kinds of link share and validate the same way: who gets the link's notification
//! emails, and the optional reminder before it expires.

use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::error::detail;
use crate::state::AppState;

/// `notify_emails`: a list of addresses (or one comma/space-separated string); empty → null,
/// meaning the app's notification address.
pub fn notify_emails(v: Option<&Value>) -> Result<Option<Value>, Response> {
    let raw: Vec<String> = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => s.split([',', ';', ' ', '\n']).map(|x| x.to_string()).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
        Some(_) => return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "notify_emails must be a list of addresses")),
    };
    let mut out: Vec<String> = Vec::new();
    for e in raw.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
        let valid = e.split_once('@').is_some_and(|(user, host)| !user.is_empty() && host.contains('.'));
        if !valid {
            return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, &format!("“{e}” isn't an email address")));
        }
        if !out.contains(&e) {
            out.push(e);
        }
    }
    if out.len() > 10 {
        return Err(detail(StatusCode::UNPROCESSABLE_ENTITY, "Up to 10 notification addresses per link"));
    }
    Ok(if out.is_empty() { None } else { Some(json!(out)) })
}

/// Apply the shared options present in a PATCH body (`exclude_unset`), and forget a sent
/// reminder when the expiry moves, so the new date gets its own.
pub async fn apply_update(state: &AppState, id: Uuid, body: &Map<String, Value>) -> Result<(), Response> {
    if body.contains_key("notify_emails") {
        let n = notify_emails(body.get("notify_emails"))?;
        let _ = sqlx::query("UPDATE links SET notify_emails = $1 WHERE id = $2").bind(n).bind(id).execute(&state.db).await;
    }
    if let Some(v) = body.get("remind_before_expiry").and_then(|v| v.as_bool()) {
        let _ = sqlx::query("UPDATE links SET remind_before_expiry = $1 WHERE id = $2").bind(v).bind(id).execute(&state.db).await;
    }
    if body.contains_key("expires_at") {
        let _ = sqlx::query("UPDATE links SET expiry_reminded_at = NULL WHERE id = $1").bind(id).execute(&state.db).await;
    }
    let _ = sqlx::query("UPDATE links SET updated_at = now() WHERE id = $1").bind(id).execute(&state.db).await;
    Ok(())
}

/// The shared options as the admin API renders them.
pub fn render(row: &sqlx::postgres::PgRow, out: &mut Value) {
    use sqlx::Row;
    out["notify_emails"] = row.get::<Option<Value>, _>("notify_emails").unwrap_or(Value::Null);
    out["remind_before_expiry"] = json!(row.get::<bool, _>("remind_before_expiry"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_emails_normalizes_and_rejects_junk() {
        assert_eq!(notify_emails(Some(&json!("A@x.example, b@y.example a@x.example"))).ok().flatten(), Some(json!(["a@x.example", "b@y.example"])));
        assert_eq!(notify_emails(Some(&json!([]))).ok().flatten(), None);
        assert_eq!(notify_emails(Some(&json!(""))).ok().flatten(), None);
        assert!(notify_emails(Some(&json!(["not-an-address"]))).is_err());
        assert!(notify_emails(Some(&json!(42))).is_err());
    }
}
