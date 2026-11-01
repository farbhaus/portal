//! An upload link's subfolder template (`{date}/{uploader_name}`, …) rendered into the folder path
//! a client's uploads land under in Frame.io.

use chrono::{DateTime, Local};
use regex::Regex;

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

/// The token's value, or empty for a token an upload path doesn't have.
fn token(name: &str, uploader_name: &str, when: DateTime<Local>) -> String {
    match name {
        "uploader_name" => uploader_name.to_string(),
        "date" => when.format("%Y-%m-%d").to_string(),
        "year" => when.format("%Y").to_string(),
        "month" => when.format("%m").to_string(),
        "day" => when.format("%d").to_string(),
        "time" => when.format("%Hh%M").to_string(),
        _ => String::new(),
    }
}

/// Render a template into a sanitized relative *directory* path (no filename appended).
pub fn render_session_prefix(template: Option<&str>, uploader_name: &str, when: Option<DateTime<Local>>) -> String {
    let Some(template) = template.filter(|t| !t.trim().is_empty()) else {
        return String::new();
    };
    let when = when.unwrap_or_else(Local::now);
    let rendered = token_re().replace_all(template, |caps: &regex::Captures| token(&caps[1], uploader_name, when));
    rendered
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_segment)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn renders_date_and_uploader_and_drops_the_rest() {
        let when = Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 0).unwrap();
        assert_eq!(render_session_prefix(Some("{date}/{uploader_name}"), "Ana", Some(when)), "2026-11-01/Ana");
        assert_eq!(render_session_prefix(Some("{year}/{month}/{day} {time}"), "", Some(when)), "2026/11/01 09h05");
        // A token an upload path doesn't have renders empty, and empty segments go.
        assert_eq!(render_session_prefix(Some("{filename}/{date}"), "", Some(when)), "2026-11-01");
        assert_eq!(render_session_prefix(Some("  "), "Ana", Some(when)), "");
        assert_eq!(render_session_prefix(None, "Ana", Some(when)), "");
    }

    #[test]
    fn a_name_cannot_climb_out_of_its_folder() {
        let when = Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 0).unwrap();
        assert_eq!(render_session_prefix(Some("{uploader_name}"), "../..", Some(when)), "_/_");
        assert_eq!(render_session_prefix(Some("{uploader_name}"), "a\nb", Some(when)), "a_b");
    }
}
