
//! Destinations CRUD — `portal.routes.destinations`.
//!
//! Reads are fully ported. `POST` reaches the Frame.io client (via the `crate::frameio` seam),
//! which is not ported, so it currently reproduces the Python behaviour for an unconnected
//! instance (409) and will work unchanged once the client lands.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::services::frameio_paths::{clean_path, refresh_config_path, resolve_folder_path, stamp_path};
use crate::services::sync_coverage::{rules_covering, Rule};
use crate::state::AppState;

fn link_state(row: &sqlx::postgres::PgRow) -> &'static str {
    let revoked: Option<chrono::DateTime<chrono::Utc>> = row.get("revoked_at");
    if revoked.is_some() {
        return "revoked";
    }
    if let Some(exp) = row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at") {
        if exp <= chrono::Utc::now() {
            return "expired";
        }
    }
    "ok"
}

async fn load_rules(state: &AppState) -> Vec<Rule> {
    sqlx::query("SELECT id, name, destination_path, enabled, source_config FROM sync_rules")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| Rule {
            id: r.get("id"),
            name: r.get("name"),
            destination_path: r.get("destination_path"),
            enabled: r.get("enabled"),
            source_config: r.get("source_config"),
        })
        .collect()
}

/// Render a destination row plus its covering rules and upload links.
fn render(dest: &sqlx::postgres::PgRow, rules: &[Rule], links: &[sqlx::postgres::PgRow]) -> Value {
    let config: Value = dest.get("config");
    let covering: Vec<Value> = rules_covering(&config, rules)
        .into_iter()
        .map(|(rule, exact)| {
            json!({
                "id": rule.id.to_string(),
                "name": rule.name,
                "destination_path": rule.destination_path,
                "enabled": rule.enabled,
                "recursive": rule.source_config.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true),
                "exact": exact,
            })
        })
        .collect();
    let mut link_out: Vec<Value> = links
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "label": row.get::<Option<String>, _>("brand_display_name").unwrap_or_else(|| "Untitled".to_string()),
                "state": link_state(row),
            })
        })
        .collect();
    // Python sorts links by created_at desc before rendering.
    link_out.reverse(); // links arrive created_at ASC
    json!({
        "id": dest.get::<Uuid, _>("id").to_string(),
        "display_name": dest.get::<String, _>("display_name"),
        "config": config,
        "subtitle": dest.get::<Option<String>, _>("subtitle"),
        "sync_rules": covering,
        "upload_links": link_out,
        "created_at": crate::pyfmt::iso(&dest.get::<chrono::DateTime<chrono::Utc>, _>("created_at")),
        "updated_at": crate::pyfmt::iso(&dest.get::<chrono::DateTime<chrono::Utc>, _>("updated_at")),
    })
}

async fn links_for(state: &AppState, dest_id: Uuid) -> Vec<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT id, brand_display_name, revoked_at, expires_at, created_at
           FROM upload_links WHERE destination_id = $1 ORDER BY created_at ASC",
    )
    .bind(dest_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}

