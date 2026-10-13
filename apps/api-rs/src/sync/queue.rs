
//! Redis-backed delayed job queue for the sync worker.
//!
//! Python used arq; both ends here are Rust, so this is a small sorted-set queue keyed by
//! ready-time. Tasks are `{"task": "...", "arg": "..."}` members scored by ready-at millis.

use redis::AsyncCommands;
use serde_json::json;

pub const PROCESS_WEBHOOK_EVENT: &str = "process_webhook_event";
pub const RUN_SYNC_JOB: &str = "run_sync_job";
pub const RECONCILE_RULE: &str = "reconcile_rule";

const QUEUE_KEY: &str = "portal:sync_jobs";

pub async fn enqueue(
    redis: &mut redis::aio::ConnectionManager,
    task: &str,
    arg: &str,
    delay_secs: f64,
) -> Result<(), redis::RedisError> {
    let ready_at_ms = chrono::Utc::now().timestamp_millis() as f64 + delay_secs * 1000.0;
    let member = json!({ "task": task, "arg": arg }).to_string();
    let _: () = redis.zadd(QUEUE_KEY, member, ready_at_ms).await?;
    Ok(())
}

/// Pop every job due at or before now (up to `max`).
pub async fn dequeue_due(
    redis: &mut redis::aio::ConnectionManager,
    max: usize,
) -> Result<Vec<(String, String)>, redis::RedisError> {
    let now_ms = chrono::Utc::now().timestamp_millis() as f64;
    let members: Vec<String> = redis
        .zrangebyscore_limit(QUEUE_KEY, "-inf", now_ms, 0, max as isize)
        .await?;
    let mut out = Vec::new();
    for m in members {
        let removed: i64 = redis.zrem(QUEUE_KEY, &m).await?;
        if removed == 0 {
            continue; // another consumer took it
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&m) {
            if let (Some(t), Some(a)) = (v.get("task").and_then(|x| x.as_str()), v.get("arg").and_then(|x| x.as_str())) {
                out.push((t.to_string(), a.to_string()));
            }
        }
    }
    Ok(out)
}
