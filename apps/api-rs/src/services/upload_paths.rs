//! An upload link's subfolder template (`{date}/{uploader_name}`, …) rendered into the folder path
//! a client's uploads land under in Frame.io.

use chrono::{DateTime, Local};
use regex::Regex;
use serde_json::{json, Value};

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

/// What the uploader entered on the upload page, for the tokens that use it.
#[derive(Default)]
pub struct Uploader<'a> {
    pub name: &'a str,
    pub project: &'a str,
}

/// The token's value, or empty for a token an upload path doesn't have.
fn token(name: &str, uploader: &Uploader, when: DateTime<Local>) -> String {
    match name {
        "uploader_name" => uploader.name.to_string(),
        "project" => uploader.project.to_string(),
        "date" => when.format("%Y-%m-%d").to_string(),
        "year" => when.format("%Y").to_string(),
        "month" => when.format("%m").to_string(),
        "day" => when.format("%d").to_string(),
        "time" => when.format("%Hh%M").to_string(),
        _ => String::new(),
    }
}

/// A link's stored asks, plus the upload-page fields its template's tokens need: `{uploader_name}`
/// asks for the name, `{project}` for the project. Without them those folders would never fill.
pub fn with_template_asks(fields: Option<Value>, template: Option<&str>) -> Value {
    let mut fields = fields.filter(Value::is_object).unwrap_or_else(|| json!({}));
    for caps in token_re().captures_iter(template.unwrap_or("")) {
        match &caps[1] {
            "uploader_name" => fields["name"] = json!(true),
            "project" => fields["project"] = json!(true),
            _ => {}
        }
    }
    fields
}

/// Render a template into a sanitized relative *directory* path (no filename appended).
pub fn render_session_prefix(template: Option<&str>, uploader: &Uploader, when: Option<DateTime<Local>>) -> String {
    let Some(template) = template.filter(|t| !t.trim().is_empty()) else {
        return String::new();
    };
    let when = when.unwrap_or_else(Local::now);
    let rendered = token_re().replace_all(template, |caps: &regex::Captures| token(&caps[1], uploader, when));
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

    fn named(name: &str) -> Uploader<'_> {
        Uploader { name, ..Default::default() }
    }

    #[test]
    fn renders_date_and_uploader_and_drops_the_rest() {
        let when = Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 0).unwrap();
        assert_eq!(render_session_prefix(Some("{date}/{uploader_name}"), &named("Ana"), Some(when)), "2026-11-01/Ana");
        assert_eq!(render_session_prefix(Some("{year}/{month}/{day} {time}"), &named(""), Some(when)), "2026/11/01 09h05");
        // A token an upload path doesn't have renders empty, and empty segments go.
        assert_eq!(render_session_prefix(Some("{filename}/{date}"), &named(""), Some(when)), "2026-11-01");
        assert_eq!(render_session_prefix(Some("  "), &named("Ana"), Some(when)), "");
        assert_eq!(render_session_prefix(None, &named("Ana"), Some(when)), "");
    }

    #[test]
    fn renders_the_project() {
        let when = Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 0).unwrap();
        let ana = Uploader { name: "Ana", project: "Harbour Lights" };
        assert_eq!(render_session_prefix(Some("{project}/{date}/{uploader_name}"), &ana, Some(when)), "Harbour Lights/2026-11-01/Ana");
        // A session without one (started before the link asked) loses that level.
        assert_eq!(render_session_prefix(Some("{project}/{date}"), &named("Ana"), Some(when)), "2026-11-01");
    }

    #[test]
    fn a_template_asks_for_what_it_uses() {
        let stored = json!({ "email": true, "name": false });
        assert_eq!(
            with_template_asks(Some(stored.clone()), Some("{project}/{date}/{uploader_name}")),
            json!({ "email": true, "name": true, "project": true })
        );
        assert_eq!(with_template_asks(Some(stored.clone()), Some("{date}")), stored);
        assert_eq!(with_template_asks(None, None), json!({}));
        assert_eq!(with_template_asks(Some(Value::Null), Some("{project}")), json!({ "project": true }));
    }

    #[test]
    fn a_name_cannot_climb_out_of_its_folder() {
        let when = Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 0).unwrap();
        assert_eq!(render_session_prefix(Some("{uploader_name}"), &named("../.."), Some(when)), "_/_");
        assert_eq!(render_session_prefix(Some("{uploader_name}"), &named("a\nb"), Some(when)), "a_b");
        let up = Uploader { project: "../x", ..Default::default() };
        assert_eq!(render_session_prefix(Some("{project}"), &up, Some(when)), "_/x");
    }
}
