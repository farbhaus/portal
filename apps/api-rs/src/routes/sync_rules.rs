
//! Sync-rule CRUD + job log + manual run — `portal.routes.sync_rules`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::detail;
use crate::frameio::FrameioError;
use crate::services::frameio_paths::{clean_path, refresh_config_path, resolve_folder_path, stamp_path};
use crate::state::AppState;
use crate::storage::frameio_backend::FrameioStorageBackend;
use crate::storage::DestinationConfig;
use crate::sync::queue::{self, RUN_SYNC_JOB};
use crate::sync::runner;

const CONFLICT_POLICIES: [&str; 3] = ["rename_suffix", "skip", "overwrite"];

fn dest_of(rule: &runner::SyncRuleRow) -> DestinationConfig {
    let t = rule.source_config.get("type").and_then(|v| v.as_str()).unwrap_or("frameio");
    DestinationConfig::new(t, rule.source_config.clone())
}

fn callback_url(state: &AppState, rule_id: Uuid) -> String {
    format!("{}/api/webhooks/frameio/{rule_id}", state.settings.base_url.trim_end_matches('/'))
}

fn rule_out(rule: &runner::SyncRuleRow, created_at: DateTime<Utc>) -> Value {
    json!({
        "id": rule.id.to_string(),
        "name": rule.source_config.get("__name").and_then(|v| v.as_str()).unwrap_or(""),
        "source": rule.source_config,
        "destination_path": rule.destination_path,
        "conflict_policy": rule.conflict_policy,
        "path_template": rule.path_template,
        "enabled": rule.enabled,
        "subscription_active": rule.frameio_webhook_id.is_some(),
        "created_at": crate::pyfmt::iso(&created_at),
    })
}

fn frameio_status(e: &FrameioError, not_connected_msg: &str, busy_msg: &str, other_msg: &str) -> Response {
    match e {
        FrameioError::NotConnected => detail(StatusCode::CONFLICT, not_connected_msg),
        FrameioError::RateLimited => detail(StatusCode::SERVICE_UNAVAILABLE, busy_msg),
        _ => detail(StatusCode::BAD_GATEWAY, other_msg),
    }
}

/// Resolve project/folder names + the display breadcrumb for a rule source.
async fn enrich_source(state: &AppState, source: &Value) -> Result<Value, Response> {
    if source.get("type").and_then(|v| v.as_str()) != Some("frameio") {
        return Err(detail(StatusCode::BAD_REQUEST, "source.type must be 'frameio'"));
    }
    let get = |k: &str| source.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let (Some(account_id), Some(workspace_id), Some(project_id), Some(folder_id)) =
        (get("account_id"), get("workspace_id"), get("project_id"), get("folder_id"))
    else {
        return Err(detail(StatusCode::BAD_REQUEST, "source missing required id"));
    };
    let mut folder_name = get("folder_name").unwrap_or_default();
    let mut project_name = get("project_name").unwrap_or_default();
    if folder_name.is_empty() || project_name.is_empty() {
        if folder_name.is_empty() {
            match state.frameio.get_folder(&account_id, &folder_id).await {
                Ok(f) => folder_name = f.name,
                Err(e) => return Err(frameio_status(&e, "Frame.io is not connected", "Frame.io is busy, try again in a moment", "Could not validate the source")),
            }
        }
        if project_name.is_empty() {
            match state.frameio.list_projects(&account_id, &workspace_id).await {
                Ok(projects) => {
                    project_name = projects.into_iter().find(|p| p.id == project_id).map(|p| p.name).unwrap_or_default();
                }
                Err(e) => return Err(frameio_status(&e, "Frame.io is not connected", "Frame.io is busy, try again in a moment", "Could not validate the source")),
            }
        }
    }
    let mut enriched = json!({
        "type": "frameio",
        "account_id": account_id,
        "workspace_id": workspace_id,
        "project_id": project_id,
        "folder_id": folder_id,
        "recursive": source.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true),
        "folder_name": folder_name,
        "project_name": project_name,
    });
    let mut path = clean_path(source.get("path"), &folder_id);
    if path.is_none() {
        if let Some((p, pn)) = resolve_folder_path(state, &account_id, &project_id, &folder_id).await {
            path = Some(p);
            if let Some(pn) = pn {
                enriched["project_name"] = json!(pn);
            }
        }
    }
    if let Some(p) = &path {
        let pn = enriched.get("project_name").and_then(|v| v.as_str()).map(|s| s.to_string());
        enriched = stamp_path(&enriched, p, pn.as_deref());
    }
    Ok(enriched)
}

async fn subscribe(state: &AppState, rule_id: Uuid, dest: &DestinationConfig) -> Result<(String, Option<String>), Response> {
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    match backend.subscribe_to_changes(dest, &callback_url(state, rule_id)).await {
        Ok(sub) => Ok((sub.id, sub.secret)),
        Err(e) => Err(frameio_status(&e, "Frame.io is not connected", "Frame.io is busy, try again in a moment", "Could not create the webhook")),
    }
}

async fn unsubscribe(state: &AppState, dest: &DestinationConfig, webhook_id: &str) {
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    let _ = backend.unsubscribe_from_changes(dest, webhook_id).await;
}

