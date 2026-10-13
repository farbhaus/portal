
//! Daily data-retention sweep — port of `portal.services.retention`. sync_jobs are never pruned.

use sqlx::PgPool;

use crate::config::Settings;

pub async fn purge_expired(db: &PgPool, settings: &Settings) -> std::collections::HashMap<String, u64> {
    let mut deleted = std::collections::HashMap::new();
    let sweeps: [(&str, &str, &str, i64); 4] = [
        ("webhook_events", "webhook_events", "received_at", settings.retention_webhook_events_days),
        ("upload_sessions", "upload_sessions", "created_at", settings.retention_sessions_days),
        ("download_sessions", "download_sessions", "created_at", settings.retention_sessions_days),
        ("audit_log", "audit_log", "created_at", settings.retention_audit_log_days),
    ];
    for (label, table, column, days) in sweeps {
        if days <= 0 {
            continue;
        }
        let q = sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE {column} < now() - ($1 || ' days')::interval"
        ));
        if let Ok(res) = sqlx::query(q).bind(days.to_string()).execute(db).await {
            deleted.insert(label.to_string(), res.rows_affected());
        }
    }
    if deleted.values().any(|v| *v > 0) {
        tracing::info!("retention.purged {:?}", deleted);
    }
    deleted
}
