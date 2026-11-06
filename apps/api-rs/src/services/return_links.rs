//! The Send link Portal keeps for an upload link's Exports folder — the Frame.io folder
//! FioTransfer uploads the user's Exports to — so the user has a link to hand the client. It
//! shares the whole folder (subfolders included), so whatever lands there later is in it too.
//!
//! Its settings follow the upload link: name, expiry, password, email code, the name/email it
//! asks for, page title and notification recipients. The upload link's expiry reminder covers
//! both. With no Exports folder (or no sync) it is revoked, not deleted, so its URL comes back
//! with the folder. It is deleted with its upload link. The upload link's `send_link` turns it
//! off; deleting the Send link does that too, so it doesn't come back.

use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;

/// The Exports folder a stored `folder.sync` names.
fn return_folder(sync: &Value) -> Option<&str> {
    sync.get("exports")?.get("folder_id")?.as_str()
}

/// The Send link's source: the Exports folder and everything under it, named from Frame.io
/// (best-effort: a link is still made when Frame.io can't say what the folder is called). Its
/// breadcrumb starts at the project, by the project's name, as the upload link's does; Frame.io's
/// own name for a project's top folder would group it under a "project" of that name.
async fn folder_source(state: &AppState, account_id: &str, project_id: &str, folder_id: &str) -> Value {
    let path: Vec<Value> = crate::services::frameio_paths::resolve_folder_path(state, account_id, project_id, folder_id)
        .await
        .map(|(path, _)| path)
        .unwrap_or_default();
    let label = path.last().and_then(|s| s.get("name")).and_then(|v| v.as_str()).unwrap_or("Send folder").to_string();
    json!({ "type": "folder", "account_id": account_id, "folder_id": folder_id, "recursive": true, "path": path, "label": label })
}

/// Whether a Send link's breadcrumb starts at its project's name (`<project> (root)`), as
/// `folder_source` makes it. Ones from before that start at Frame.io's own name for the
/// project's top folder.
fn starts_at_project(source: &Value) -> bool {
    source
        .get("path")
        .and_then(|p| p.get(0))
        .and_then(|s| s.get("name"))
        .and_then(|v| v.as_str())
        .is_some_and(|n| n.ends_with(" (root)"))
}

