
//! `GET /api/branding/logo` — public logo serving, mirroring `portal.routes.branding`.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sqlx::Row;

use crate::error::detail;
use crate::state::AppState;

pub async fn get_logo(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let row = sqlx::query("SELECT logo_png, logo_content_type, logo_updated_at FROM app_settings LIMIT 1")
        .fetch_optional(&state.db)
        .await;
    let Ok(Some(row)) = row else {
        return detail(StatusCode::NOT_FOUND, "No logo set");
    };
    let png: Option<Vec<u8>> = row.get("logo_png");
    let Some(png) = png else {
        return detail(StatusCode::NOT_FOUND, "No logo set");
    };
    let content_type: Option<String> = row.get("logo_content_type");
    let updated: Option<chrono::DateTime<chrono::Utc>> = row.get("logo_updated_at");
    let etag = updated.map(|d| format!("\"{}\"", d.timestamp()));

    if let Some(etag) = &etag {
        if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
    }

    let media_type = content_type.unwrap_or_else(|| "image/png".to_string());
    let mut response = png.into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(&media_type).unwrap(),
    );
    h.insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("public, max-age=300"),
    );
    if let Some(etag) = etag {
        if let Ok(v) = axum::http::HeaderValue::from_str(&etag) {
            h.insert(header::ETAG, v);
        }
    }
    response
}
