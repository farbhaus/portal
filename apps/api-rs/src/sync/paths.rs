
//! Path templating + conflict resolution — port of `portal.sync.paths`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use regex::Regex;

pub struct PathContext {
    pub filename: String,
    pub project: String,
    pub folder: String,
    pub subfolder: String,
    pub uploader_name: String,
    pub when: Option<DateTime<Local>>,
}

fn unsafe_re() -> Regex {
    Regex::new(r"[\x00-\x1f/\\]").unwrap()
}
fn token_re() -> Regex {
    Regex::new(r"\{(\w+)\}").unwrap()
}

fn sanitize_segment(value: &str) -> String {
    let cleaned = unsafe_re().replace_all(value, "_");
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned.to_string()
    }
}

fn split_segments(value: &str) -> Vec<String> {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_segment)
        .collect()
}

fn tokens(ctx: &PathContext) -> std::collections::HashMap<String, String> {
    let name = ctx.filename.clone();
    let when = ctx.when.unwrap_or_else(Local::now);
    let p = Path::new(&name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let mut m = std::collections::HashMap::new();
    for (k, v) in [
        ("filename", name.clone()),
        ("basename", name.clone()),
        ("stem", stem),
        ("ext", ext),
        ("project", ctx.project.clone()),
        ("folder", ctx.folder.clone()),
        ("subfolder", ctx.subfolder.clone()),
        ("uploader_name", ctx.uploader_name.clone()),
        ("date", when.format("%Y-%m-%d").to_string()),
        ("year", when.format("%Y").to_string()),
        ("month", when.format("%m").to_string()),
        ("day", when.format("%d").to_string()),
        ("time", when.format("%Hh%M").to_string()),
    ] {
        m.insert(k.to_string(), v);
    }
    m
}

fn join_posix(segments: &[String]) -> String {
    segments
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("/")
}

/// Render a relative POSIX path (always ending in a filename) from a template.
pub fn render_path_template(template: Option<&str>, ctx: &PathContext) -> String {
    let basename = sanitize_segment(&ctx.filename);
    let sub_segments = split_segments(&ctx.subfolder);
    let Some(template) = template.filter(|t| !t.trim().is_empty()) else {
        let mut segs = sub_segments;
        segs.push(basename);
        return join_posix(&segs);
    };

    let tk = tokens(ctx);
    let rendered = token_re().replace_all(template, |caps: &regex::Captures| {
        tk.get(&caps[1]).cloned().unwrap_or_default()
    });
    let mut segments = split_segments(&rendered);
    let names_filename = ["filename", "basename", "stem"].iter().any(|t| template.contains(&format!("{{{t}}}")));
    let insert_sub = if template.contains("{subfolder}") { Vec::new() } else { sub_segments };

    if !names_filename {
        segments.extend(insert_sub);
        segments.push(basename);
    } else if !segments.is_empty() {
        let last = segments.pop().unwrap();
        segments.extend(insert_sub);
        segments.push(last);
    } else {
        segments.extend(insert_sub);
        segments.push(basename);
    }
    join_posix(&segments)
}

/// Render a template into a sanitized relative *directory* path (no filename appended).
pub fn render_session_prefix(template: Option<&str>, uploader_name: &str, when: Option<DateTime<Local>>) -> String {
    let Some(template) = template.filter(|t| !t.trim().is_empty()) else {
        return String::new();
    };
    let ctx = PathContext {
        filename: String::new(),
        project: String::new(),
        folder: String::new(),
        subfolder: String::new(),
        uploader_name: uploader_name.to_string(),
        when,
    };
    let tk = tokens(&ctx);
    let rendered = token_re().replace_all(template, |caps: &regex::Captures| {
        tk.get(&caps[1]).cloned().unwrap_or_default()
    });
    let segments: Vec<String> = rendered
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_segment)
        .collect();
    join_posix(&segments)
}

pub fn resolve_destination(root: &str, template: Option<&str>, ctx: &PathContext) -> PathBuf {
    Path::new(root).join(render_path_template(template, ctx))
}

/// Apply the conflict policy. Returns the path to write, or None to skip.
pub fn resolve_conflict(path: &Path, policy: &str) -> Option<PathBuf> {
    if !path.exists() {
        return Some(path.to_path_buf());
    }
    match policy {
        "overwrite" => Some(path.to_path_buf()),
        "skip" => None,
        _ => {
            // rename_suffix
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            let suffix = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            let mut i = 1;
            loop {
                let candidate = path.with_file_name(format!("{stem}_{i}{suffix}"));
                if !candidate.exists() {
                    return Some(candidate);
                }
                i += 1;
            }
        }
    }
}
