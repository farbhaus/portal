
//! Global per-token lockout for link password attempts — port of `portal.lib.lockout`.

use redis::AsyncCommands;

use crate::config::Settings;

fn key(kind: &str, token: &str) -> String {
    format!("pwlock:{kind}:{token}")
}

/// Ok(()) unless this token's failure budget is exhausted (then Err(detail)).
pub async fn check_password_lockout(
    redis: &mut redis::aio::ConnectionManager,
    settings: &Settings,
    kind: &str,
    token: &str,
) -> Result<(), String> {
    if !settings.rate_limit_enabled {
        return Ok(());
    }
    let count: Option<i64> = redis.get(key(kind, token)).await.unwrap_or(None);
    if let Some(c) = count {
        if c >= settings.password_lockout_max_failures {
            return Err("Too many incorrect password attempts — try again later".to_string());
        }
    }
    Ok(())
}

pub async fn register_password_failure(
    redis: &mut redis::aio::ConnectionManager,
    settings: &Settings,
    kind: &str,
    token: &str,
) {
    if !settings.rate_limit_enabled {
        return;
    }
    let k = key(kind, token);
    let count: i64 = redis.incr(&k, 1).await.unwrap_or(0);
    if count == 1 {
        let _: Result<(), _> = redis.expire(&k, settings.password_lockout_window_seconds).await;
    }
}
