# Portal MCP server (Rust)

A Rust stdio MCP server over Portal's admin HTTP API, built with
[`rmcp`](https://crates.io/crates/rmcp). No Python or Node is needed to run it.

It exposes upload links (including the `.turbosort` project folder name), sync rules, and Frame.io
browsing as MCP tools. It authenticates with `PORTAL_API_TOKEN` (the bearer token described in
[../docs/API.md](../docs/API.md)).

## Build

```bash
cd mcp
cargo build --release
```

## Run

```bash
PORTAL_URL=https://portal.example.com \
PORTAL_API_TOKEN='<the token from Portal .env>' \
./target/release/portal-mcp
```

The server speaks MCP over stdio.

## MCP client configuration

```json
{
  "mcpServers": {
    "portal": {
      "command": "/path/to/portal/mcp/target/release/portal-mcp",
      "env": {
        "PORTAL_URL": "https://portal.example.com",
        "PORTAL_API_TOKEN": "<token>"
      }
    }
  }
}
```

## Tools

| Tool | Purpose |
|---|---|
| `health` | Service, database and Redis health |
| `list_upload_links` / `get_upload_link` | Read upload links |
| `create_upload_link` | Create a link for a Frame.io folder, with optional `turbosort` |
| `update_upload_link` | Patch any link fields |
| `set_project_zeus` | Set or clear the Live_Work folder for a link |
| `delete_upload_link` | Delete a link |
| `list_sync_rules` / `get_sync_rule` | Read Frame.io-to-NAS rules |
| `create_sync_rule` / `run_sync_rule` | Create a rule, or reconcile one now |
| `frameio_status`, `list_frameio_accounts`, `list_frameio_workspaces`, `list_frameio_projects`, `list_frameio_folders`, `list_frameio_files` | Browse the connected Frame.io account |

The token is full-admin. Keep it out of logs and client configuration files you share.
