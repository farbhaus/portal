
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
    if let Some(uid) = uid {
        if let Ok(id) = Uuid::parse_str(&uid) {
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
    }
    let mut res = StatusCode::FOUND.into_response();
    res.headers_mut()
        .insert(header::LOCATION, HeaderValue::from_static("/login"));
    // Starlette adds Vary: Cookie when the session was read; match it on the redirect too.
    res.headers_mut()
        .append(header::VARY, HeaderValue::from_static("Cookie"));
    Some(res)
}
