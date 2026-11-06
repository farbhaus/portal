//! What FioTransfer reads off an upload link, beyond `folder.sync` itself (validated in
//! `routes::upload_links`): the Imports and Exports folder names a link gets when it doesn't name
//! its own (a Settings choice, so any naming convention fits), and the project folder a synced
//! link gets when none is given.

use serde_json::Value;
use sqlx::PgPool;

/// The Imports folder's name when Settings leaves it unset.
pub const DEFAULT_IMPORTS: &str = "1_DRIVE";
/// The Exports folder's name when Settings leaves it unset.
pub const DEFAULT_EXPORTS: &str = "0_EXPORTS";

/// The names a link's Imports and Exports folders get on the user's machine unless it names its
/// own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneNames {
    pub imports: String,
    pub exports: String,
}

impl Default for LaneNames {
    fn default() -> Self {
        LaneNames { imports: DEFAULT_IMPORTS.to_string(), exports: DEFAULT_EXPORTS.to_string() }
    }
}

/// The names Settings holds, each falling back to its default when unset.
pub async fn lane_names(db: &PgPool) -> LaneNames {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT fiotransfer_imports_folder, fiotransfer_exports_folder FROM app_settings LIMIT 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    let pick = |v: Option<String>, default: &str| {
        v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
    };
    match row {
        Some((imports, exports)) => LaneNames { imports: pick(imports, DEFAULT_IMPORTS), exports: pick(exports, DEFAULT_EXPORTS) },
        None => LaneNames::default(),
    }
}

/// One folder name on disk: a single path segment of at most 255 bytes, no separators, no `%`, no
/// control characters and not `.`/`..`. `field` names it in the error.
pub fn clean_folder_name(field: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.contains('%')
        || v.chars().any(char::is_control) || v.len() > 255 {
        return Err(format!("{field} must be a single folder name"));
    }
    Ok(v.to_string())
}

/// A Frame.io folder name made into one folder name on disk: separators, `%` and control
/// characters become `-`, at most 255 bytes. None when nothing usable is left.
fn segment(name: &str) -> Option<String> {
    let mut out: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | '%') || c.is_control() { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string();
    while out.len() > 255 {
        out.pop();
    }
    let out = out.trim().to_string();
    (!out.is_empty() && !out.chars().all(|c| c == '.')).then_some(out)
}

/// The project folder a synced link gets when none is given: the job folder its Imports sit in,
/// named for the user's machine.
///
/// - The link receives into the Imports folder itself (it's named like the Imports setting, as
///   `ACME / 1_DRIVE` is): the folder above it.
/// - Otherwise the first folder below the project (a job folder, as an unnamed link is named), or
///   the project itself when the link receives into its root.
///
/// A project's root is shown by its project's name, never by what Frame.io calls the folder.
pub fn default_project_folder(folder: &Value, imports: &str) -> Option<String> {
    let project = folder.get("project_name").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let names: Vec<String> = folder
        .get("path")
        .and_then(|p| p.as_array())
        .map(|path| {
            path.iter()
                .enumerate()
                .map(|(i, seg)| {
                    let name = seg.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
                    match (i, project) {
                        (0, Some(p)) => p.to_string(),
                        (0, None) => name.trim_end_matches(" (root)").trim().to_string(),
                        _ => name.to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let pick = match names.as_slice() {
        [] => project
            .map(str::to_string)
            .or_else(|| folder.get("folder_name").and_then(|v| v.as_str()).map(str::to_string))?,
        [only] => only.clone(),
        [.., parent, last] if last.eq_ignore_ascii_case(imports.trim()) => parent.clone(),
        [_, job, ..] => job.clone(),
    };
    segment(&pick)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(path: &[&str]) -> Value {
        json!({ "path": path.iter().enumerate().map(|(i, n)| json!({ "id": i.to_string(), "name": n })).collect::<Vec<_>>() })
    }

    #[test]
    fn the_project_folder_is_the_job_folder_the_imports_sit_in() {
        // A project per job, receiving into its Imports folder: the project.
        assert_eq!(default_project_folder(&at(&["ACME (root)", "1_DRIVE"]), "1_DRIVE").as_deref(), Some("ACME"));
        // A year project holding job folders: the job, whether the link receives into the job
        // folder or into a folder inside it.
        assert_eq!(default_project_folder(&at(&["2026 (root)", "ACME_SPOT", "RUSHES"]), "RUSHES").as_deref(), Some("ACME_SPOT"));
        assert_eq!(default_project_folder(&at(&["2026 (root)", "ACME_SPOT", "RUSHES"]), "1_DRIVE").as_deref(), Some("ACME_SPOT"));
        assert_eq!(default_project_folder(&at(&["2026 (root)", "ACME_SPOT"]), "1_DRIVE").as_deref(), Some("ACME_SPOT"));
        // Deeper, still the folder above the Imports folder.
        assert_eq!(default_project_folder(&at(&["2026 (root)", "Client", "ACME_SPOT", "1_drive"]), "1_DRIVE").as_deref(), Some("ACME_SPOT"));
    }

    #[test]
    fn a_projects_root_goes_by_the_projects_name() {
        // Frame.io's own name for a project's top folder never becomes a folder on disk.
        let mut f = at(&["root"]);
        f["project_name"] = json!("ACME");
        assert_eq!(default_project_folder(&f, "1_DRIVE").as_deref(), Some("ACME"));
        let mut f = at(&["root", "1_DRIVE"]);
        f["project_name"] = json!("ACME");
        assert_eq!(default_project_folder(&f, "1_DRIVE").as_deref(), Some("ACME"));
        assert_eq!(default_project_folder(&at(&["ACME (root)"]), "1_DRIVE").as_deref(), Some("ACME"));
        assert_eq!(default_project_folder(&json!({ "project_name": "ACME" }), "1_DRIVE").as_deref(), Some("ACME"));
    }

    #[test]
    fn the_project_folder_is_one_folder_name() {
        assert_eq!(default_project_folder(&at(&["2026 (root)", "Job/7 50%"]), "1_DRIVE").as_deref(), Some("Job-7 50-"));
        assert_eq!(default_project_folder(&at(&["2026 (root)", ".."]), "1_DRIVE"), None);
        assert_eq!(default_project_folder(&json!({}), "1_DRIVE"), None);
    }

    #[test]
    fn folder_names_are_one_path_segment() {
        assert_eq!(clean_folder_name("x", " RUSHES ").as_deref(), Ok("RUSHES"));
        for bad in [".", "..", "a/b", "a\\b", "a%2Fb", "a\nb"] {
            assert_eq!(clean_folder_name("x", bad), Err("x must be a single folder name".to_string()), "{bad:?}");
        }
        assert_eq!(clean_folder_name("x", "  "), Err("x must not be empty".to_string()));
        assert!(clean_folder_name("x", &"a".repeat(256)).is_err());
    }
}
