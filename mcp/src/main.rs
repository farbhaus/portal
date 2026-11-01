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
    /// Fields to PATCH. Only supplied keys change; `turbosort: null` clears the project folder name.
    /// The FioTransfer sync rides in `folder`: send the link's `folder` with a `sync` block
    /// (`{"mode":"add_only","drive":{"local":"1_DRIVE"},"exports":{"local":"0_EXPORTS"}}`), or
    /// `sync: null` to remove it.
    fields: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetProjectFolder {
    link_id: String,
    /// Project folder name, or null to clear it.
    project_folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateUploadLink {
    name: String,
    account_id: String,
    workspace_id: String,
    project_id: String,
    folder_id: String,
    /// The project's local folder name; FioTransfer reads it from `GET /api/upload-links`.
    turbosort: Option<String>,
    /// RFC 3339 timestamp.
    expires_at: Option<String>,
    password: Option<String>,
    max_file_size: Option<i64>,
    allowed_extensions: Option<Vec<String>>,
    subfolder_template: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RuleId {
    rule_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateSyncRule {
    name: String,
    account_id: String,
    workspace_id: String,
    project_id: String,
    folder_id: String,
    destination_path: String,
    conflict_policy: Option<String>,
    path_template: Option<String>,
    enabled: Option<bool>,
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

    #[tool(description = "List upload links, including public URL, state, project folder (turbosort) and FioTransfer sync.")]
    async fn list_upload_links(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/upload-links", None, &[]).await?)
    }

    #[tool(description = "Read one upload link by id.")]
    async fn get_upload_link(&self, Parameters(LinkId { link_id }): Parameters<LinkId>) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &link_id)?;
        pretty(self.call(reqwest::Method::GET, &format!("/api/upload-links/{link_id}"), None, &[]).await?)
    }

    #[tool(description = "Create an upload link for a Frame.io folder. Set turbosort to the project's local folder name.")]
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
        let optional = [
            ("turbosort", p.turbosort.map(Value::String)),
            ("expires_at", p.expires_at.map(Value::String)),
            ("password", p.password.map(Value::String)),
            ("max_file_size", p.max_file_size.map(|v| json!(v))),
            ("allowed_extensions", p.allowed_extensions.map(|v| json!(v))),
            ("subfolder_template", p.subfolder_template.map(Value::String)),
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

    #[tool(description = "Set or clear (null) the project's local folder name (turbosort) for an upload link.")]
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

    #[tool(description = "Delete an upload link.")]
    async fn delete_upload_link(&self, Parameters(LinkId { link_id }): Parameters<LinkId>) -> Result<String, ErrorData> {
        let link_id = path_id("link_id", &link_id)?;
        pretty(self.call(reqwest::Method::DELETE, &format!("/api/upload-links/{link_id}"), None, &[]).await?)
    }

    #[tool(description = "List Frame.io-to-NAS sync rules.")]
    async fn list_sync_rules(&self) -> Result<String, ErrorData> {
        pretty(self.call(reqwest::Method::GET, "/api/sync-rules", None, &[]).await?)
    }

    #[tool(description = "Read one sync rule.")]
    async fn get_sync_rule(&self, Parameters(RuleId { rule_id }): Parameters<RuleId>) -> Result<String, ErrorData> {
        let rule_id = path_id("rule_id", &rule_id)?;
        pretty(self.call(reqwest::Method::GET, &format!("/api/sync-rules/{rule_id}"), None, &[]).await?)
    }

    #[tool(description = "Create a sync rule. destination_path must be writable by the Portal container.")]
    async fn create_sync_rule(
        &self,
        Parameters(p): Parameters<CreateSyncRule>,
    ) -> Result<String, ErrorData> {
        let mut body = json!({
            "name": p.name,
            "source": {
                "type": "frameio",
                "account_id": p.account_id,
                "workspace_id": p.workspace_id,
                "project_id": p.project_id,
                "folder_id": p.folder_id,
                "recursive": true,
            },
            "destination_path": p.destination_path,
            "conflict_policy": p.conflict_policy.unwrap_or_else(|| "rename_suffix".to_string()),
            "enabled": p.enabled.unwrap_or(true),
        });
        if let Some(template) = p.path_template {
            body["path_template"] = json!(template);
        }
        pretty(self.call(reqwest::Method::POST, "/api/sync-rules", Some(body), &[]).await?)
    }

    #[tool(description = "Reconcile a sync rule now: queue any missing files.")]
    async fn run_sync_rule(&self, Parameters(RuleId { rule_id }): Parameters<RuleId>) -> Result<String, ErrorData> {
        let rule_id = path_id("rule_id", &rule_id)?;
        pretty(self.call(reqwest::Method::POST, &format!("/api/sync-rules/{rule_id}/run"), None, &[]).await?)
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
    use super::path_id;

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
