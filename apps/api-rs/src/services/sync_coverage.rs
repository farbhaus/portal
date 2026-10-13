
//! Sync rules covering a destination's folder — port of `portal.services.sync_coverage`.
//! Pure and DB-only: never calls Frame.io.

use serde_json::Value;

pub struct Rule {
    pub id: uuid::Uuid,
    pub name: String,
    pub destination_path: String,
    pub enabled: bool,
    pub source_config: Value,
}

/// (rule, exact) pairs. `exact` is false when covered by a recursive rule on an ancestor folder.
pub fn rules_covering<'a>(dest_config: &Value, rules: &'a [Rule]) -> Vec<(&'a Rule, bool)> {
    let account_id = dest_config.get("account_id").and_then(|v| v.as_str());
    let folder_id = dest_config.get("folder_id").and_then(|v| v.as_str());
    let (Some(account_id), Some(folder_id)) = (account_id, folder_id) else {
        return Vec::new();
    };
    let ancestors: Vec<String> = dest_config
        .get("path")
        .and_then(|v| v.as_array())
        .map(|arr| {
            let upto = arr.len().saturating_sub(1);
            arr[..upto]
                .iter()
                .filter_map(|seg| seg.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut out = Vec::new();
    for rule in rules {
        let src = &rule.source_config;
        if src.get("account_id").and_then(|v| v.as_str()) != Some(account_id) {
            continue;
        }
        let rule_folder = src.get("folder_id").and_then(|v| v.as_str());
        if rule_folder == Some(folder_id) {
            out.push((rule, true));
        } else if src.get("recursive").map(|v| v.as_bool().unwrap_or(true)).unwrap_or(true) {
            if let Some(rf) = rule_folder {
                if ancestors.iter().any(|a| a == rf) {
                    out.push((rule, false));
                }
            }
        }
    }
    out
}
