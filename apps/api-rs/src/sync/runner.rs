
//! Sync job execution + reconciliation — port of `portal.sync.runner`.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::Settings;
use crate::frameio::FrameioError;
use crate::storage::frameio_backend::FrameioStorageBackend;
use crate::storage::DestinationConfig;
use crate::sync::destination::{arm_destination, check_destination, DestinationUnavailable};
use crate::sync::download::download_to_file;
use crate::sync::paths::{resolve_conflict, resolve_destination, PathContext};

pub const DOWNLOADABLE_STATUSES: [&str; 2] = ["uploaded", "transcoded"];

#[derive(Debug)]
pub enum JobError {
    SourceNotReady(String),
    DestinationUnavailable(String),
    Frameio(FrameioError),
    Other(String),
}

#[derive(Clone)]
pub struct SyncJobRow {
    pub id: Uuid,
    pub sync_rule_id: Uuid,
    pub frameio_file_id: String,
    pub status: String,
    pub bytes: Option<i64>,
    pub sha256: Option<String>,
    pub error: Option<String>,
    pub retry_count: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_wait_started_at: Option<DateTime<Utc>>,
}

pub struct SyncRuleRow {
    pub id: Uuid,
    pub destination_path: String,
    pub conflict_policy: String,
    pub path_template: Option<String>,
    pub source_config: serde_json::Value,
    pub frameio_webhook_id: Option<String>,
    pub webhook_secret: Option<String>,
    pub enabled: bool,
}

pub async fn load_rule(db: &PgPool, id: Uuid) -> sqlx::Result<Option<SyncRuleRow>> {
    let row = sqlx::query(
        "SELECT id, destination_path, conflict_policy, path_template, source_config,
                frameio_webhook_id, webhook_secret, enabled FROM sync_rules WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| SyncRuleRow {
        id: r.get("id"),
        destination_path: r.get("destination_path"),
        conflict_policy: r.get("conflict_policy"),
        path_template: r.get("path_template"),
        source_config: r.get("source_config"),
        frameio_webhook_id: r.get("frameio_webhook_id"),
        webhook_secret: r.get("webhook_secret"),
        enabled: r.get("enabled"),
    }))
}

pub async fn load_job(db: &PgPool, id: Uuid) -> sqlx::Result<Option<SyncJobRow>> {
    let row = sqlx::query(
        "SELECT id, sync_rule_id, frameio_file_id, status, bytes, sha256, error, retry_count,
                started_at, completed_at, created_at, updated_at, source_wait_started_at
           FROM sync_jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| SyncJobRow {
        id: r.get("id"),
        sync_rule_id: r.get("sync_rule_id"),
        frameio_file_id: r.get("frameio_file_id"),
        status: r.get("status"),
        bytes: r.get("bytes"),
        sha256: r.get("sha256"),
        error: r.get("error"),
        retry_count: r.get("retry_count"),
        started_at: r.get("started_at"),
        completed_at: r.get("completed_at"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        source_wait_started_at: r.get("source_wait_started_at"),
    }))
}

pub async fn save_job(db: &PgPool, job: &SyncJobRow) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE sync_jobs SET status=$1, bytes=$2, sha256=$3, error=$4, retry_count=$5,
            started_at=$6, completed_at=$7, source_wait_started_at=$8, updated_at=now()
         WHERE id=$9",
    )
    .bind(&job.status)
    .bind(job.bytes)
    .bind(&job.sha256)
    .bind(&job.error)
    .bind(job.retry_count)
    .bind(job.started_at)
    .bind(job.completed_at)
    .bind(job.source_wait_started_at)
    .bind(job.id)
    .execute(db)
    .await?;
    Ok(())
}

pub fn destination(rule: &SyncRuleRow) -> DestinationConfig {
    let t = rule.source_config.get("type").and_then(|v| v.as_str()).unwrap_or("frameio");
    DestinationConfig::new(t, rule.source_config.clone())
}

pub fn is_orphaned(job: &SyncJobRow, now: DateTime<Utc>, stale_after_seconds: i64) -> bool {
    if job.status != "running" {
        return false;
    }
    match job.started_at {
        Some(started) => (now - started).num_seconds() > stale_after_seconds,
        None => false,
    }
}

pub fn is_stranded_pending(job: &SyncJobRow, now: DateTime<Utc>, stale_after_seconds: i64) -> bool {
    if job.status != "pending" || job.started_at.is_some() {
        return false;
    }
    (now - job.updated_at).num_seconds() > stale_after_seconds
}

fn in_scope(rule: &SyncRuleRow, parent_id: Option<&str>) -> bool {
    let recursive = rule.source_config.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true);
    if recursive {
        return true;
    }
    parent_id == rule.source_config.get("folder_id").and_then(|v| v.as_str())
}

