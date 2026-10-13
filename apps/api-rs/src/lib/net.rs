
//! Real client address behind the proxy chain — port of `portal.lib.net`.

use axum::http::HeaderMap;

use crate::config::Settings;

pub fn client_ip(headers: &HeaderMap, peer: Option<std::net::SocketAddr>, settings: &Settings) -> String {
    let hops = settings.trusted_proxy_hops;
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if hops > 0 {
            let parts: Vec<&str> = xff.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
            if parts.len() >= hops {
                return parts[parts.len() - hops].to_string();
            }
            tracing::warn!(
                "net.xff_shorter_than_trusted_hops trusted={} entries={}",
                hops,
                parts.len()
            );
        }
    }
    peer.map(|p| p.ip().to_string()).unwrap_or_else(|| "unknown".to_string())
}
