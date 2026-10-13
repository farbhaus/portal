
//! Admin-only endpoints. The `Admin` extractor is the port of `require_admin`: it reads the
//! session (which is what makes Starlette add `Vary: Cookie`) and rejects with the exact 401
//! Portal emits. The authenticated bodies are not ported yet and report that explicitly rather
//! than pretending.

use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::Admin;
use crate::error::detail;

fn not_ported() -> Response {
    detail(StatusCode::NOT_IMPLEMENTED, "endpoint not yet ported")
}

pub async fn privacy_subject(_: Admin) -> Response {
    not_ported()
}