/// Run one sync job to completion, setting its terminal fields.
pub async fn execute_job(
    db: &PgPool,
    job: &mut SyncJobRow,
    backend: &FrameioStorageBackend,
    http: &reqwest::Client,
    settings: &Settings,
) -> Result<(), JobError> {
    let Some(rule) = load_rule(db, job.sync_rule_id).await.map_err(|e| JobError::Other(e.to_string()))? else {
        return Err(JobError::Other(format!("sync rule {} is gone", job.sync_rule_id)));
    };

    let armed: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sync_jobs WHERE sync_rule_id = $1 AND status = 'done' LIMIT 1",
    )
    .bind(rule.id)
    .fetch_optional(db)
    .await
    .unwrap_or(None);
    if let Err(e) = check_destination(&rule.destination_path, armed.is_some()) {
        return Err(JobError::DestinationUnavailable(e.to_string()));
    }

    let dest = destination(&rule);
    let remote = match backend.get_file(&dest, &job.frameio_file_id).await {
        Ok(f) => f,
        Err(FrameioError::NotFound) => {
            job.status = "skipped".to_string();
            job.completed_at = Some(Utc::now());
            job.error = Some("source file no longer exists on Frame.io".to_string());
            return Ok(());
        }
        Err(e) => return Err(JobError::Frameio(e)),
    };

    if let Some(status) = &remote.status {
        if !DOWNLOADABLE_STATUSES.contains(&status.as_str()) {
            return Err(JobError::SourceNotReady(format!("file status is '{status}', not yet downloadable")));
        }
    }

    if !in_scope(&rule, remote.parent_id.as_deref()) {
        job.status = "skipped".to_string();
        job.completed_at = Some(Utc::now());
        return Ok(());
    }

    let ctx = PathContext {
        filename: remote.name.clone(),
        project: rule.source_config.get("project_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        folder: rule.source_config.get("folder_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        subfolder: remote.relative_dir.clone(),
        uploader_name: String::new(),
        when: Some(chrono::Local::now()),
    };
    let target = resolve_destination(&rule.destination_path, rule.path_template.as_deref(), &ctx);
    let Some(final_path) = resolve_conflict(&target, &rule.conflict_policy) else {
        job.status = "skipped".to_string();
        job.completed_at = Some(Utc::now());
        return Ok(());
    };

    let download = match backend.get_download_url(&dest, &job.frameio_file_id).await {
        Ok(d) => d,
        Err(FrameioError::Forbidden) => {
            return Err(JobError::SourceNotReady("original not downloadable yet (403)".into()))
        }
        Err(e) => return Err(JobError::Frameio(e)),
    };

    let mut tmp = final_path.clone();
    let name = final_path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    tmp.set_file_name(format!("{name}.part"));
    let result = download_to_file(http, &download.url, remote.size, &tmp, settings.sync_download_concurrency).await;
    let result = match result {
        Ok(r) => {
            std::fs::rename(&tmp, &final_path).map_err(|e| JobError::Other(e.to_string()))?;
            r
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(JobError::Other(e.to_string()));
        }
    };

    job.status = "done".to_string();
    job.bytes = Some(result.bytes_written);
    job.sha256 = Some(result.sha256.clone());
    job.completed_at = Some(Utc::now());
    job.error = None;
    arm_destination(&rule.destination_path);
    Ok(())
}

/// Re-pick every job left `running` (worker died mid-download) and re-enqueue it.
pub async fn reclaim_orphaned_running(db: &PgPool) -> sqlx::Result<Vec<Uuid>> {
    let rows = sqlx::query("SELECT id FROM sync_jobs WHERE status = 'running'")
        .fetch_all(db)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.get("id")).collect();
    for id in &ids {
        sqlx::query(
            "UPDATE sync_jobs SET status='pending', started_at=NULL, completed_at=NULL, error=NULL, updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .execute(db)
        .await?;
    }
    Ok(ids)
}

/// Create + enqueue jobs for files in a rule's folder that lack one. Returns the count.
pub async fn reconcile_rule(
    db: &PgPool,
    rule_id: Uuid,
    backend: &FrameioStorageBackend,
    settings: &Settings,
) -> Result<Vec<Uuid>, JobError> {
    let Some(rule) = load_rule(db, rule_id).await.map_err(|e| JobError::Other(e.to_string()))? else {
        return Ok(Vec::new());
    };
    if !rule.enabled {
        return Ok(Vec::new());
    }
    let dest = destination(&rule);
    let recursive = rule.source_config.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true);
    let files = if recursive {
        backend.list_files_recursive(&dest).await.map_err(JobError::Frameio)?
    } else {
        backend.list_files_in_destination(&dest).await.map_err(JobError::Frameio)?
    };

    let now = Utc::now();
    let stale_after = settings.sync_job_timeout_seconds;
    let pending_stale_after = settings.sync_pending_stale_seconds;
    let mut created: Vec<Uuid> = Vec::new();

    for f in files {
        let existing = sqlx::query(
            "SELECT id, status, started_at, updated_at FROM sync_jobs WHERE sync_rule_id = $1 AND frameio_file_id = $2",
        )
        .bind(rule.id)
        .bind(&f.id)
        .fetch_optional(db)
        .await
        .map_err(|e| JobError::Other(e.to_string()))?;

        match existing {
            None => {
                let id: Uuid = sqlx::query_scalar(
                    "INSERT INTO sync_jobs (sync_rule_id, frameio_file_id, status, retry_count) VALUES ($1,$2,'pending',0) RETURNING id",
                )
                .bind(rule.id)
                .bind(&f.id)
                .fetch_one(db)
                .await
                .map_err(|e| JobError::Other(e.to_string()))?;
                created.push(id);
            }
            Some(row) => {
                let job = SyncJobRow {
                    id: row.get("id"),
                    sync_rule_id: rule.id,
                    frameio_file_id: f.id.clone(),
                    status: row.get("status"),
                    bytes: None,
                    sha256: None,
                    error: None,
                    retry_count: 0,
                    started_at: row.get("started_at"),
                    completed_at: None,
                    created_at: now,
                    updated_at: row.get("updated_at"),
                    source_wait_started_at: None,
                };
                if job.status == "dead_letter"
                    || is_orphaned(&job, now, stale_after)
                    || is_stranded_pending(&job, now, pending_stale_after)
                {
                    sqlx::query(
                        "UPDATE sync_jobs SET status='pending', retry_count=0, error=NULL, started_at=NULL, completed_at=NULL, source_wait_started_at=NULL, updated_at=now() WHERE id=$1",
                    )
                    .bind(job.id)
                    .execute(db)
                    .await
                    .map_err(|e| JobError::Other(e.to_string()))?;
                    created.push(job.id);
                }
            }
        }
    }
    Ok(created)
}
