
//! SPA navigation guard — the Rust equivalent of the SvelteKit `(admin)/+layout.server.ts` redirect.
//!
//! With SSR, an unauthenticated request to an admin page was answered server-side with `302
//! /login` before any HTML was sent. A static SPA removed that, so this restores it: the shell is
//! served only for public pages or with a valid, active session.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::session::SessionHandle;
use crate::state::AppState;

/// Pages anyone may load. `/u/` and `/d/` are the branded client-facing pages; `/login` and
/// `/privacy` are public by design; the rest are static build assets.
pub fn is_public(path: &str) -> bool {
    path == "/login"
        || path == "/privacy"
        || path.starts_with("/u/")
        || path.starts_with("/d/")
        || path.starts_with("/_app/")
        || path.starts_with("/fonts/")
        || path == "/favicon.svg"
        || path == "/favicon.png"
        || path == "/service-worker.js"
}

/// `Some(302 /login)` when the request targets a guarded page without a valid session.
pub async fn maybe_redirect(
    state: &AppState,
    path: &str,
    handle: &SessionHandle,
) -> Option<Response> {
    if path.starts_with("/api/") || is_public(path) {
        return None;
    }
    let uid = {
        let mut s = handle.lock().unwrap();
        s.get("user_id")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
    };
    if let Some(uid) = uid
        && let Ok(id) = Uuid::parse_str(&uid) {
            let ok = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id = $1 AND is_active")
                .bind(id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten()
                .is_some();
            if ok {
                return None;
            }
        }
    let mut res = StatusCode::FOUND.into_response();
    res.headers_mut()
        .insert(header::LOCATION, HeaderValue::from_static("/login"));
    // Starlette adds Vary: Cookie when the session was read; match it on the redirect too.
    res.headers_mut()
        .append(header::VARY, HeaderValue::from_static("Cookie"));
    Some(res)
}

/// The SPA shell (`index.html`) as built, read once.
fn shell(web_dir: &str) -> Option<&'static str> {
    static SHELL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    SHELL
        .get_or_init(|| std::fs::read_to_string(format!("{web_dir}/index.html")).ok())
        .as_deref()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// The shell with its `<title>` replaced.
fn with_title(shell: &str, title: &str) -> String {
    match (shell.find("<title>"), shell.find("</title>")) {
        (Some(a), Some(b)) if a < b => format!("{}<title>{}{}", &shell[..a], escape_html(title), &shell[b..]),
        _ => shell.to_string(),
    }
}

#[derive(Clone, Copy)]
pub enum PublicPage {
    Upload,
    Download,
}

/// The branded name a public link page shows (its `<title>`), or None for an unknown token —
/// the same names `GET /api/public/links/{token}` and `/api/public/downloads/{token}` return.
async fn public_link_title(state: &AppState, page: PublicPage, token: &str) -> Option<String> {
    let nonempty = |v: Option<String>| v.filter(|s| !s.is_empty());
    let kind = match page {
        PublicPage::Upload => "upload",
        PublicPage::Download => "download",
    };
    let row: (Option<String>,) = sqlx::query_as(
        "SELECT brand_display_name FROM links WHERE kind = $1 AND token = $2",
    )
    .bind(kind)
    .bind(token)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()?;
    let brand = crate::services::runtime_config::link_brand(&state.db, &state.settings).await;
    let fallback = if kind == "upload" { "Upload" } else { "Download" };
    Some(nonempty(row.0).or(nonempty(brand.name)).unwrap_or_else(|| fallback.into()))
}

/// Serve the shell for `/u/{token}` and `/d/{token}` the way server rendering did: titled with
/// the link's branded name (what chat apps and mail clients show in a link preview), and with a
/// 404 status for a link that doesn't exist. The page itself still renders client-side.
pub async fn public_page(
    state: &AppState,
    page: PublicPage,
    token: &str,
    headers: &axum::http::HeaderMap,
    peer: std::net::SocketAddr,
) -> Response {
    let web_dir = std::env::var("WEB_DIR").unwrap_or_else(|_| "web/build".to_string());
    let Some(shell) = shell(&web_dir) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Its own budget, so a page view doesn't spend the API call's; once spent, skip the lookup.
    let mut redis = state.redis.clone();
    let within_budget = crate::common::ratelimit::check(&mut redis, &state.settings, "page", token, headers, Some(peer))
        .await
        .is_ok();
    let (status, body) = match within_budget.then_some(()) {
        Some(()) => match public_link_title(state, page, token).await {
            Some(title) => (StatusCode::OK, with_title(shell, &title)),
            None => (StatusCode::NOT_FOUND, shell.to_string()),
        },
        None => (StatusCode::OK, shell.to_string()),
    };
    let mut res = (status, axum::response::Html(body)).into_response();
    // The shell names the current build's assets; never let a stale copy outlive a deploy.
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res
}

pub async fn upload_page(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    public_page(&state, PublicPage::Upload, &token, &headers, peer).await
}

pub async fn download_page(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    public_page(&state, PublicPage::Download, &token, &headers, peer).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_the_shell_and_escapes_the_name() {
        let shell = "<head><title>Portal</title></head><body></body>";
        assert_eq!(with_title(shell, "Acme Rushes"), "<head><title>Acme Rushes</title></head><body></body>");
        assert_eq!(with_title(shell, "<b>&Co"), "<head><title>&lt;b&gt;&amp;Co</title></head><body></body>");
        assert_eq!(with_title("<head></head>", "X"), "<head></head>");
    }
}
