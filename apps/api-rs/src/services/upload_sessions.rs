
//! Abandoned-upload sweep — port of `portal.services.upload_sessions`.

use sqlx::PgPool;

use crate::config::Settings;

pub async fn abandon_stale_upload_sessions(db: &PgPool, settings: &Settings) -> u64 {
    if settings.upload_stale_after_minutes <= 0 {
        return 0;
    }
    let res = sqlx::query(
        "UPDATE upload_sessions SET status='abandoned', updated_at=now()
          WHERE status='in_progress'
            AND COALESCE(last_activity_at, started_at, created_at) < now() - ($1 || ' minutes')::interval",
    )
    .bind(settings.upload_stale_after_minutes.to_string())
    .execute(db)
    .await;
    if let Ok(r) = res {
        r.rows_affected()
    } else {
        0
    }
}
