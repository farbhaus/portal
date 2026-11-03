
//! Email notifications over SMTP — port of `portal.notify.email`.
//! Email is optional: when host/from are unset the config is not "configured" and callers skip.

use lettre::message::MultiPart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::services::runtime_config::{EmailBrand, EmailConfig};

#[derive(Debug)]
pub struct EmailError(pub String);
impl std::fmt::Display for EmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for EmailError {}

#[derive(Clone)]
pub struct UploadedFile {
    pub name: String,
    pub size: Option<i64>,
}

fn escape(s: &str) -> String {
    // Same set as Python's html.escape(quote=True).
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

fn html_shell(brand: &EmailBrand, heading: &str, body_html: &str) -> String {
    let accent = escape(&brand.accent_color);
    let name = escape(&brand.name);
    let header = match &brand.logo_url {
        Some(url) => format!(
            "<img src=\"{}\" alt=\"{name}\" style=\"max-height:32px;max-width:180px;display:block\">",
            escape(url)
        ),
        None => format!("<span style=\"font-size:18px;font-weight:700;color:#ffffff\">{name}</span>"),
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"color-scheme\" content=\"light\">\
<meta name=\"supported-color-schemes\" content=\"light\"></head>\
<body style=\"margin:0;padding:0;color-scheme:light;background:#f4f4f5;\
font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif\">\
<div style=\"max-width:560px;margin:0 auto;padding:24px 16px\">\
<div style=\"background:#ffffff;border-radius:12px;overflow:hidden;border:1px solid #e4e4e7\">\
<div style=\"background:#18181b;padding:16px 24px;border-bottom:3px solid {accent}\">{header}</div>\
<div style=\"padding:24px\">\
<h1 style=\"margin:0 0 16px;font-size:20px;font-weight:600;color:#18181b\">{heading_esc}</h1>\
<div style=\"font-size:14px;line-height:1.6;color:#3f3f46\">{body_html}</div>\
</div></div>\
<p style=\"margin:16px 4px 0;font-size:12px;color:#a1a1aa\">Sent by {name}.</p>\
</div></body></html>",
        heading_esc = escape(heading),
    )
}

/// Decimal units, as Finder, Frame.io and the web app count them (lib/format.ts `formatBytes`).
fn format_bytes(n: i64) -> String {
    let mut value = n as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if value < 1000.0 || unit == "TB" {
            return match unit {
                "B" => format!("{value:.0} {unit}"),
                _ if value >= 100.0 => format!("{value:.0} {unit}"),
                _ => format!("{value:.1} {unit}"),
            };
        }
        value /= 1000.0;
    }
    format!("{n} B")
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds as i64;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, secs) = (seconds / 60, seconds % 60);
    if minutes < 60 {
        return format!("{minutes}m {secs}s");
    }
    let (hours, minutes) = (minutes / 60, minutes % 60);
    format!("{hours}h {minutes}m")
}

/// The test-email error for a rejected SMTP login (535).
pub const AUTH_REJECTED: &str = "the mail server rejected the username or password. iCloud, Gmail and other \
     providers with two-factor sign-in need an app-specific password here, not your account password";

/// Every caller sends to exactly one person; never let an address list fan a message out.
fn single_recipient(to: &str) -> bool {
    to.parse::<lettre::message::Mailboxes>().map(|m| m.into_iter().count()).unwrap_or(1) == 1
}

