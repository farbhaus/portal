//! Public API description served at `GET /api/openapi.yaml`.

use axum::http::header;
use axum::response::{IntoResponse, Response};

const SPEC: &str = include_str!("../../openapi.yaml");

pub async fn openapi() -> Response {
    ([(header::CONTENT_TYPE, "application/yaml; charset=utf-8")], SPEC).into_response()
}
