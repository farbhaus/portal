//! Portal MCP server.
//!
//! A Rust stdio MCP server over Portal's admin HTTP API. Configure with:
//!   PORTAL_URL=https://portal.example.com
//!   PORTAL_API_TOKEN=<the server's PORTAL_API_TOKEN>

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::transport::stdio;
use rmcp::{tool, tool_router, ErrorData, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
struct PortalServer {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl PortalServer {
    fn new() -> anyhow::Result<Self> {
        let base_url = std::env::var("PORTAL_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18080".to_string())
            .trim_end_matches('/')
            .to_string();
        let token = std::env::var("PORTAL_API_TOKEN")
            .map_err(|_| anyhow::anyhow!("PORTAL_API_TOKEN is not set"))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self { http, base_url, token })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        query: &[(&str, String)],
    ) -> Result<Value, ErrorData> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token);
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(format!("Portal request failed: {e}"), None))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| ErrorData::internal_error(format!("Portal response failed: {e}"), None))?;
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
                .unwrap_or(text);
            return Err(ErrorData::internal_error(
                format!("Portal API {status}: {detail}"),
                None,
            ));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "ok": true }));
        }
        serde_json::from_str(&text)
            .map_err(|e| ErrorData::internal_error(format!("Bad JSON from Portal: {e}"), None))
    }
}

/// A link's `folder.sync` after switching FioTransfer on or off: null when off, else what it
/// already stores with the asked-for lanes changed. `imports` turns Imports (`drive`) on or off
/// (omitted: as stored, and on for a link that wasn't synced); `exports_on: Some(false)` turns
/// Exports off; `exports_folder_id` sets the folder Exports go up to. Names on disk are kept, so
/// a folder FioTransfer already made doesn't move; Portal fills a missing one from Settings.
fn with_sync(stored: Option<&Value>, enabled: bool, imports: Option<bool>, exports_on: Option<bool>, exports_folder_id: Option<&str>) -> Value {
    if !enabled {
        return Value::Null;
    }
    let was_synced = stored.is_some_and(|v| v.is_object());
    let mut sync = stored.filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({}));
    let Some(obj) = sync.as_object_mut() else { return sync };
    match imports {
        Some(false) => {
            obj.remove("drive");
        }
        Some(true) => {
            obj.entry("drive").or_insert_with(|| json!({}));
        }
        None if !was_synced => {
            obj.insert("drive".to_string(), json!({}));
        }
        None => {}
    }
    if exports_on == Some(false) {
        obj.remove("exports");
    } else if let Some(id) = exports_folder_id.map(str::trim).filter(|s| !s.is_empty()) {
        let lane = obj.entry("exports").or_insert_with(|| json!({}));
        if !lane.is_object() {
            *lane = json!({});
        }
        lane["folder_id"] = json!(id);
    }
    sync
}

/// A Portal id that goes into a URL path. Portal's ids are UUIDs, and anything else is refused
/// before it reaches the URL: a `../` in an id would otherwise resolve to another endpoint (an
/// "upload link" delete landing on `/api/frameio/projects/{id}`).
fn path_id<'a>(name: &str, value: &'a str) -> Result<&'a str, ErrorData> {
    let v = value.trim();
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    let groups: Vec<&str> = v.split('-').collect();
    let ok = groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| hex(g, n));
    if ok {
        Ok(v)
    } else {
        Err(ErrorData::invalid_params(format!("{name} must be a Portal id (a UUID)"), None))
    }
}

/// `ask_for` as the API's `uploader_fields_required`, refusing a field the upload page doesn't have.
fn uploader_fields(ask_for: &[String]) -> Result<Value, ErrorData> {
    let mut fields = json!({ "name": false, "email": false, "message": false, "project": false });
    for f in ask_for {
        match fields.get_mut(f.trim()) {
            Some(v) => *v = json!(true),
            None => return Err(ErrorData::invalid_params(format!("ask_for: unknown field {f:?} (name, email, message, project)"), None)),
        }
    }
    Ok(fields)
}