pub async fn send_email(config: &EmailConfig, to: &str, subject: &str, text: &str, html: Option<&str>) -> Result<(), EmailError> {
    if !config.configured() {
        return Err(EmailError("Email is not configured".into()));
    }
    if !single_recipient(to) {
        return Err(EmailError("Refusing to send to more than one recipient".into()));
    }
    let from = config.smtp_from.parse().map_err(|_| EmailError("bad from address".into()))?;
    let to_addr = to.parse().map_err(|_| EmailError("bad to address".into()))?;
    let builder = Message::builder().from(from).to(to_addr).subject(subject);
    let email = match html {
        Some(h) => builder.multipart(MultiPart::alternative_plain_html(text.to_string(), h.to_string())),
        None => builder.singlepart(lettre::message::SinglePart::plain(text.to_string())),
    }
    .map_err(|e| EmailError(e.to_string()))?;

    // Implicit TLS, else STARTTLS when enabled, else plain SMTP (a local relay) — as Python's
    // aiosmtplib.send(use_tls=..., start_tls=...).
    let base = if config.smtp_use_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
    } else if config.smtp_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
    } else {
        Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host))
    }
    .map_err(|e| EmailError(e.to_string()))?;
    let mut transport = base.port(config.smtp_port);
    if !config.smtp_username.is_empty() {
        transport = transport.credentials(Credentials::new(config.smtp_username.clone(), config.smtp_password.clone()));
    }
    if let Err(e) = transport.build().send(email).await {
        tracing::warn!("notify.email.send_failed to={to} error={e}");
        // 535: the server refused the login. With iCloud, Gmail and other providers that use
        // two-factor sign-in that is almost always the account password where an app-specific
        // password is required — say so, rather than a bare "could not send".
        if e.status().is_some_and(|code| code.to_string() == "535") {
            return Err(EmailError(AUTH_REJECTED.into()));
        }
        return Err(EmailError("Could not send email".into()));
    }
    tracing::info!("notify.email.sent to={to} subject={subject}");
    Ok(())
}

pub async fn send_verification_code(config: &EmailConfig, to: &str, code: &str, ttl_minutes: i64, brand: &EmailBrand) -> Result<(), EmailError> {
    let subject = "Your verification code";
    let text = format!("Your verification code is {code}\n\nIt expires in {ttl_minutes} minutes. If you didn't request this, you can ignore this email.");
    let body_html = format!(
        "<p style=\"margin:0 0 8px\">Use this code to verify your email address:</p>\
<div style=\"margin:8px 0 16px;padding:14px;background:#f4f4f5;border-radius:8px;\
font-size:30px;font-weight:700;letter-spacing:6px;text-align:center;color:{accent}\">{code_esc}</div>\
<p style=\"margin:0;color:#71717a\">It expires in {ttl_minutes} minutes. If you didn't request this, you can ignore this email.</p>",
        accent = escape(&brand.accent_color),
        code_esc = escape(code),
    );
    let html = html_shell(brand, "Verify your email", &body_html);
    send_email(config, to, subject, &text, Some(&html)).await
}

fn meta_row(label: &str, value: &str) -> String {
    format!(
        "<tr><td style=\"padding:4px 12px 4px 0;color:#71717a;white-space:nowrap;vertical-align:top\">{}</td><td style=\"padding:4px 0;color:#18181b\">{}</td></tr>",
        escape(label), escape(value)
    )
}

fn file_size_label(f: &UploadedFile) -> String {
    f.size.map(format_bytes).unwrap_or_else(|| "—".to_string())
}

fn file_lines(files: &[UploadedFile]) -> Vec<String> {
    files.iter().map(|f| format!("  • {}  ({})", f.name, file_size_label(f))).collect()
}

fn file_list_html(files: &[UploadedFile]) -> String {
    let rows: String = files
        .iter()
        .map(|f| {
            format!(
                "<li style=\"margin:2px 0\"><span style=\"color:#18181b\">{}</span> <span style=\"color:#a1a1aa\">({})</span></li>",
                escape(&f.name),
                escape(&file_size_label(f))
            )
        })
        .collect();
    format!("<ul style=\"margin:0 0 16px;padding-left:18px\">{rows}</ul>")
}

