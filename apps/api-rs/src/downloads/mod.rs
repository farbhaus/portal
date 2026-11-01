
//! Download-link helpers — port of `portal.downloads.links`.

use futures::stream::StreamExt;

use crate::frameio::{FrameioClient, FrameioError};

const SELECTION_CONCURRENCY: usize = 8;

#[derive(Clone, Debug)]
pub struct ResolvedFile {
    pub id: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub media_type: Option<String>,
    pub thumbnail_url: Option<String>,
    pub path: String,
    /// Frame.io's upload status, when the listing carries it ("uploaded", "transcoded", …).
    pub status: Option<String>,
}

/// Frame.io upload statuses whose original can be downloaded.
const DOWNLOADABLE_STATUSES: [&str; 2] = ["uploaded", "transcoded"];

impl ResolvedFile {
    /// Whether Frame.io can serve the original yet (unknown status counts as ready).
    pub fn ready(&self) -> bool {
        self.status.as_deref().is_none_or(|s| DOWNLOADABLE_STATUSES.contains(&s))
    }
}

pub struct ResolvedSource {
    pub files: Vec<ResolvedFile>,
}

fn file_from_value(v: &serde_json::Value) -> ResolvedFile {
    let thumb = v
        .get("media_links")
        .and_then(|m| m.get("thumbnail"))
        .and_then(|t| t.get("download_url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    ResolvedFile {
        id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        name: v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        file_size: v.get("file_size").and_then(|x| x.as_i64()),
        media_type: v.get("media_type").and_then(|x| x.as_str()).map(|s| s.to_string()),
        thumbnail_url: thumb,
        path: v.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        status: v.get("status").and_then(|x| x.as_str()).map(|s| s.to_string()),
    }
}

/// The file with this id within the link's source, or None if it isn't part of it — port of
/// `routes.public_downloads._resolve_file`, the live check for sessions without a snapshot.
pub async fn resolve_file(client: &FrameioClient, source: &serde_json::Value, file_id: &str) -> Result<Option<ResolvedFile>, FrameioError> {
    let account_id = source.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    match source.get("type").and_then(|v| v.as_str()) {
        Some("file") => {
            if source.get("file_id").and_then(|v| v.as_str()) != Some(file_id) {
                return Ok(None);
            }
            Ok(Some(file_from_value(&client.get_file(account_id, file_id).await?)))
        }
        Some("selection") => {
            let listed = source
                .get("file_ids")
                .and_then(|v| v.as_array())
                .is_some_and(|ids| ids.iter().any(|v| v.as_str() == Some(file_id)));
            if !listed {
                return Ok(None);
            }
            Ok(Some(file_from_value(&client.get_file(account_id, file_id).await?)))
        }
        Some("folder") => Ok(resolve_source(client, source).await?.files.into_iter().find(|f| f.id == file_id)),
        _ => Ok(None),
    }
}

/// Resolve a link's source into concrete downloadable files.
pub async fn resolve_source(client: &FrameioClient, source: &serde_json::Value) -> Result<ResolvedSource, FrameioError> {
    let account_id = source.get("account_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let stype = source.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let files = match stype {
        "file" => {
            let fid = source.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
            vec![file_from_value(&client.get_file(&account_id, fid).await?)]
        }
        "selection" => {
            let ids: Vec<String> = source
                .get("file_ids")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            // At most SELECTION_CONCURRENCY lookups in flight; `buffered` yields results in input
            // order, so the listing matches the curated selection order (Python: a semaphore +
            // gather). The first failure propagates as-is.
            let fetched: Vec<Result<serde_json::Value, FrameioError>> = futures::stream::iter(ids)
                .map(|fid| {
                    let account = account_id.clone();
                    async move { client.get_file(&account, &fid).await }
                })
                .buffered(SELECTION_CONCURRENCY)
                .collect()
                .await;
            let mut out = Vec::with_capacity(fetched.len());
            for f in fetched {
                out.push(file_from_value(&f?));
            }
            out
        }
        "folder" => {
            let folder_id = source.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
            let recursive = source.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
            let listing = if recursive {
                client.list_files_recursive(&account_id, folder_id).await?
            } else {
                client.list_files(&account_id, folder_id).await?
                    .into_iter()
                    .map(|(id, name, size, media)| serde_json::json!({
                        "id": id, "name": name, "file_size": size, "media_type": media, "path": ""
                    }))
                    .collect()
            };
            listing.iter().map(file_from_value).collect()
        }
        other => return Err(FrameioError::Other(format!("unknown source type: {other:?}"))),
    };
    Ok(ResolvedSource { files })
}

/// Per-link text for a download link (logo/accent come from global branding).
pub fn branding(brand_display_name: &Option<String>, brand_subtitle: &Option<String>) -> (String, Option<String>) {
    let name = brand_display_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Download".to_string());
    (name, brand_subtitle.clone())
}