fn pretty(value: Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(&value)
        .map_err(|e| ErrorData::internal_error(format!("Could not encode response: {e}"), None))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LinkId {
    link_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateLink {
    link_id: String,
    /// Fields to PATCH. Only supplied keys change. "Sync with FioTransfer" rides in `folder`: send
    /// the link's `folder` with a `sync` block (`{"drive":{}}` for Imports,
    /// `{"exports":{"folder_id":"…"}}` for Exports, both for both), or `sync: null` to stop.
    /// `turbosort` is the project folder. `set_fiotransfer_sync` does this for you.
    fields: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetFiotransferSync {
    link_id: String,
    /// true syncs the link's project folders with FioTransfer; false stops syncing it.
    enabled: bool,
    /// Imports: what the client sends comes down to the user's machine. Omit to keep it as it is
    /// (on for a link that wasn't synced).
    imports: Option<bool>,
    /// false turns Exports off.
    exports: Option<bool>,
    /// Exports: the Frame.io folder the user's exports go up to (Portal shares it with a Send
    /// link). Setting it turns Exports on.
    #[serde(alias = "return_folder_id")]
    exports_folder_id: Option<String>,
    /// The project folder on the user's machine (`turbosort`). Omit for the job folder.
    project_folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetProjectFolder {
    link_id: String,
    /// Project folder name (`turbosort`), or null to clear it.
    project_folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateUploadLink {
    name: String,
    account_id: String,
    workspace_id: String,
    project_id: String,
    folder_id: String,
    /// true syncs the link's project folders with FioTransfer: Imports (what the client uploads
    /// comes down), and Exports when `exports_folder_id` is given.
    sync: Option<bool>,
    /// false leaves Imports off (an Exports-only link). Default on.
    imports: Option<bool>,
    /// The Frame.io folder the user's exports go up to (implies `sync`). Omit for no Exports.
    #[serde(alias = "return_folder_id")]
    exports_folder_id: Option<String>,
    /// The project folder on the user's machine. Omit for the job folder the link receives in.
    turbosort: Option<String>,
    /// RFC 3339 timestamp.
    expires_at: Option<String>,
    password: Option<String>,
    max_file_size: Option<i64>,
    allowed_extensions: Option<Vec<String>>,
    /// Folders the uploads land in below the link's folder, e.g. `{project}/{date}`. Tokens:
    /// date, year, month, day, time, uploader_name, project. A template using uploader_name or
    /// project asks the client for it.
    subfolder_template: Option<String>,
    /// What the upload page asks the client for, any of: name, email, message, project.
    ask_for: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Account {
    account_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Workspace {
    account_id: String,
    workspace_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Folder {
    account_id: String,
    folder_id: String,
}

#[tool_router(server_handler)]
impl PortalServer {
    #[tool(description = "Return Portal's service, database and Redis health.")]
    async fn health(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/health", None, &[]).await?)
    }

    #[tool(description = "List upload links, including public URL, state and FioTransfer sync (folder.sync).")]
    async fn list_upload_links(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/upload-links", None, &[]).await?)
    }

    #[tool(description = "Read one upload link by id.")]
    async fn get_upload_link(&self, Parameters(LinkId { link_id }): Parameters<LinkId>) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &link_id)?;
        pretty(self.call(reqwest::Method::GET, &format!("/api/upload-links/{link_id}"), None, &[]).await?)
    }

    #[tool(description = "Create an upload link for a Frame.io folder. Set sync to give it a folder in FioTransfer.")]
    async fn create_upload_link(
        &self,
        Parameters(p): Parameters<CreateUploadLink>,
    ) -> Result<String, ErrorData> {
        let mut body = json!({
            "name": p.name,
            "folder": {
                "type": "frameio",
                "account_id": p.account_id,
                "workspace_id": p.workspace_id,
                "project_id": p.project_id,
                "folder_id": p.folder_id,
            }
        });
        if p.sync.unwrap_or(false) || p.exports_folder_id.is_some() {
            body["folder"]["sync"] = with_sync(None, true, p.imports, None, p.exports_folder_id.as_deref());
        }
        let optional = [
            ("turbosort", p.turbosort.map(Value::String)),
            ("expires_at", p.expires_at.map(Value::String)),
            ("password", p.password.map(Value::String)),
            ("max_file_size", p.max_file_size.map(|v| json!(v))),
            ("allowed_extensions", p.allowed_extensions.map(|v| json!(v))),
            ("subfolder_template", p.subfolder_template.map(Value::String)),
            ("uploader_fields_required", p.ask_for.as_deref().map(uploader_fields).transpose()?),
        ];
        for (key, value) in optional {
            if let Some(value) = value {
                body[key] = value;
            }
        }
        pretty(self.call(reqwest::Method::POST, "/api/upload-links", Some(body), &[]).await?)
    }

    #[tool(description = "Patch an upload link. Only the supplied fields change.")]
    async fn update_upload_link(&self, Parameters(p): Parameters<UpdateLink>) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &p.link_id)?;
        pretty(
            self.call(
                reqwest::Method::PATCH,
                &format!("/api/upload-links/{link_id}"),
                Some(p.fields),
                &[],
            )
            .await?,
        )
    }

    #[tool(description = "Set an upload link's project folder name (turbosort): the folder FioTransfer keeps its Imports and Exports in. null goes back to the job folder.")]
    async fn set_project_folder(
        &self,
        Parameters(p): Parameters<SetProjectFolder>,
    ) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &p.link_id)?;
        pretty(
            self.call(
                reqwest::Method::PATCH,
                &format!("/api/upload-links/{link_id}"),
                Some(json!({ "turbosort": p.project_folder })),
                &[],
            )
            .await?,
        )
    }

    #[tool(description = "Turn Sync with FioTransfer on or off for an upload link, and set its Imports, Exports folder and project folder.")]
    async fn set_fiotransfer_sync(
        &self,
        Parameters(p): Parameters<SetFiotransferSync>,
    ) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &p.link_id)?;
        let path = format!("/api/upload-links/{link_id}");
        let link = self.call(reqwest::Method::GET, &path, None, &[]).await?;
        let mut folder = link.get("folder").filter(|f| f.is_object()).cloned().ok_or_else(|| {
            ErrorData::invalid_params("That upload link has no Frame.io folder to sync", None)
        })?;
        folder["sync"] = with_sync(folder.get("sync"), p.enabled, p.imports, p.exports, p.exports_folder_id.as_deref());
        let mut body = json!({ "folder": folder });
        if let Some(name) = p.project_folder {
            body["turbosort"] = json!(name);
        }
        pretty(self.call(reqwest::Method::PATCH, &path, Some(body), &[]).await?)
    }

    #[tool(description = "Delete an upload link.")]
    async fn delete_upload_link(&self, Parameters(LinkId { link_id }): Parameters<LinkId>) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &link_id)?;
        pretty(self.call(reqwest::Method::DELETE, &format!("/api/upload-links/{link_id}"), None, &[]).await?)
    }

    #[tool(description = "Return whether Frame.io is connected and when its token expires.")]
    async fn frameio_status(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/frameio/status", None, &[]).await?)
    }

    #[tool(description = "List Frame.io accounts visible to the connected user.")]
    async fn list_frameio_accounts(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/frameio/accounts", None, &[]).await?)
    }

    #[tool(description = "List workspaces in a Frame.io account.")]
    async fn list_frameio_workspaces(
        &self,
        Parameters(Account { account_id }): Parameters<Account>,
    ) -> Result<String, ErrorData> {
        pretty(
            self.call(
                reqwest::Method::GET,
                "/api/frameio/workspaces",
                None,
                &[("account_id", account_id)],
            )
            .await?,
        )
    }

    #[tool(description = "List projects in a Frame.io workspace.")]
    async fn list_frameio_projects(
        &self,
        Parameters(Workspace { account_id, workspace_id }): Parameters<Workspace>,
    ) -> Result<String, ErrorData> {
        pretty(
            self.call(
                reqwest::Method::GET,
                "/api/frameio/projects",
                None,
                &[("account_id", account_id), ("workspace_id", workspace_id)],
            )
            .await?,
        )
    }

    #[tool(description = "List subfolders of a Frame.io folder.")]
    async fn list_frameio_folders(
        &self,
        Parameters(Folder { account_id, folder_id }): Parameters<Folder>,
    ) -> Result<String, ErrorData> {
        pretty(
            self.call(
                reqwest::Method::GET,
                "/api/frameio/folders",
                None,
                &[("account_id", account_id), ("folder_id", folder_id)],
            )
            .await?,
        )
    }

    #[tool(description = "List files in a Frame.io folder.")]
    async fn list_frameio_files(
        &self,
        Parameters(Folder { account_id, folder_id }): Parameters<Folder>,
    ) -> Result<String, ErrorData> {
        pretty(
            self.call(
                reqwest::Method::GET,
                "/api/frameio/files",
                None,
                &[("account_id", account_id), ("folder_id", folder_id)],
            )
            .await?,
        )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = PortalServer::new()?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{path_id, uploader_fields, with_sync};
    use serde_json::{json, Value};

    #[test]
    fn sync_switch_keeps_what_the_link_stores() {
        let exp = "6f4121b3-628d-4b7a-91ea-8b1ac4a3f0a3";
        // A link that wasn't synced gets Imports, and Exports when a folder is given.
        assert_eq!(with_sync(None, true, None, None, None), json!({ "drive": {} }));
        assert_eq!(with_sync(None, true, None, None, Some(exp)), json!({ "drive": {}, "exports": { "folder_id": exp } }));
        assert_eq!(with_sync(None, true, Some(false), None, Some(exp)), json!({ "exports": { "folder_id": exp } }));
        assert_eq!(with_sync(Some(&json!({ "drive": {} })), false, None, None, None), Value::Null);
        // A stored link keeps its lanes and names; a new Exports folder keeps the name on disk.
        let old = json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" }, "exports": { "local": "0_EXPORTS", "folder_id": exp } });
        assert_eq!(with_sync(Some(&old), true, None, None, None), old);
        let other = "8008cf70-f28e-474d-a209-d67afb9fddb6";
        assert_eq!(
            with_sync(Some(&old), true, None, None, Some(other)),
            json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" }, "exports": { "local": "0_EXPORTS", "folder_id": other } })
        );
        // Lanes turn off one at a time.
        assert_eq!(with_sync(Some(&old), true, None, Some(false), None), json!({ "mode": "add_only", "drive": { "local": "1_DRIVE" } }));
        assert_eq!(
            with_sync(Some(&old), true, Some(false), None, None),
            json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS", "folder_id": exp } })
        );
        // An Exports-only link stays that way unless Imports is asked for.
        let exports_only = json!({ "mode": "add_only", "exports": { "local": "0_EXPORTS", "folder_id": exp } });
        assert_eq!(with_sync(Some(&exports_only), true, None, None, None), exports_only);
    }

    #[test]
    fn ask_for_names_the_upload_page_fields() {
        let asked = uploader_fields(&["name".into(), " project ".into()]).unwrap();
        assert_eq!(asked, json!({ "name": true, "email": false, "message": false, "project": true }));
        assert!(uploader_fields(&["phone".into()]).is_err());
    }

    #[test]
    fn path_ids_are_uuids_only() {
        let id = "8008cf70-f28e-474d-a209-d67afb9fddb6";
        assert_eq!(path_id("link_id", id).ok(), Some(id));
        assert_eq!(path_id("link_id", " 8008CF70-F28E-474D-A209-D67AFB9FDDB6 ").ok(), Some("8008CF70-F28E-474D-A209-D67AFB9FDDB6"));
        for bad in [
            "",
            "../frameio/projects/8008cf70-f28e-474d-a209-d67afb9fddb6",
            "%2e%2e/frameio/files/x",
            "8008cf70-f28e-474d-a209-d67afb9fddb6/../../settings",
            "8008cf70f28e474da209d67afb9fddb6",
            "8008cf70-f28e-474d-a209-d67afb9fddbz",
        ] {
            assert!(path_id("link_id", bad).is_err(), "{bad:?} must be refused");
        }
    }
}