#[allow(clippy::too_many_arguments)] // mirrors Python's keyword-only render_upload_completion
pub fn render_upload_completion(
    link_name: &str, uploader_name: Option<&str>, uploader_email: Option<&str>, uploader_message: Option<&str>,
    uploader_project: Option<&str>, files: &[UploadedFile], total_bytes: i64, duration_seconds: f64, brand: &EmailBrand,
) -> (String, String, String) {
    let count = files.len();
    let plural = if count != 1 { "s" } else { "" };
    let subject = format!("New upload: {link_name} ({count} file{plural})");
    let dash = |v: Option<&str>| v.unwrap_or("—").to_string();
    let mut lines = vec![
        format!("A new upload completed on \"{link_name}\"."), String::new(),
        format!("Uploader name:    {}", dash(uploader_name)),
        format!("Uploader email:   {}", dash(uploader_email)),
        format!("Message:          {}", dash(uploader_message)),
    ];
    // Only links that ask for a project have one.
    if let Some(p) = uploader_project {
        lines.push(format!("Project:          {p}"));
    }
    lines.extend([String::new(), format!("Files ({count}):")]);
    lines.extend(file_lines(files));
    lines.push(String::new());
    lines.push(format!("Total:    {}", format_bytes(total_bytes)));
    lines.push(format!("Duration: {}", format_duration(duration_seconds)));
    let text = lines.join("\n");

    let body_html = format!(
        "<p style=\"margin:0 0 16px\">A new upload completed on <strong>{}</strong>.</p>\
<table role=\"presentation\" style=\"border-collapse:collapse;margin:0 0 16px;width:100%\">{}{}{}{}</table>\
<p style=\"margin:0 0 4px;font-weight:600;color:#18181b\">Files ({count})</p>\
{file_list}\
<table role=\"presentation\" style=\"border-collapse:collapse;width:100%\">{}{}</table>",
        escape(link_name), meta_row("Uploader", &dash(uploader_name)), meta_row("Email", &dash(uploader_email)),
        meta_row("Message", &dash(uploader_message)), uploader_project.map(|p| meta_row("Project", p)).unwrap_or_default(),
        meta_row("Total", &format_bytes(total_bytes)),
        meta_row("Duration", &format_duration(duration_seconds)),
        file_list = file_list_html(files),
    );
    let html = html_shell(brand, "Upload complete", &body_html);
    (subject, text, html)
}

#[allow(clippy::too_many_arguments)] // mirrors Python's keyword-only send_upload_completion
/// `to`: the link's own notification addresses; empty means the app's notify address.
pub async fn send_upload_completion(
    config: &EmailConfig, to: &[String], link_name: &str, uploader_name: Option<&str>, uploader_email: Option<&str>,
    uploader_message: Option<&str>, uploader_project: Option<&str>, files: &[UploadedFile], total_bytes: i64, duration_seconds: f64,
    brand: &EmailBrand,
) {
    if !config.configured() { return; }
    let (subject, text, html) =
        render_upload_completion(link_name, uploader_name, uploader_email, uploader_message, uploader_project, files, total_bytes, duration_seconds, brand);
    let fallback = [config.resolved_notify_email()];
    for addr in if to.is_empty() { &fallback[..] } else { to } {
        let _ = send_email(config, addr, &subject, &text, Some(&html)).await;
    }
}

/// The (subject, text, html) of the uploader's delivery receipt (issue #86): the list of files
/// that arrived, for them to keep. Carries nothing the uploader didn't send.
pub fn render_upload_receipt(link_name: &str, files: &[UploadedFile], total_bytes: i64, brand: &EmailBrand) -> (String, String, String) {
    let count = files.len();
    let noun = format!("{count} file{}", if count != 1 { "s" } else { "" });
    let subject = format!("Delivered to {link_name}: {noun}");
    let mut lines = vec![
        format!("Your upload to \"{link_name}\" is complete. These files were delivered:"),
        String::new(),
    ];
    lines.extend(file_lines(files));
    lines.push(String::new());
    lines.push(format!("Total: {noun}, {}", format_bytes(total_bytes)));
    let text = lines.join("\n");
    let body_html = format!(
        "<p style=\"margin:0 0 16px\">Your upload to <strong>{}</strong> is complete. These files were delivered:</p>\
{}<table role=\"presentation\" style=\"border-collapse:collapse;width:100%\">{}</table>",
        escape(link_name),
        file_list_html(files),
        meta_row("Total", &format!("{noun}, {}", format_bytes(total_bytes))),
    );
    let html = html_shell(brand, "Upload complete", &body_html);
    (subject, text, html)
}

/// Email the uploader their delivery receipt (best-effort). Only ever to an address the uploader
/// proved they own — checking that is the caller's job (see `routes::public::complete_session`);
/// mailing typed-in addresses would let anyone make Portal send mail to any inbox.
pub async fn send_upload_receipt(config: &EmailConfig, to: &str, link_name: &str, files: &[UploadedFile], total_bytes: i64, brand: &EmailBrand) {
    if !config.configured() {
        return;
    }
    let (subject, text, html) = render_upload_receipt(link_name, files, total_bytes, brand);
    let _ = send_email(config, to, &subject, &text, Some(&html)).await; // already logged
}

