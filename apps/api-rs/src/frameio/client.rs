
//! Frame.io V4 client — port of `portal.frameio.client` (the one Frame.io client module).
//!
//! Calls the same REST endpoints the `frameio` Python SDK issues (the SDK is a generated Fern
//! client; the paths were extracted from it), with the same retry/rate-limit behavior, the same
//! error taxonomy, and the same folder caches. Auth is supplied per request via the connection's
//! refresh-on-expiry token.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::config::Settings;
use crate::frameio::connection::{get_primary_connection, get_valid_access_token, ConnectionRow};
use crate::frameio::{FrameioError, FrameOrAdobeId, PickerItem, ProjectItem};

const MAX_RETRIES: u32 = 4;
const PICKER_PAGE_SIZE: usize = 100;
const PICKER_MAX_ITEMS: usize = 1000;
const MAX_FOLDER_DEPTH: usize = 64;
const API_BASE: &str = "https://api.frame.io/v4";

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}
fn i(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(|x| x.as_i64())
}
fn arr(v: &Value, k: &str) -> Vec<Value> {
    v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
}

pub struct FrameioClient {
    http: reqwest::Client,
    db: PgPool,
    settings: Arc<Settings>,
    folder_cache: Mutex<HashMap<(String, String), String>>,
    folder_meta_cache: Mutex<HashMap<String, (String, Option<String>)>>,
}

