
//! Frame.io integration: the one client module, its OAuth, and its connection lifecycle.
//! Mirrors `portal.frameio.*`.

mod client;
pub mod connection;
pub mod oauth;
pub mod webhooks;

pub use client::FrameioClient;

/// Frame.io accepts either a Frame.io account id or an Adobe id in the account path segment.
pub type FrameOrAdobeId = String;

#[derive(Debug, thiserror::Error)]
pub enum FrameioError {
    #[error("no usable Frame.io connection")]
    NotConnected,
    #[error("Frame.io is rate-limiting")]
    RateLimited,
    #[error("Frame.io entity not found")]
    NotFound,
    #[error("Frame.io forbidden (not ready?)")]
    Forbidden,
    #[error("{0}")]
    Other(String),
}

impl FrameioError {
    /// Map to the API status a route should surface, as the Python routes do.
    pub fn is_not_connected(&self) -> bool {
        matches!(self, FrameioError::NotConnected)
    }
}

#[derive(Clone, Debug)]
pub struct PickerItem {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectItem {
    pub id: String,
    pub name: String,
    pub root_folder_id: Option<String>,
    pub workspace_id: Option<String>,
}
