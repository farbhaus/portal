
//! Backend-agnostic storage interface — port of `portal.storage.base`.

pub mod frameio_backend;

use std::collections::HashMap;

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct DestinationConfig {
    pub r#type: String,
    pub data: Value,
}

impl DestinationConfig {
    pub fn new(r#type: &str, data: Value) -> Self {
        Self { r#type: r#type.to_string(), data }
    }
    /// Value of a required string field.
    pub fn require(&self, key: &str) -> Result<String, String> {
        match self.data.get(key).and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => Ok(s.to_string()),
            _ => Err(format!("destination config missing '{key}' for type '{}'", self.r#type)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RemoteFile {
    pub id: String,
    pub name: String,
    pub size: Option<i64>,
    pub media_type: Option<String>,
    pub parent_id: Option<String>,
    pub project_id: Option<String>,
    pub relative_dir: String,
    pub status: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UploadSession {
    pub id: String,
    pub destination: DestinationConfig,
    pub filename: String,
    pub size: i64,
    pub backend_data: Value,
}

#[derive(Clone, Debug)]
pub struct UploadChunk {
    pub part: i64,
    pub size: i64,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct UploadCredentials {
    pub upload_session_id: String,
    pub chunks: Vec<UploadChunk>,
    pub headers: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct DownloadURL {
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct ChangeSubscription {
    pub id: String,
    pub secret: Option<String>,
}

/// `complete_upload` called before the backend finished assembling the file.
#[derive(Debug)]
pub struct UploadNotReady(pub String);
impl std::fmt::Display for UploadNotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "upload not ready: {}", self.0)
    }
}
impl std::error::Error for UploadNotReady {}
