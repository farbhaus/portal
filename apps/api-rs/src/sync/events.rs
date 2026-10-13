
//! Turn Frame.io webhook events into sync jobs — port of `portal.sync.events`.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::frameio::webhooks::{parse_webhook_payload, SYNC_TRIGGER_EVENTS};

pub fn rule_matches(source_config: &serde_json::Value, payload: &crate::frameio::webhooks::WebhookPayload) -> bool {
    for (key, value) in [
        ("account_id", &payload.account_id),
        ("workspace_id", &payload.workspace_id),
        ("project_id", &payload.project_id),
    ] {
        let Some(v) = value else { return false };
        if source_config.get(key).and_then(|x| x.as_str()) != Some(v.as_str()) {
            return false;
        }
    }
    true
}

pub fn is_sync_trigger(payload: &crate::frameio::webhooks::WebhookPayload) -> bool {
    payload
        .r#type
        .as_deref()
        .map(|t| SYNC_TRIGGER_EVENTS.contains(&t))
        .unwrap_or(false)
        && payload.resource_type.as_deref() == Some("file")
}

async fn create_jobs(db: &PgPool, payload: &crate::frameio::webhooks::WebhookPayload, file_id: &str) -> Vec<Uuid> {
    let rules = sqlx::query("SELECT id, source_config FROM sync_rules WHERE enabled")
        .fetch_all(db)
        .await
        .unwrap_or_default();
    let mut created = Vec::new();
    for r in rules {
        let source: serde_json::Value = r.get("source_config");
        if !rule_matches(&source, payload) {
            continue;
        }
        let rule_id: Uuid = r.get("id");
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM sync_jobs WHERE sync_rule_id = $1 AND frameio_file_id = $2",
        )
        .bind(rule_id)
        .bind(file_id)
        .fetch_optional(db)
        .await
        .unwrap_or(None);
        if existing.is_some() {
            continue;
        }
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO sync_jobs (sync_rule_id, frameio_file_id, status, retry_count) VALUES ($1,$2,'pending',0) RETURNING id",
        )
        .bind(rule_id)
        .bind(file_id)
        .fetch_one(db)
        .await
        .unwrap_or(Uuid::nil());
        if id != Uuid::nil() {
            created.push(id);
        }
    }
    created
}

/// Process one persisted webhook event; returns the ids of created jobs.
pub async fn process_event(db: &PgPool, event_id: Uuid) -> Vec<Uuid> {
    let row = sqlx::query("SELECT raw, processed FROM webhook_events WHERE id = $1")
        .bind(event_id)
        .fetch_optional(db)
        .await
        .unwrap_or(None);
    let Some(row) = row else { return Vec::new() };
    let processed: bool = row.get("processed");
    if processed {
        return Vec::new();
    }
    let raw: serde_json::Value = row.get("raw");

    let mut created = Vec::new();
    let mut error: Option<String> = None;
    let payload = parse_webhook_payload(&raw);
    if is_sync_trigger(&payload) {
        if let Some(file_id) = &payload.resource_id {
            created = create_jobs(db, &payload, file_id).await;
        }
    }
    let _ = &mut error;
    let _ = sqlx::query("UPDATE webhook_events SET processed=true, processed_at=now(), error=$1 WHERE id=$2")
        .bind(error.as_deref())
        .bind(event_id)
        .execute(db)
        .await;
    created
}

/// Drive sync the moment a client upload through a Portal link completes.
pub async fn trigger_sync_for_upload(
    db: &PgPool,
    account_id: &str,
    folder_id: &str,
    file_ids: &[String],
) -> Vec<Uuid> {
    if file_ids.is_empty() {
        return Vec::new();
    }
    let rules = sqlx::query("SELECT id, source_config FROM sync_rules WHERE enabled")
        .fetch_all(db)
        .await
        .unwrap_or_default();
    let mut created = Vec::new();
    for r in rules {
        let source: serde_json::Value = r.get("source_config");
        if source.get("account_id").and_then(|v| v.as_str()) != Some(account_id)
            || source.get("folder_id").and_then(|v| v.as_str()) != Some(folder_id)
        {
            continue;
        }
        let rule_id: Uuid = r.get("id");
        for file_id in file_ids {
            let existing: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM sync_jobs WHERE sync_rule_id = $1 AND frameio_file_id = $2",
            )
            .bind(rule_id)
            .bind(file_id)
            .fetch_optional(db)
            .await
            .unwrap_or(None);
            if existing.is_some() {
                continue;
            }
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO sync_jobs (sync_rule_id, frameio_file_id, status, retry_count) VALUES ($1,$2,'pending',0) RETURNING id",
            )
            .bind(rule_id)
            .bind(file_id)
            .fetch_one(db)
            .await
            .unwrap_or(Uuid::nil());
            if id != Uuid::nil() {
                created.push(id);
            }
        }
    }
    created
}