/// The (subject, text, html) of the reminder that a link expires within a day.
pub fn render_expiry_reminder(name: &str, expires_at: &chrono::DateTime<chrono::Utc>, admin_url: &str, public_url: &str, brand: &EmailBrand) -> (String, String, String) {
    // The server's clock zone (the container's TZ, UTC by default) is named, and the relative time
    // leads, so the hour can't be misread from another zone.
    let local = expires_at.with_timezone(&chrono::Local);
    let zone = if local.offset().local_minus_utc() == 0 { "UTC".to_string() } else { local.format("UTC%:z").to_string() };
    let hours = (*expires_at - chrono::Utc::now()).num_minutes().max(0) as f64 / 60.0;
    let soon = if hours < 1.5 { "within the hour".to_string() } else { format!("in about {} hours", hours.round()) };
    let when = format!("{soon} ({} {zone})", local.format("%a %-d %b at %H:%M"));
    let subject = format!("“{name}” expires {soon}");
    let text = format!(
        "The link “{name}” stops working {when}.\n\nExtend it or let it close: {admin_url}\n\nThe link itself: {public_url}"
    );
    let body_html = format!(
        "<p style=\"margin:0 0 16px\">The link <strong>{}</strong> stops working <strong>{}</strong>.</p>\
<p style=\"margin:0 0 16px\"><a href=\"{admin}\" style=\"display:inline-block;background:{accent};color:#111111;\
text-decoration:none;font-weight:600;padding:10px 18px;border-radius:8px\">Extend or review it</a></p>\
<p style=\"margin:0;font-size:12px;color:#71717a;word-break:break-all\">{public}</p>",
        escape(name),
        escape(&when),
        admin = escape(admin_url),
        public = escape(public_url),
        accent = escape(&brand.accent_color),
    );
    let html = html_shell(brand, "A link expires soon", &body_html);
    (subject, text, html)
}

/// The (subject, text, html) of a link sent to a client from the admin: what the link is for, the
/// link itself, and an optional note from the sender.
pub fn render_link_invite(kind: &str, title: &str, url: &str, note: Option<&str>, brand: &EmailBrand) -> (String, String, String) {
    let (subject, lead, cta) = if kind == "download" {
        (format!("{}: files for you", brand.name), format!("{} shared files with you: \"{title}\".", brand.name), "Open and download")
    } else {
        (format!("{}: send your files", brand.name), format!("{} asked you to upload files: \"{title}\".", brand.name), "Open and upload")
    };
    let mut lines = vec![lead.clone(), String::new()];
    if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
        lines.push(n.trim().to_string());
        lines.push(String::new());
    }
    lines.push(url.to_string());
    let text = lines.join("\n");
    let note_html = note
        .filter(|n| !n.trim().is_empty())
        .map(|n| format!("<p style=\"margin:0 0 16px;white-space:pre-line\">{}</p>", escape(n.trim())))
        .unwrap_or_default();
    let body_html = format!(
        "<p style=\"margin:0 0 16px\">{}</p>{note_html}\
<p style=\"margin:0 0 16px\"><a href=\"{url_esc}\" style=\"display:inline-block;background:{accent};color:#111111;\
text-decoration:none;font-weight:600;padding:10px 18px;border-radius:8px\">{cta}</a></p>\
<p style=\"margin:0;font-size:12px;color:#71717a;word-break:break-all\">{url_esc}</p>",
        escape(&lead),
        url_esc = escape(url),
        accent = escape(&brand.accent_color),
    );
    let html = html_shell(brand, title, &body_html);
    (subject, text, html)
}

