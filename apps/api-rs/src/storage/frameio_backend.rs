
//! Frame.io-backed StorageBackend — port of `portal.storage.frameio_backend`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;

use crate::frameio::{FrameioClient, FrameioError};
use crate::storage::{
    DestinationConfig, DownloadURL, RemoteFile, UploadChunk,
    UploadCredentials, UploadSession,
};

fn split_path(path: &str) -> Result<(Vec<String>, String), String> {
    let parts: Vec<String> = path
        .replace('\\', "/")
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .map(|p| p.to_string())
        .collect();
    if parts.is_empty() {
        return Err("empty upload path".to_string());
    }
    let name = parts.last().cloned().unwrap();
    Ok((parts[..parts.len() - 1].to_vec(), name))
}

pub struct FrameioStorageBackend {
    pub client: Arc<FrameioClient>,
}

impl FrameioStorageBackend {
    pub fn new(client: Arc<FrameioClient>) -> Self {
        Self { client }
    }

    /// Resolve (or create) `dirs` under `root_folder_id` and start a local upload of `name` there.
    async fn upload_into(
        &self,
        account_id: &str,
        root_folder_id: &str,
        dirs: &[String],
        name: &str,
        size: i64,
    ) -> Result<(String, (String, Option<String>, Vec<(i64, String)>)), FrameioError> {
        let folder_id = if dirs.is_empty() {
            root_folder_id.to_string()
        } else {
            self.client.ensure_folder_path(account_id, root_folder_id, dirs).await?
        };
        let upload = self.client.create_local_upload(account_id, &folder_id, name, size).await?;
        Ok((folder_id, upload))
    }

    pub async fn create_upload_session(
        &self,
        dest: &DestinationConfig,
        filename: &str,
        size: i64,
    ) -> Result<UploadSession, FrameioError> {
        let account_id = dest.require("account_id").map_err(FrameioError::Other)?;
        let root_folder_id = dest.require("folder_id").map_err(FrameioError::Other)?;
        let (dirs, name) = split_path(filename).map_err(FrameioError::Other)?;
        let (folder_id, (file_id, media_type, chunks)) =
            match self.upload_into(&account_id, &root_folder_id, &dirs, &name, size).await {
                // Subfolder ids are cached for the life of the process. If one has since been
                // deleted on Frame.io, forget the path and resolve it again (recreating it).
                Err(FrameioError::NotFound) if !dirs.is_empty() => {
                    self.client.forget_folder_path(&root_folder_id, &dirs).await;
                    self.upload_into(&account_id, &root_folder_id, &dirs, &name, size).await?
                }
                r => r?,
            };
        Ok(UploadSession {
            id: file_id.clone(),
            destination: dest.clone(),
            filename: name,
            size,
            backend_data: json!({
                "file_id": file_id,
                "media_type": media_type,
                "folder_id": folder_id,
                "chunks": chunks.iter().map(|(s, u)| json!({"size": s, "url": u})).collect::<Vec<_>>(),
            }),
        })
    }

    pub fn get_upload_credentials(&self, session: &UploadSession) -> UploadCredentials {
        let chunks = session
            .backend_data
            .get("chunks")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let media_type = session.backend_data.get("media_type").and_then(|v| v.as_str());
        let mut headers = HashMap::new();
        headers.insert("x-amz-acl".to_string(), "private".to_string());
        if let Some(mt) = media_type {
            headers.insert("Content-Type".to_string(), mt.to_string());
        }
        UploadCredentials {
            upload_session_id: session.id.clone(),
            chunks: chunks
                .iter()
                .enumerate()
                .map(|(i, c)| UploadChunk {
                    part: i as i64 + 1,
                    size: c.get("size").and_then(|v| v.as_i64()).unwrap_or(0),
                    url: c.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                })
                .collect(),
            headers,
        }
    }

    pub async fn complete_upload(&self, session: &UploadSession) -> Result<RemoteFile, FrameioError> {
        let account_id = session.destination.require("account_id").map_err(FrameioError::Other)?;
        let file_id = session
            .backend_data
            .get("file_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let (complete, failed) = self.client.get_upload_status(&account_id, &file_id).await?;
        if failed {
            return Err(FrameioError::Other(format!("Upload failed for file {file_id}")));
        }
        if !complete {
            // Callers detect this marker with `is_not_ready` and report "not ready yet".
            return Err(FrameioError::Other(format!("__not_ready__{file_id}")));
        }
        Ok(RemoteFile {
            id: file_id,
            name: session.filename.clone(),
            size: Some(session.size),
            ..Default::default()
        })
    }

    pub async fn get_download_url(&self, dest: &DestinationConfig, file_id: &str) -> Result<DownloadURL, FrameioError> {
        let account_id = dest.require("account_id").map_err(FrameioError::Other)?;
        let url = self.client.get_download_url(&account_id, file_id).await?;
        Ok(DownloadURL { url })
    }

}

/// Marker error used to translate `complete_upload`'s not-ready case.
pub fn is_not_ready(e: &FrameioError) -> Option<String> {
    if let FrameioError::Other(msg) = e
        && let Some(id) = msg.strip_prefix("__not_ready__") {
            return Some(id.to_string());
        }
    None
}