async fn fetch_rule(state: &AppState, id: Uuid) -> Option<(runner::SyncRuleRow, String, DateTime<Utc>)> {
    let row = sqlx::query("SELECT * FROM sync_rules WHERE id = $1").bind(id).fetch_optional(&state.db).await.ok().flatten()?;
    let rule = runner::SyncRuleRow {
        id: row.get("id"),
        destination_path: row.get("destination_path"),
        conflict_policy: row.get("conflict_policy"),
        path_template: row.get("path_template"),
        source_config: row.get("source_config"),
        frameio_webhook_id: row.get("frameio_webhook_id"),
        webhook_secret: row.get("webhook_secret"),
        enabled: row.get("enabled"),
    };
    Some((rule, row.get::<String, _>("name"), row.get::<DateTime<Utc>, _>("created_at")))
}

fn rule_json(name: &str, rule: &runner::SyncRuleRow, created_at: DateTime<Utc>) -> Value {
    json!({
        "id": rule.id.to_string(),
        "name": name,
        "source": rule.source_config,
        "destination_path": rule.destination_path,
        "conflict_policy": rule.conflict_policy,
        "path_template": rule.path_template,
        "enabled": rule.enabled,
        "subscription_active": rule.frameio_webhook_id.is_some(),
        "created_at": crate::pyfmt::iso(&created_at),
    })
}

pub async fn list_rules(State(state): State<AppState>, _: Admin) -> Response {
    let rows = sqlx::query("SELECT * FROM sync_rules ORDER BY created_at DESC").fetch_all(&state.db).await;
    let Ok(rows) = rows else { return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error") };
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            let rule = runner::SyncRuleRow {
                id: r.get("id"),
                destination_path: r.get("destination_path"),
                conflict_policy: r.get("conflict_policy"),
                path_template: r.get("path_template"),
                source_config: r.get("source_config"),
                frameio_webhook_id: r.get("frameio_webhook_id"),
                webhook_secret: r.get("webhook_secret"),
                enabled: r.get("enabled"),
            };
            rule_json(&r.get::<String, _>("name"), &rule, r.get("created_at"))
        })
        .collect();
    Json(Value::Array(out)).into_response()
}