pub async fn list_destinations(State(state): State<AppState>, _: Admin) -> Response {
    let dests = sqlx::query(
        "SELECT id, display_name, config, subtitle, created_at, updated_at
           FROM destinations ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await;
    let dests = match dests {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("destinations.list_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let rules = load_rules(&state).await;
    let mut out = Vec::with_capacity(dests.len());
    for d in &dests {
        let links = links_for(&state, d.get("id")).await;
        out.push(render(d, &rules, &links));
    }
    Json(Value::Array(out)).into_response()
}

async fn fetch_one(state: &AppState, id: Uuid) -> Option<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT id, display_name, config, subtitle, created_at, updated_at
           FROM destinations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
}

pub async fn get_destination(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
    let Some(mut dest) = fetch_one(&state, id).await else {
        return detail(StatusCode::NOT_FOUND, "Destination not found");
    };
    // Lazily backfill/refresh the cached breadcrumb (best-effort; never fails the request).
    let config: Value = dest.get("config");
    if let Some(refreshed) = refresh_config_path(&state, &config).await {
        let _ = sqlx::query("UPDATE destinations SET config = $1 WHERE id = $2")
            .bind(&refreshed)
            .bind(id)
            .execute(&state.db)
            .await;
        dest = fetch_one(&state, id).await.unwrap_or(dest);
    }
    let rules = load_rules(&state).await;
    let links = links_for(&state, id).await;
    Json(render(&dest, &rules, &links)).into_response()
}

pub async fn update_destination(
    State(state): State<AppState>,
    _: Admin,
    Path(id): Path<Uuid>,
    Json(body): Json<Map<String, Value>>,
) -> Response {
    if fetch_one(&state, id).await.is_none() {
        return detail(StatusCode::NOT_FOUND, "Destination not found");
    }
    // `exclude_unset` semantics: only keys present in the body are written.
    if body.contains_key("display_name") {
        let v = body.get("display_name").unwrap();
        let _ = sqlx::query("UPDATE destinations SET display_name = $1 WHERE id = $2")
            .bind(v.as_str())
            .bind(id)
            .execute(&state.db)
            .await;
    }
    if body.contains_key("subtitle") {
        let v = body.get("subtitle").unwrap();
        let _ = sqlx::query("UPDATE destinations SET subtitle = $1 WHERE id = $2")
            .bind(v.as_str())
            .bind(id)
            .execute(&state.db)
            .await;
    }
    let dest = fetch_one(&state, id).await.unwrap();
    let rules = load_rules(&state).await;
    let links = links_for(&state, id).await;
    Json(render(&dest, &rules, &links)).into_response()
}

pub async fn delete_destination(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    if fetch_one(&state, id).await.is_none() {
        return detail(StatusCode::NOT_FOUND, "Destination not found");
    }
    let res = sqlx::query("DELETE FROM destinations WHERE id = $1").bind(id).execute(&state.db).await;
    if let Err(e) = res {
        tracing::error!("destinations.delete_failed: {e}");
        return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    }
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1, 'destination.deleted', $2)")
        .bind(admin.id)
        .bind(json!({ "destination_id": id.to_string() }))
        .execute(&state.db)
        .await;
    StatusCode::NO_CONTENT.into_response()
}

pub async fn create_destination(
    State(state): State<AppState>,
    admin: Admin,
    Json(body): Json<Value>,
) -> Response {
    let config = match body.get("config") {
        Some(c) => c.clone(),
        None => return detail(StatusCode::UNPROCESSABLE_ENTITY, "Field required"),
    };
    let account_id = config.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let folder_id = config.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
    let project_id = config.get("project_id").and_then(|v| v.as_str()).unwrap_or("");

    // Validate the picked folder against Frame.io (the client seam reports NotConnected today).
    let folder = match state.frameio.get_folder(account_id, folder_id).await {
        Ok(f) => f,
        Err(crate::frameio::FrameioError::NotConnected) => {
            return detail(StatusCode::CONFLICT, "Frame.io is not connected")
        }
        Err(_) => return detail(StatusCode::BAD_REQUEST, "Frame.io folder not found or inaccessible"),
    };

    let mut stored = config.as_object().cloned().unwrap_or_default();
    stored.remove("path");
    stored.remove("project_name");
    stored.insert("folder_name".to_string(), json!(folder.name));

    let mut stored = Value::Object(stored);
    let mut path = clean_path(config.get("path"), folder_id);
    let project_name = body
        .get("project_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let project_name = if project_name.is_empty() { None } else { Some(project_name) };
    if let Some(p) = &path {
        stored = stamp_path(&stored, p, project_name.as_deref());
    } else if let Some((p, pn)) = resolve_folder_path(&state, account_id, project_id, folder_id).await {
        // Non-UI callers don't know the breadcrumb; resolve it best-effort (display-only).
        stored = stamp_path(&stored, &p, pn.as_deref());
    }

    let display_name = body.get("display_name").and_then(|v| v.as_str()).unwrap_or("");
    let subtitle = body.get("subtitle").cloned().unwrap_or(Value::Null);
    let inserted = sqlx::query(
        "INSERT INTO destinations (display_name, config, subtitle) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(display_name)
    .bind(&stored)
    .bind(if subtitle.is_null() { None } else { subtitle.as_str() })
    .fetch_one(&state.db)
    .await;
    let id: Uuid = match inserted {
        Ok(row) => row.get("id"),
        Err(e) => {
            tracing::error!("destinations.create_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1, 'destination.created', $2)")
        .bind(admin.id)
        .bind(json!({ "destination_id": id.to_string(), "display_name": display_name }))
        .execute(&state.db)
        .await;

    let dest = fetch_one(&state, id).await.unwrap();
    let rules = load_rules(&state).await;
    let links = links_for(&state, id).await;
    (StatusCode::CREATED, Json(render(&dest, &rules, &links))).into_response()
}
