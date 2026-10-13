
//! Redis-backed fixed-window rate limiting — port of `portal.lib.ratelimit`.
//! No-op when `RATE_LIMIT_ENABLED` is false.

use redis::AsyncCommands;

use crate::config::Settings;
use crate::lib::net::client_ip;

/// Ok(()) unless the per-minute budget for (kind, token, client IP) is exceeded.
pub async fn check(
    redis: &mut redis::aio::ConnectionManager,
    settings: &Settings,
    kind: &str,
    token: &str,
    headers: &axum::http::HeaderMap,
    peer: Option<std::net::SocketAddr>,
) -> Result<(), String> {
    if !settings.rate_limit_enabled {
        return Ok(());
    }
    let per_minute = if kind == "strict" {
        settings.rate_limit_strict_per_minute
    } else {
        settings.rate_limit_default_per_minute
    };
    let ip = client_ip(headers, peer, settings);
    let key = format!("rl:{kind}:{token}:{ip}");
    let count: i64 = redis.incr(&key, 1).await.unwrap_or(0);
    if count == 1 {
        let _: Result<(), _> = redis.expire(&key, 60).await;
    }
    if count > per_minute {
        return Err("Too many requests. Please slow down.".to_string());
    }
    Ok(())
}