impl FrameioClient {
    pub fn new(db: PgPool, settings: Arc<Settings>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("http client"),
            db,
            settings,
            folder_cache: Mutex::new(HashMap::new()),
            folder_meta_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Shared HTTP client (used by the OAuth exchange, which is not an API call).
    pub fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    /// Bearer token for the primary connection, refreshing if near expiry.
    async fn access_token(&self) -> Result<String, FrameioError> {
        let conn: ConnectionRow = get_primary_connection(&self.db)
            .await
            .map_err(|e| FrameioError::Other(e.to_string()))?
            .ok_or(FrameioError::NotConnected)?;
        get_valid_access_token(&self.db, &self.settings, &self.http, &conn)
            .await
            .map_err(|_| FrameioError::NotConnected)
    }

    /// One API call with the SDK's retry policy (429/5xx with backoff, honoring Retry-After).
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<Value, FrameioError> {
        let token = self.access_token().await?;
        let url = format!("{API_BASE}/{path}");
        let mut last_status = None;
        for attempt in 0..MAX_RETRIES {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&token)
                .query(query);
            if let Some(b) = &body {
                req = req.json(b);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(FrameioError::Other(e.to_string()));
                    }
                    tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
                    continue;
                }
            };
            let status = resp.status();
            last_status = Some(status);
            if status.is_success() {
                if status == reqwest::StatusCode::NO_CONTENT {
                    return Ok(Value::Null);
                }
                return resp
                    .json::<Value>()
                    .await
                    .or_else(|_| Ok(Value::Null));
            }
            // Transient: retry with backoff.
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if attempt + 1 < MAX_RETRIES {
                    let wait = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(Duration::from_secs)
                        .unwrap_or_else(|| Duration::from_millis(500 * 2u64.pow(attempt)));
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
            break;
        }
        match last_status {
            Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => Err(FrameioError::RateLimited),
            Some(reqwest::StatusCode::NOT_FOUND) => Err(FrameioError::NotFound),
            Some(reqwest::StatusCode::FORBIDDEN) => Err(FrameioError::Forbidden),
            Some(s) => Err(FrameioError::Other(format!("Frame.io returned {s}"))),
            None => Err(FrameioError::Other("Frame.io call failed".into())),
        }
    }

    async fn send_data(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<Value, FrameioError> {
        let v = self.send(method, path, query, body).await?;
        Ok(v.get("data").cloned().unwrap_or(v))
    }

    /// Cursor-paginate an endpoint that returns `{data: [...], links: {next}}`.
    async fn paginate(&self, path: &str, mut query: Vec<(String, String)>) -> Result<Vec<Value>, FrameioError> {
        let mut out = Vec::new();
        loop {
            let v = self.send(reqwest::Method::GET, path, &query, None).await?;
            out.extend(v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default());
            if out.len() >= PICKER_MAX_ITEMS {
                break;
            }
            match v.get("links").and_then(|l| l.get("next")).and_then(|n| n.as_str()) {
                Some(next) if !next.is_empty() => {
                    query.retain(|(k, _)| k != "after");
                    query.push(("after".to_string(), next.to_string()));
                }
                _ => break,
            }
        }
        out.truncate(PICKER_MAX_ITEMS);
        Ok(out)
    }

    // ── pickers ───────────────────────────────────────────────────────────────

    pub async fn list_accounts(&self) -> Result<Vec<PickerItem>, FrameioError> {
        let q = vec![("page_size".into(), PICKER_PAGE_SIZE.to_string())];
        Ok(self
            .paginate("accounts", q)
            .await?
            .iter()
            .map(|a| PickerItem {
                id: s(a, "id").unwrap_or_default(),
                name: s(a, "display_name").or_else(|| s(a, "id")).unwrap_or_default(),
                parent_id: None,
                project_id: None,
            })
            .collect())
    }

    pub async fn list_workspaces(&self, account_id: &str) -> Result<Vec<PickerItem>, FrameioError> {
        let q = vec![("page_size".into(), PICKER_PAGE_SIZE.to_string())];
        Ok(self
            .paginate(&format!("accounts/{account_id}/workspaces"), q)
            .await?
            .iter()
            .map(|w| PickerItem {
                id: s(w, "id").unwrap_or_default(),
                name: s(w, "name").or_else(|| s(w, "id")).unwrap_or_default(),
                parent_id: None,
                project_id: None,
            })
            .collect())
    }

    pub async fn list_projects(
        &self,
        account_id: &str,
        workspace_id: &str,
    ) -> Result<Vec<ProjectItem>, FrameioError> {
        let q = vec![("page_size".into(), PICKER_PAGE_SIZE.to_string())];
        Ok(self
            .paginate(&format!("accounts/{account_id}/workspaces/{workspace_id}/projects"), q)
            .await?
            .iter()
            .map(|p| ProjectItem {
                id: s(p, "id").unwrap_or_default(),
                name: s(p, "name").or_else(|| s(p, "id")).unwrap_or_default(),
                root_folder_id: s(p, "root_folder_id"),
                workspace_id: None,
            })
            .collect())
    }

    pub async fn create_project(
        &self,
        account_id: &str,
        workspace_id: &str,
        name: &str,
        restricted: bool,
    ) -> Result<ProjectItem, FrameioError> {
        let body = json!({"data": {"name": name, "restricted": restricted}});
        let p = self
            .send_data(
                reqwest::Method::POST,
                &format!("accounts/{account_id}/workspaces/{workspace_id}/projects"),
                &[],
                Some(body),
            )
            .await?;
        Ok(ProjectItem {
            id: s(&p, "id").unwrap_or_default(),
            name: s(&p, "name").unwrap_or_else(|| name.to_string()),
            root_folder_id: s(&p, "root_folder_id"),
            workspace_id: None,
        })
    }

    pub async fn delete_project(&self, account_id: &str, project_id: &str) -> Result<(), FrameioError> {
        self.send(
            reqwest::Method::DELETE,
            &format!("accounts/{account_id}/projects/{project_id}"),
            &[],
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn get_project(&self, account_id: &str, project_id: &str) -> Result<ProjectItem, FrameioError> {
        let p = self
            .send_data(
                reqwest::Method::GET,
                &format!("accounts/{account_id}/projects/{project_id}"),
                &[],
                None,
            )
            .await?;
        Ok(ProjectItem {
            id: s(&p, "id").unwrap_or_default(),
            name: s(&p, "name").or_else(|| s(&p, "id")).unwrap_or_default(),
            root_folder_id: s(&p, "root_folder_id"),
            workspace_id: s(&p, "workspace_id"),
        })
    }

    pub async fn get_folder(&self, account_id: &str, folder_id: &str) -> Result<PickerItem, FrameioError> {
        let f = self
            .send_data(
                reqwest::Method::GET,
                &format!("accounts/{account_id}/folders/{folder_id}"),
                &[],
                None,
            )
            .await?;
        Ok(PickerItem {
            id: s(&f, "id").unwrap_or_default(),
            name: s(&f, "name").or_else(|| s(&f, "id")).unwrap_or_default(),
            parent_id: s(&f, "parent_id"),
            project_id: s(&f, "project_id"),
        })
    }

    pub async fn list_subfolders(&self, account_id: &str, folder_id: &str) -> Result<Vec<PickerItem>, FrameioError> {
        let q = vec![("page_size".into(), PICKER_PAGE_SIZE.to_string())];
        Ok(self
            .paginate(&format!("accounts/{account_id}/folders/{folder_id}/folders"), q)
            .await?
            .iter()
            .map(|f| PickerItem {
                id: s(f, "id").unwrap_or_default(),
                name: s(f, "name").or_else(|| s(f, "id")).unwrap_or_default(),
                parent_id: None,
                project_id: None,
            })
            .collect())
    }

    pub async fn create_folder(
        &self,
        account_id: &str,
        parent_folder_id: &str,
        name: &str,
    ) -> Result<PickerItem, FrameioError> {
        let body = json!({"data": {"name": name}});
        let f = self
            .send_data(
                reqwest::Method::POST,
                &format!("accounts/{account_id}/folders/{parent_folder_id}/folders"),
                &[],
                Some(body),
            )
            .await?;
        Ok(PickerItem {
            id: s(&f, "id").unwrap_or_default(),
            name: s(&f, "name").unwrap_or_else(|| name.to_string()),
            parent_id: None,
            project_id: None,
        })
    }

    pub async fn list_files(&self, account_id: &str, folder_id: &str) -> Result<Vec<(String, String, Option<i64>, Option<String>)>, FrameioError> {
        let q = vec![
            ("type".into(), "file".to_string()),
            ("page_size".into(), PICKER_PAGE_SIZE.to_string()),
        ];
        Ok(self
            .paginate(&format!("accounts/{account_id}/folders/{folder_id}/children"), q)
            .await?
            .iter()
            .map(|a| (s(a, "id").unwrap_or_default(), s(a, "name").unwrap_or_default(), i(a, "file_size"), s(a, "media_type")))
            .collect())
    }

    pub async fn delete_file(&self, account_id: &str, file_id: &str) -> Result<(), FrameioError> {
        self.send(reqwest::Method::DELETE, &format!("accounts/{account_id}/files/{file_id}"), &[], None).await.map(|_| ())
    }

    pub async fn delete_folder(&self, account_id: &str, folder_id: &str) -> Result<(), FrameioError> {
        self.send(reqwest::Method::DELETE, &format!("accounts/{account_id}/folders/{folder_id}"), &[], None).await.map(|_| ())
    }

    pub async fn move_file(&self, account_id: &str, file_id: &str, parent_id: &str) -> Result<(), FrameioError> {
        let body = json!({"data": {"parent_id": parent_id}});
        self.send(reqwest::Method::PATCH, &format!("accounts/{account_id}/files/{file_id}/move"), &[], Some(body)).await.map(|_| ())
    }

    pub async fn copy_file(&self, account_id: &str, file_id: &str, parent_id: &str) -> Result<(String, String, Option<i64>, Option<String>), FrameioError> {
        let body = json!({"data": {"parent_id": parent_id}});
        let f = self
            .send_data(reqwest::Method::POST, &format!("accounts/{account_id}/files/{file_id}/copy"), &[], Some(body))
            .await?;
        Ok((s(&f, "id").unwrap_or_default(), s(&f, "name").unwrap_or_default(), i(&f, "file_size"), s(&f, "media_type")))
    }

    pub async fn move_folder(&self, account_id: &str, folder_id: &str, parent_id: &str) -> Result<(), FrameioError> {
        let body = json!({"data": {"parent_id": parent_id}});
        self.send(reqwest::Method::PATCH, &format!("accounts/{account_id}/folders/{folder_id}/move"), &[], Some(body)).await.map(|_| ())
    }

    pub async fn copy_folder(&self, account_id: &str, folder_id: &str, parent_id: &str) -> Result<PickerItem, FrameioError> {
        let body = json!({"data": {"parent_id": parent_id}});
        let f = self
            .send_data(reqwest::Method::POST, &format!("accounts/{account_id}/folders/{folder_id}/copy"), &[], Some(body))
            .await?;
        Ok(PickerItem {
            id: s(&f, "id").unwrap_or_default(),
            name: s(&f, "name").or_else(|| s(&f, "id")).unwrap_or_default(),
            parent_id: None,
            project_id: None,
        })
    }

    /// Walk up `parent_id` to the rule's root, returning the relative folder path ("" at root).
    pub async fn folder_relative_path(&self, account_id: &str, parent_id: Option<&str>, root_folder_id: &str) -> Result<String, FrameioError> {
        let Some(parent_id) = parent_id else { return Ok(String::new()) };
        if parent_id == root_folder_id {
            return Ok(String::new());
        }
        let mut segments: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut current = Some(parent_id.to_string());
        for _ in 0..MAX_FOLDER_DEPTH {
            let Some(cur) = current.clone() else { break };
            if cur == root_folder_id || seen.contains(&cur) {
                break;
            }
            seen.push(cur.clone());
            let (name, next_parent) = self.folder_meta(account_id, &cur).await?;
            segments.push(name);
            current = next_parent;
        }
        segments.reverse();
        Ok(segments.join("/"))
    }

    /// Folder ancestry, top-most first, ending at `folder_id`.
    pub async fn folder_ancestry(&self, account_id: &str, folder_id: &str) -> Result<Vec<PickerItem>, FrameioError> {
        let mut items: Vec<PickerItem> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut current = Some(folder_id.to_string());
        for _ in 0..MAX_FOLDER_DEPTH {
            let Some(cur) = current.clone() else { break };
            if seen.contains(&cur) {
                break;
            }
            seen.push(cur.clone());
            let (name, parent) = self.folder_meta(account_id, &cur).await?;
            items.push(PickerItem {
                id: cur,
                name,
                parent_id: parent.clone(),
                project_id: None,
            });
            current = parent;
        }
        items.reverse();
        Ok(items)
    }

    async fn folder_meta(&self, account_id: &str, folder_id: &str) -> Result<(String, Option<String>), FrameioError> {
        if let Some(cached) = self.folder_meta_cache.lock().await.get(folder_id).cloned() {
            return Ok(cached);
        }
        let folder = self.get_folder(account_id, folder_id).await?;
        let meta = (folder.name, folder.parent_id);
        self.folder_meta_cache.lock().await.insert(folder_id.to_string(), meta.clone());
        Ok(meta)
    }

    /// Create a file placeholder + presigned chunk URLs.
    pub async fn create_local_upload(&self, account_id: &str, folder_id: &str, name: &str, file_size: i64) -> Result<(String, Option<String>, Vec<(i64, String)>), FrameioError> {
        let body = json!({"data": {"name": name, "file_size": file_size}});
        let f = self
            .send_data(reqwest::Method::POST, &format!("accounts/{account_id}/folders/{folder_id}/files/local_upload"), &[], Some(body))
            .await?;
        let chunks = f
            .get("upload_urls")
            .and_then(|u| u.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|u| (i(u, "size").unwrap_or(0), s(u, "url").unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        Ok((s(&f, "id").unwrap_or_default(), s(&f, "media_type"), chunks))
    }

    pub async fn get_upload_status(&self, account_id: &str, file_id: &str) -> Result<(bool, bool), FrameioError> {
        let d = self
            .send_data(reqwest::Method::GET, &format!("accounts/{account_id}/files/{file_id}/status"), &[], None)
            .await?;
        Ok((
            d.get("upload_complete").and_then(|v| v.as_bool()).unwrap_or(false),
            d.get("upload_failed").and_then(|v| v.as_bool()).unwrap_or(false),
        ))
    }

    /// Resolve-or-create a nested subfolder path under `root_folder_id`; returns the leaf id.
    pub async fn ensure_folder_path(&self, account_id: &str, root_folder_id: &str, parts: &[String]) -> Result<String, FrameioError> {
        let mut current = root_folder_id.to_string();
        for part in parts {
            let key = (current.clone(), part.clone());
            if let Some(cached) = self.folder_cache.lock().await.get(&key).cloned() {
                current = cached;
                continue;
            }
            let existing = self.list_subfolders(account_id, &current).await?;
            let found = existing.into_iter().find(|f| &f.name == part);
            current = match found {
                Some(f) => f.id,
                None => self.create_folder(account_id, &current, part).await?.id,
            };
            self.folder_cache.lock().await.insert(key, current.clone());
        }
        Ok(current)
    }

    /// Mint a short-lived signed download URL (`include=media_links.original`).
    pub async fn get_download_url(&self, account_id: &str, file_id: &str) -> Result<String, FrameioError> {
        let f = self
            .send_data(
                reqwest::Method::GET,
                &format!("accounts/{account_id}/files/{file_id}"),
                &[("include".into(), "media_links.original".into())],
                None,
            )
            .await?;
        f.get("media_links")
            .and_then(|m| m.get("original"))
            .and_then(|o| o.get("download_url"))
            .and_then(|u| u.as_str())
            .map(|u| u.to_string())
            .ok_or_else(|| FrameioError::Other(format!("No download URL available for file {file_id}")))
    }

    /// File metadata + best-effort thumbnail (recipient listing).
    pub async fn get_file(&self, account_id: &str, file_id: &str) -> Result<Value, FrameioError> {
        self.send_data(
            reqwest::Method::GET,
            &format!("accounts/{account_id}/files/{file_id}"),
            &[("include".into(), "media_links.thumbnail".into())],
            None,
        )
        .await
    }

    /// File metadata for the sync worker (no includes).
    pub async fn get_file_detail(&self, account_id: &str, file_id: &str) -> Result<Value, FrameioError> {
        self.send_data(reqwest::Method::GET, &format!("accounts/{account_id}/files/{file_id}"), &[], None).await
    }

    /// All files under a folder, descending into subfolders, tagged with their relative path.
    pub async fn list_files_recursive(&self, account_id: &str, folder_id: &str) -> Result<Vec<Value>, FrameioError> {
        let mut collected: Vec<Value> = Vec::new();
        let mut stack = vec![folder_id.to_string()];
        let mut seen: Vec<String> = Vec::new();
        let mut rel: HashMap<String, String> = HashMap::new();
        rel.insert(folder_id.to_string(), String::new());
        while let Some(current) = stack.pop() {
            if seen.contains(&current) || collected.len() >= PICKER_MAX_ITEMS {
                continue;
            }
            seen.push(current.clone());
            let here = rel.get(&current).cloned().unwrap_or_default();
            for (id, name, size, media_type) in self.list_files(account_id, &current).await? {
                collected.push(json!({
                    "id": id, "name": name, "file_size": size, "media_type": media_type,
                    "path": here,
                }));
            }
            for sf in self.list_subfolders(account_id, &current).await? {
                if !seen.contains(&sf.id) {
                    rel.insert(sf.id.clone(), format!("{here}/{}", sf.name).trim_start_matches('/').to_string());
                    stack.push(sf.id);
                }
            }
        }
        collected.truncate(PICKER_MAX_ITEMS);
        Ok(collected)
    }

    /// Create a workspace webhook; the signing secret is returned only here.
    pub async fn create_webhook(&self, account_id: &str, workspace_id: &str, name: &str, url: &str, events: &[String]) -> Result<(String, String), FrameioError> {
        let body = json!({"data": {"name": name, "url": url, "events": events}});
        let d = self
            .send_data(reqwest::Method::POST, &format!("accounts/{account_id}/workspaces/{workspace_id}/webhooks"), &[], Some(body))
            .await?;
        let secret = s(&d, "secret").ok_or_else(|| FrameioError::Other("Frame.io did not return a webhook signing secret".into()))?;
        Ok((s(&d, "id").unwrap_or_default(), secret))
    }

    pub async fn delete_webhook(&self, account_id: &str, webhook_id: &str) -> Result<(), FrameioError> {
        self.send(reqwest::Method::DELETE, &format!("accounts/{account_id}/webhooks/{webhook_id}"), &[], None).await.map(|_| ())
    }

    /// Create a share over assets in a project; returns the response data.
    pub async fn create_share(
        &self,
        account_id: &str,
        project_id: &str,
        name: &str,
        asset_ids: &[String],
        access: &str,
        downloading_enabled: bool,
        expiration: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<Value, FrameioError> {
        let mut data = json!({
            "type": "asset",
            "name": name,
            "access": access,
            "asset_ids": asset_ids,
            "downloading_enabled": downloading_enabled,
        });
        if let Some(e) = expiration {
            data["expiration"] = json!(e);
        }
        if let Some(p) = passphrase {
            data["passphrase"] = json!(p);
        }
        self.send_data(
            reqwest::Method::POST,
            &format!("accounts/{account_id}/projects/{project_id}/shares"),
            &[],
            Some(json!({"data": data})),
        )
        .await
    }
}

/// SDK type used for account ids (accepts a Frame.io id or an Adobe id).
pub type _Unused = FrameOrAdobeId;
