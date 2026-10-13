
use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Settings;
use crate::frameio::FrameioClient;

/// Shared application state. The Redis client is a `ConnectionManager` that reconnects on its
/// own, matching the Python service's one-client-per-request-but-reconnecting behaviour closely
/// enough for the health probe while being cheaper.
#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub settings: Arc<Settings>,
    pub db: PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub frameio: Arc<FrameioClient>,
}
