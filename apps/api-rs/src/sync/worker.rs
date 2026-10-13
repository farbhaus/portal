
//! Sync worker — port of `portal.sync.worker`. Consumes the Redis queue and runs the crons.

use std::time::{Duration, Instant};

use chrono::Utc;
use uuid::Uuid;

use crate::state::AppState;
use crate::storage::frameio_backend::FrameioStorageBackend;
use crate::sync::events::process_event;
use crate::sync::queue::{self, PROCESS_WEBHOOK_EVENT, RECONCILE_RULE, RUN_SYNC_JOB};
use crate::sync::runner::{self, execute_job, JobError};

/// Run one task from the queue.
async fn run_task(state: AppState, task: String, arg: String) {
    match task.as_str() {
        PROCESS_WEBHOOK_EVENT => {
            if let Ok(event_id) = Uuid::parse_str(&arg) {
                let created = process_event(&state.db, event_id).await;
                let mut redis = state.redis.clone();
                for job_id in created {
                    let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
                }
            }
        }
        RECONCILE_RULE => {
            if let Ok(rule_id) = Uuid::parse_str(&arg) {
                let backend = FrameioStorageBackend::new(state.frameio.clone());
                match runner::reconcile_rule(&state.db, rule_id, &backend, &state.settings).await {
                    Ok(created) => {
                        let mut redis = state.redis.clone();
                        for job_id in created {
                            let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
                        }
                    }
                    Err(e) => tracing::warn!("sync.reconcile.failed {e:?}"),
                }
            }
        }
        RUN_SYNC_JOB => {
            if let Ok(job_id) = Uuid::parse_str(&arg) {
                run_sync_job(&state, job_id).await;
            }
        }
        other => tracing::warn!("worker.unknown_task {other}"),
    }
}

async fn run_sync_job(state: &AppState, job_id: Uuid) {
    let Ok(Some(mut job)) = runner::load_job(&state.db, job_id).await else { return };
    if ["done", "skipped", "dead_letter"].contains(&job.status.as_str()) {
        return;
    }
    job.status = "running".to_string();
    job.started_at = Some(Utc::now());
    let _ = runner::save_job(&state.db, &job).await;

    let backend = FrameioStorageBackend::new(state.frameio.clone());
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .expect("http client");

    match execute_job(&state.db, &mut job, &backend, &http, &state.settings).await {
        Ok(()) => {
            let _ = runner::save_job(&state.db, &job).await;
        }
        Err(JobError::SourceNotReady(msg)) => {
            let now = Utc::now();
            let waiting_since = job.source_wait_started_at.unwrap_or_else(|| {
                job.source_wait_started_at = Some(now);
                now
            });
            let waited = (now - waiting_since).num_seconds();
            let mut redis = state.redis.clone();
            if waited > state.settings.sync_source_ready_timeout_seconds {
                job.status = "skipped".to_string();
                job.error = Some(format!("source never became ready: {msg}"));
                job.completed_at = Some(now);
                let _ = runner::save_job(&state.db, &job).await;
            } else {
                job.status = "waiting".to_string();
                job.error = Some(msg);
                let _ = runner::save_job(&state.db, &job).await;
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), state.settings.sync_source_poll_seconds as f64).await;
            }
        }
        Err(JobError::DestinationUnavailable(msg)) => {
            job.status = "waiting".to_string();
            job.error = Some(msg);
            let _ = runner::save_job(&state.db, &job).await;
            let mut redis = state.redis.clone();
            let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), state.settings.sync_source_poll_seconds as f64).await;
        }
        Err(e) => {
            let error = format!("{e:?}");
            job.retry_count += 1;
            job.error = Some(error.clone());
            let mut redis = state.redis.clone();
            if job.retry_count >= state.settings.sync_max_retries {
                job.status = "dead_letter".to_string();
                let _ = runner::save_job(&state.db, &job).await;
                tracing::warn!("sync.job.dead_letter job_id={job_id} error={error}");
            } else {
                job.status = "pending".to_string();
                let _ = runner::save_job(&state.db, &job).await;
                let defer = (state.settings.sync_retry_base_seconds * 2i64.pow((job.retry_count - 1) as u32)) as f64;
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), defer).await;
            }
        }
    }
}

/// Hourly/interval reconciliation of every enabled rule.
async fn reconcile_all(state: &AppState) -> usize {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sync_rules WHERE enabled")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let mut total = 0;
    for id in ids {
        let backend = FrameioStorageBackend::new(state.frameio.clone());
        if let Ok(created) = runner::reconcile_rule(&state.db, id, &backend, &state.settings).await {
            let mut redis = state.redis.clone();
            for job_id in created {
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
            }
            total += 1;
        }
    }
    total
}

/// Worker entrypoint: reclaim orphans on boot, then drain the queue and run the crons.
pub async fn run(state: AppState) -> anyhow::Result<()> {
    tracing::info!("worker.startup");
    // Re-pick jobs stranded "running" by a previous worker.
    if let Ok(orphans) = runner::reclaim_orphaned_running(&state.db).await {
        let mut redis = state.redis.clone();
        for id in orphans {
            let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &id.to_string(), 0.0).await;
        }
    }

    let reconcile_every = Duration::from_secs((state.settings.sync_reconcile_interval_minutes.max(1) * 60) as u64);
    let mut last_reconcile = Instant::now() - reconcile_every; // reconcile once at startup
    let mut last_abandon = Instant::now();
    let mut last_retention = Instant::now();
    let mut inflight: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    loop {
        inflight.retain(|h| !h.is_finished());
        let capacity = state.settings.sync_max_concurrent_jobs.saturating_sub(inflight.len());
        if capacity > 0 {
            let mut redis = state.redis.clone();
            if let Ok(jobs) = queue::dequeue_due(&mut redis, capacity).await {
                for (task, arg) in jobs {
                    let st = state.clone();
                    inflight.push(tokio::spawn(run_task(st, task, arg)));
                }
            }
        }

        if last_reconcile.elapsed() >= reconcile_every {
            last_reconcile = Instant::now();
            reconcile_all(&state).await;
        }
        if last_abandon.elapsed() >= Duration::from_secs(15 * 60) {
            last_abandon = Instant::now();
            let _ = crate::services::upload_sessions::abandon_stale_upload_sessions(&state.db, &state.settings).await;
        }
        if last_retention.elapsed() >= Duration::from_secs(24 * 60 * 60) {
            last_retention = Instant::now();
            let _ = crate::services::retention::purge_expired(&state.db, &state.settings).await;
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
