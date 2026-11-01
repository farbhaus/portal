
//! Frame.io integration: the one client module, its OAuth, and its connection lifecycle.
//! Mirrors `portal.frameio.*`.

mod client;
pub mod connection;
pub mod oauth;

pub use client::FrameioClient;

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

#[derive(Clone, Debug)]
pub struct PickerItem {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub project_id: Option<String>,
}

/// A project or folder found by name with `FrameioClient::search`.
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub root_folder_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectItem {
    pub id: String,
    pub name: String,
    pub root_folder_id: Option<String>,
    pub workspace_id: Option<String>,
}