pub async fn create_rule(State(state): State<AppState>, admin: Admin, Json(body): Json<Value>) -> Response {
    let policy = body.get("conflict_policy").and_then(|v| v.as_str()).unwrap_or("rename_suffix").to_string();
    if !CONFLICT_POLICIES.contains(&policy.as_str()) {
        return detail(StatusCode::BAD_REQUEST, "conflict_policy must be one of ['overwrite', 'rename_suffix', 'skip']");
    }
    let source = body.get("source").cloned().unwrap_or_else(|| json!({}));
    let enriched = match enrich_source(&state, &source).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let destination_path = body.get("destination_path").and_then(|v| v.as_str()).unwrap_or("");
    let path_template = body.get("path_template").and_then(|v| v.as_str());
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

    let id: Uuid = match sqlx::query_scalar(
        "INSERT INTO sync_rules (name, source_config, destination_path, conflict_policy, path_template, enabled)
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(name)
    .bind(&enriched)
    .bind(destination_path)
    .bind(&policy)
    .bind(path_template)
    .bind(enabled)
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("sync_rules.create_failed: {e}");
            return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    if enabled {
        let dest = DestinationConfig::new("frameio", enriched.clone());
        match subscribe(&state, id, &dest).await {
            Ok((wid, secret)) => {
                let _ = sqlx::query("UPDATE sync_rules SET frameio_webhook_id=$1, webhook_secret=$2, updated_at=now() WHERE id=$3")
                    .bind(wid).bind(secret).bind(id).execute(&state.db).await;
            }
            Err(r) => return r,
        }
    }
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'sync_rule.created',$2)")
        .bind(admin.id).bind(json!({ "sync_rule_id": id.to_string() })).execute(&state.db).await;

    let Some((rule, name, created)) = fetch_rule(&state, id).await else { return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error") };
    (StatusCode::CREATED, Json(rule_json(&name, &rule, created))).into_response()
}

pub async fn get_rule(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
    let Some((rule, name, created)) = fetch_rule(&state, id).await else {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    };
    if let Some(refreshed) = refresh_config_path(&state, &rule.source_config).await {
        let _ = sqlx::query("UPDATE sync_rules SET source_config=$1, updated_at=now() WHERE id=$2")
            .bind(&refreshed).bind(id).execute(&state.db).await;
        if let Some((rule, name, created)) = fetch_rule(&state, id).await {
            return Json(rule_json(&name, &rule, created)).into_response();
        }
    }
    Json(rule_json(&name, &rule, created)).into_response()
}

pub async fn update_rule(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> Response {
    let Some((rule, _, _)) = fetch_rule(&state, id).await else {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    };
    if let Some(policy) = body.get("conflict_policy").and_then(|v| v.as_str()) {
        if !CONFLICT_POLICIES.contains(&policy) {
            return detail(StatusCode::BAD_REQUEST, "conflict_policy must be one of ['overwrite', 'rename_suffix', 'skip']");
        }
    }
    if let Some(enabled) = body.get("enabled").and_then(|v| v.as_bool()) {
        if enabled != rule.enabled {
            let dest = dest_of(&rule);
            if enabled {
                match subscribe(&state, id, &dest).await {
                    Ok((wid, secret)) => {
                        let _ = sqlx::query("UPDATE sync_rules SET frameio_webhook_id=$1, webhook_secret=$2, updated_at=now() WHERE id=$3")
                            .bind(wid).bind(secret).bind(id).execute(&state.db).await;
                    }
                    Err(r) => return r,
                }
            } else if let Some(wid) = &rule.frameio_webhook_id {
                unsubscribe(&state, &dest, wid).await;
                let _ = sqlx::query("UPDATE sync_rules SET frameio_webhook_id=NULL, webhook_secret=NULL, updated_at=now() WHERE id=$1")
                    .bind(id).execute(&state.db).await;
            }
        }
    }
    for key in ["name", "destination_path", "conflict_policy", "path_template"] {
        if body.contains_key(key) {
            let v = body.get(key).unwrap();
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE sync_rules SET {key}=$1, updated_at=now() WHERE id=$2")))
                .bind(v.as_str()).bind(id).execute(&state.db).await;
        }
    }
    if body.contains_key("enabled") {
        let _ = sqlx::query("UPDATE sync_rules SET enabled=$1, updated_at=now() WHERE id=$2")
            .bind(body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)).bind(id).execute(&state.db).await;
    }
    let Some((rule, name, created)) = fetch_rule(&state, id).await else { return detail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error") };
    Json(rule_json(&name, &rule, created)).into_response()
}

pub async fn delete_rule(State(state): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> Response {
    let Some((rule, _, _)) = fetch_rule(&state, id).await else {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    };
    if let Some(wid) = &rule.frameio_webhook_id {
        unsubscribe(&state, &dest_of(&rule), wid).await;
    }
    let _ = sqlx::query("DELETE FROM sync_rules WHERE id = $1").bind(id).execute(&state.db).await;
    let _ = sqlx::query("INSERT INTO audit_log (user_id, action, detail) VALUES ($1,'sync_rule.deleted',$2)")
        .bind(admin.id).bind(json!({ "sync_rule_id": id.to_string() })).execute(&state.db).await;
    StatusCode::NO_CONTENT.into_response()
}

pub async fn run_rule(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
    if fetch_rule(&state, id).await.is_none() {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    }
    let backend = FrameioStorageBackend::new(state.frameio.clone());
    match runner::reconcile_rule(&state.db, id, &backend, &state.settings).await {
        Ok(created) => {
            let mut redis = state.redis.clone();
            for job_id in &created {
                let _ = queue::enqueue(&mut redis, RUN_SYNC_JOB, &job_id.to_string(), 0.0).await;
            }
            Json(json!({ "ok": true, "created": created.len() })).into_response()
        }
        Err(e) => detail(StatusCode::BAD_GATEWAY, &format!("{e:?}")),
    }
}

pub async fn list_jobs(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>, Query(q): Query<HashMap<String, String>>) -> Response {
    if fetch_rule(&state, id).await.is_none() {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    }
    let limit = q.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query("SELECT * FROM sync_jobs WHERE sync_rule_id=$1 ORDER BY created_at DESC LIMIT $2")
        .bind(id).bind(limit).fetch_all(&state.db).await.unwrap_or_default();
    let out: Vec<Value> = rows.iter().map(|j| job_out(j)).collect();
    Json(Value::Array(out)).into_response()
}

pub fn job_out(j: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": j.get::<Uuid, _>("id").to_string(),
        "frameio_file_id": j.get::<String, _>("frameio_file_id"),
        "status": j.get::<String, _>("status"),
        "bytes": j.get::<Option<i64>, _>("bytes"),
        "sha256": j.get::<Option<String>, _>("sha256"),
        "error": j.get::<Option<String>, _>("error"),
        "retry_count": j.get::<i32, _>("retry_count"),
        "started_at": j.get::<Option<DateTime<Utc>>, _>("started_at").map(|d| crate::pyfmt::iso(&d)),
        "completed_at": j.get::<Option<DateTime<Utc>>, _>("completed_at").map(|d| crate::pyfmt::iso(&d)),
        "created_at": crate::pyfmt::iso(&j.get::<DateTime<Utc>, _>("created_at")),
    })
}

pub async fn job_stats(State(state): State<AppState>, _: Admin, Path(id): Path<Uuid>) -> Response {
    if fetch_rule(&state, id).await.is_none() {
        return detail(StatusCode::NOT_FOUND, "Sync rule not found");
    }
    let rows = sqlx::query("SELECT status, count(*) AS n FROM sync_jobs WHERE sync_rule_id=$1 GROUP BY status")
        .bind(id).fetch_all(&state.db).await.unwrap_or_default();
    let mut counts = Map::new();
    for r in rows {
        counts.insert(r.get::<String, _>("status"), json!(r.get::<i64, _>("n")));
    }
    Json(json!({ "counts": counts })).into_response()
}
