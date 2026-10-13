
//! Error responses shaped exactly like the Python service's.
//!
//! FastAPI emits `{"detail": "..."}` for both its own 404 and Portal's `PortalError` handlers
//! (see `portal.lib.errors`). The byte-for-byte diff compares raw bodies, so the key order and
//! the absence of whitespace matter.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub fn detail(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "detail": message }))).into_response()
}

pub async fn not_found() -> Response {
    detail(StatusCode::NOT_FOUND, "Not Found")
}