pub async fn send_test_email(config: &EmailConfig, brand: &EmailBrand) -> Result<(), EmailError> {
    let text = "This is a test email from Portal. If you received it, SMTP is configured correctly.";
    let body_html = "<p style=\"margin:0\">This is a test email from Portal. If you received it, SMTP is configured correctly.</p>";
    let html = html_shell(brand, "Test email", body_html);
    send_email(config, &config.resolved_notify_email(), "Portal test email", text, Some(&html)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brand() -> EmailBrand {
        EmailBrand { name: "Acme Post".into(), accent_color: "#ff8800".into(), logo_url: None }
    }

    #[test]
    fn link_invite_carries_the_link_and_escapes_the_note() {
        let (subject, text, html) = render_link_invite(
            "upload",
            "Send us your Day 3 cards",
            "https://portal.example/u/abc",
            Some("Cards <A001> & <A002>"),
            &brand(),
        );
        assert_eq!(subject, "Acme Post: send your files");
        assert!(text.contains("https://portal.example/u/abc"));
        assert!(text.contains("Cards <A001> & <A002>"));
        assert!(html.contains("href=\"https://portal.example/u/abc\""));
        assert!(html.contains("Cards &lt;A001&gt; &amp; &lt;A002&gt;"));
        assert!(!html.contains("<A001>"));
        let (subject, _, html) = render_link_invite("download", "Grade v4", "https://portal.example/d/x", None, &brand());
        assert_eq!(subject, "Acme Post: files for you");
        assert!(html.contains("Open and download"));
    }

    #[test]
    fn expiry_reminder_says_when_relatively_and_names_the_zone() {
        let at = chrono::Utc::now() + chrono::Duration::hours(20);
        let (subject, text, html) =
            render_expiry_reminder("Cards <A>", &at, "https://portal.example/upload-links/1", "https://portal.example/u/t", &brand());
        assert_eq!(subject, "“Cards <A>” expires in about 20 hours");
        assert!(text.contains("in about 20 hours (") && text.contains(" UTC"), "{text}");
        assert!(html.contains("Cards &lt;A&gt;") && html.contains("href=\"https://portal.example/upload-links/1\""));
    }

    #[test]
    fn sizes_are_decimal_like_the_web_app() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(93_000_000_000), "93.0 GB");
        assert_eq!(format_bytes(164_000_000_000), "164 GB");
        assert_eq!(format_bytes(1_500_000_000_000), "1.5 TB");
    }

    #[test]
    fn refuses_address_lists() {
        assert!(single_recipient("client@example.com"));
        assert!(!single_recipient("client@example.com, eve@example.com"));
        assert!(!single_recipient("a@example.com,b@example.com"));
    }

    #[test]
    fn receipt_lists_the_delivered_files() {
        let files = vec![
            UploadedFile { name: "A001.mov".into(), size: Some(2048) },
            UploadedFile { name: "<b>.wav".into(), size: None },
        ];
        let (subject, text, html) = render_upload_receipt("Acme Deliveries", &files, 2048, &brand());
        assert_eq!(subject, "Delivered to Acme Deliveries: 2 files");
        assert!(text.starts_with("Your upload to \"Acme Deliveries\" is complete. These files were delivered:"));
        assert!(text.contains("  • A001.mov  (2.0 KB)"));
        assert!(text.contains("  • <b>.wav  (—)"));
        assert!(text.ends_with("Total: 2 files, 2.0 KB"));
        assert!(html.contains("&lt;b&gt;.wav")); // file names are escaped in the HTML part
        let (one, _, _) = render_upload_receipt("X", &files[..1], 1, &brand());
        assert_eq!(one, "Delivered to X: 1 file");
    }

    #[test]
    fn completion_names_the_project_only_when_given() {
        let files = vec![UploadedFile { name: "A001.mov".into(), size: Some(2048) }];
        let (_, text, html) = render_upload_completion("Cards", Some("Ana"), None, None, Some("Harbour <Lights>"), &files, 2048, 3.0, &brand());
        assert!(text.contains("Message:          —\nProject:          Harbour <Lights>\n\nFiles (1):"));
        assert!(html.contains("Harbour &lt;Lights&gt;"));
        let (_, text, html) = render_upload_completion("Cards", Some("Ana"), None, None, None, &files, 2048, 3.0, &brand());
        assert!(!text.contains("Project:") && !html.contains("Project"));
    }

    #[test]
    fn escapes_like_python_html_escape() {
        assert_eq!(escape("<a href=\"x\">Tom's & co</a>"), "&lt;a href=&quot;x&quot;&gt;Tom&#x27;s &amp; co&lt;/a&gt;");
    }
}
