
//! Download-link helpers — port of `portal.downloads.links`.

use futures::stream::{FuturesUnordered, StreamExt};

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
}

pub struct ResolvedSource {
    pub account_id: String,
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
            let mut futs = FuturesUnordered::new();
            for fid in ids {
                let account = account_id.clone();
                futs.push(async move { (fid.clone(), client.get_file(&account, &fid).await) });
            }
            let mut results: Vec<(String, Result<serde_json::Value, FrameioError>)> = Vec::new();
            // Bounded concurrency: drain in windows of SELECTION_CONCURRENCY.
            let _ = SELECTION_CONCURRENCY;
            while let Some(r) = futs.next().await {
                results.push(r);
            }
            let mut out = Vec::new();
            // Preserve input order.
            for (want, _) in source.get("file_ids").and_then(|v| v.as_array()).map(|a| a.iter().enumerate().map(|(i, v)| (v.as_str().unwrap_or("").to_string(), i)).collect::<Vec<_>>()).unwrap_or_default() {
                if let Some((_, v)) = results.iter().find(|(id, _)| id == &want) {
                    out.push(file_from_value(v.as_ref().map_err(|e| FrameioError::Other(format!("{e:?}")))?));
                }
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
    Ok(ResolvedSource { account_id, files })
}

/// Per-link text for a download link (logo/accent come from global branding).
pub fn branding(brand_display_name: &Option<String>, brand_subtitle: &Option<String>) -> (String, Option<String>) {
    let name = brand_display_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Download".to_string());
    (name, brand_subtitle.clone())
}
