
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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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

fn format_bytes(n: i64) -> String {
    let mut value = n as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if value < 1024.0 || unit == "TB" {
            return if unit == "B" { format!("{value:.0} {unit}") } else { format!("{value:.1} {unit}") };
        }
        value /= 1024.0;
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

pub async fn send_email(config: &EmailConfig, to: &str, subject: &str, text: &str, html: Option<&str>) -> Result<(), EmailError> {
    if !config.configured() {
        return Err(EmailError("Email is not configured".into()));
    }
    let from = config.smtp_from.parse().map_err(|_| EmailError("bad from address".into()))?;
    let to_addr = to.parse().map_err(|_| EmailError("bad to address".into()))?;
    let body = match html {
        Some(h) => MultiPart::alternative_plain_html(text.to_string(), h.to_string()),
        None => MultiPart::alternative_plain_html(text.to_string(), text.to_string()),
    };
    let email = Message::builder()
        .from(from)
        .to(to_addr)
        .subject(subject)
        .multipart(body)
        .map_err(|e| EmailError(e.to_string()))?;

    let base = if config.smtp_use_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
    }
    .map_err(|e| EmailError(e.to_string()))?;
    let mut transport = base.port(config.smtp_port);
    if !config.smtp_username.is_empty() {
        transport = transport.credentials(Credentials::new(config.smtp_username.clone(), config.smtp_password.clone()));
    }
    transport.build().send(email).await.map_err(|e| EmailError(format!("Could not send email: {e}")))?;
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

pub fn render_upload_completion(
    link_name: &str, uploader_name: Option<&str>, uploader_email: Option<&str>, uploader_message: Option<&str>,
    files: &[UploadedFile], total_bytes: i64, duration_seconds: f64, brand: &EmailBrand,
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
        String::new(), format!("Files ({count}):"),
    ];
    for f in files {
        let size = f.size.map(format_bytes).unwrap_or_else(|| "—".to_string());
        lines.push(format!("  • {}  ({size})", f.name));
    }
    lines.push(String::new());
    lines.push(format!("Total:    {}", format_bytes(total_bytes)));
    lines.push(format!("Duration: {}", format_duration(duration_seconds)));
    let text = lines.join("\n");

    let file_rows: String = files.iter().map(|f| {
        let size = f.size.map(format_bytes).unwrap_or_else(|| "—".to_string());
        format!("<li style=\"margin:2px 0\"><span style=\"color:#18181b\">{}</span> <span style=\"color:#a1a1aa\">({})</span></li>", escape(&f.name), escape(&size))
    }).collect();
    let body_html = format!(
        "<p style=\"margin:0 0 16px\">A new upload completed on <strong>{}</strong>.</p>\
<table role=\"presentation\" style=\"border-collapse:collapse;margin:0 0 16px;width:100%\">{}{}{}</table>\
<p style=\"margin:0 0 4px;font-weight:600;color:#18181b\">Files ({count})</p>\
<ul style=\"margin:0 0 16px;padding-left:18px\">{file_rows}</ul>\
<table role=\"presentation\" style=\"border-collapse:collapse;width:100%\">{}{}</table>",
        escape(link_name), meta_row("Uploader", &dash(uploader_name)), meta_row("Email", &dash(uploader_email)),
        meta_row("Message", &dash(uploader_message)), meta_row("Total", &format_bytes(total_bytes)),
        meta_row("Duration", &format_duration(duration_seconds)),
    );
    let html = html_shell(brand, "Upload complete", &body_html);
    (subject, text, html)
}

pub async fn send_upload_completion(
    config: &EmailConfig, link_name: &str, uploader_name: Option<&str>, uploader_email: Option<&str>,
    uploader_message: Option<&str>, files: &[UploadedFile], total_bytes: i64, duration_seconds: f64, brand: &EmailBrand,
) {
    if !config.configured() { return; }
    let (subject, text, html) = render_upload_completion(link_name, uploader_name, uploader_email, uploader_message, files, total_bytes, duration_seconds, brand);
    let _ = send_email(config, &config.resolved_notify_email(), &subject, &text, Some(&html)).await;
}

pub async fn send_test_email(config: &EmailConfig, brand: &EmailBrand) -> Result<(), EmailError> {
    let text = "This is a test email from Portal. If you received it, SMTP is configured correctly.";
    let body_html = "<p style=\"margin:0\">This is a test email from Portal. If you received it, SMTP is configured correctly.</p>";
    let html = html_shell(brand, "Test email", body_html);
    send_email(config, &config.resolved_notify_email(), "Portal test email", text, Some(&html)).await
}