/// Bring an upload link's Send link in line with it: make it, update it, or revoke it.
pub async fn sync(state: &AppState, upload_id: Uuid) {
    let Ok(Some(up)) = sqlx::query(
        "SELECT name, folder, expires_at, password_hash, verify_email, uploader_fields_required,
                brand_display_name, brand_subtitle, notify_emails, revoked_at, send_link
           FROM links WHERE id = $1 AND kind = 'upload'",
    )
    .bind(upload_id)
    .fetch_optional(&state.db)
    .await
    else {
        return;
    };
    let folder: Value = up.get::<Option<Value>, _>("folder").unwrap_or(Value::Null);
    let account_id = folder.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let project_id = folder.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
    let wanted: bool = up.get("send_link");
    let target = folder.get("sync").and_then(return_folder).filter(|_| wanted && !account_id.is_empty());
    let existing: Option<(Uuid, Value)> = sqlx::query_as("SELECT id, source FROM links WHERE return_for = $1")
        .bind(upload_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

    let Some(target) = target else {
        if let Some((id, _)) = existing {
            let _ = sqlx::query("UPDATE links SET revoked_at = COALESCE(revoked_at, now()), updated_at = now() WHERE id = $1")
                .bind(id)
                .execute(&state.db)
                .await;
        }
        return;
    };

    // Frame.io names the folder only when it is new or changed, or when its breadcrumb was made
    // before it started at the project's name.
    let source = match &existing {
        Some((_, s)) if s.get("folder_id").and_then(|v| v.as_str()) == Some(target) && starts_at_project(s) => s.clone(),
        _ => folder_source(state, account_id, project_id, target).await,
    };
    let asks: Value = up.get::<Option<Value>, _>("uploader_fields_required").unwrap_or_else(|| json!({}));
    let flag = |k: &str| asks.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let viewer_fields = json!({ "name": flag("name"), "email": flag("email") });
    let expires_at: Option<chrono::DateTime<chrono::Utc>> = up.get("expires_at");

    let name: Option<String> = up.get("name");
    let password_hash: Option<String> = up.get("password_hash");
    let verify_email: bool = up.get("verify_email");
    let title: Option<String> = up.get("brand_display_name");
    let subtitle: Option<String> = up.get("brand_subtitle");
    let notify: Option<Value> = up.get("notify_emails");
    let revoked_at: Option<chrono::DateTime<chrono::Utc>> = up.get("revoked_at");
    let saved = match existing {
        Some((id, _)) => {
            sqlx::query(
                "UPDATE links SET name = $2, source = $3, expires_at = $4, password_hash = $5, verify_email = $6,
                        viewer_fields_required = $7, brand_display_name = $8, brand_subtitle = $9,
                        notify_emails = $10, revoked_at = $11, remind_before_expiry = false, updated_at = now()
                  WHERE id = $1",
            )
            .bind(id)
            .bind(&name)
            .bind(&source)
            .bind(expires_at)
            .bind(&password_hash)
            .bind(verify_email)
            .bind(&viewer_fields)
            .bind(&title)
            .bind(&subtitle)
            .bind(&notify)
            .bind(revoked_at)
            .execute(&state.db)
            .await
        }
        None => {
            sqlx::query(
                "INSERT INTO links (kind, token, return_for, name, source, expires_at, password_hash, verify_email,
                        viewer_fields_required, brand_display_name, brand_subtitle, notify_emails, revoked_at,
                        allow_preview, downloads_count, remind_before_expiry)
                 VALUES ('download',$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,true,0,false)",
            )
            .bind(crate::routes::download_links::generate_token())
            .bind(upload_id)
            .bind(&name)
            .bind(&source)
            .bind(expires_at)
            .bind(&password_hash)
            .bind(verify_email)
            .bind(&viewer_fields)
            .bind(&title)
            .bind(&subtitle)
            .bind(&notify)
            .bind(revoked_at)
            .execute(&state.db)
            .await
        }
    };
    if let Err(e) = saved {
        tracing::error!("return_links.sync_failed upload_link_id={upload_id} error={e}");
    }
}

/// Upload links with an Exports folder and a Send link wanted but none yet (set before Portal
/// made them, or by an older release), or one whose breadcrumb doesn't start at the project's
/// name: make or fix theirs. Run once when the worker starts.
pub async fn backfill(state: &AppState) {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT l.id FROM links l
          WHERE l.kind = 'upload'
            AND l.send_link
            AND l.folder->'sync'->'exports'->>'folder_id' IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM links r WHERE r.return_for = l.id
                             AND r.source->'path'->0->>'name' LIKE '% (root)')",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    for id in &ids {
        sync(state, *id).await;
    }
    if !ids.is_empty() {
        tracing::info!("return_links.backfilled count={}", ids.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_folder_is_the_exports_folder() {
        assert_eq!(return_folder(&json!({ "exports": { "local": "0_EXPORTS", "folder_id": "e" } })), Some("e"));
        assert_eq!(return_folder(&json!({ "drive": { "local": "1_DRIVE", "folder_id": "d" } })), None);
        assert_eq!(return_folder(&json!({ "exports": { "local": "0_EXPORTS" } })), None);
        assert_eq!(return_folder(&json!({})), None);
    }

    #[test]
    fn a_breadcrumb_from_frameio_alone_is_rebuilt() {
        assert!(starts_at_project(&json!({ "path": [{ "id": "r", "name": "ACME (root)" }, { "id": "e", "name": "0_EXPORTS" }] })));
        assert!(!starts_at_project(&json!({ "path": [{ "id": "r", "name": "root" }, { "id": "e", "name": "0_EXPORTS" }] })));
        assert!(!starts_at_project(&json!({ "path": [] })));
        assert!(!starts_at_project(&json!({})));
    }
}
