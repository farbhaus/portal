"""Email notifications over SMTP (aiosmtplib).

Senders take a resolved EmailConfig (services.runtime_config, DB-backed). Email is optional — when
host/from are unset the config is not "configured" and callers skip sending. Sends are best-effort
from the request path (fire via BackgroundTasks); failures are logged, never surfaced to uploaders.
"""

from dataclasses import dataclass
from email.message import EmailMessage
from html import escape
from typing import TYPE_CHECKING

import aiosmtplib

from portal.lib.logging import get_logger

if TYPE_CHECKING:
    from portal.services.runtime_config import EmailConfig

log = get_logger("notify.email")

# Default brand styling for emails when no app branding is configured. Mirrors the dark-on-light,
# amber-accent look of the admin UI so emails feel like part of the product, not a system alert.
_DEFAULT_BRAND_NAME = "Portal"
_DEFAULT_ACCENT = "#f59e0b"


class EmailError(Exception):
    """An SMTP send failed (or email isn't configured)."""


@dataclass(frozen=True)
class EmailBrand:
    """Branding applied to the HTML email shell (header bar + accents)."""

    name: str = _DEFAULT_BRAND_NAME
    accent_color: str = _DEFAULT_ACCENT
    logo_url: str | None = None


@dataclass(frozen=True)
class UploadedFile:
    name: str
    size: int | None


def _html_shell(*, brand: EmailBrand, heading: str, body_html: str) -> str:
    """Wrap content in a responsive, email-client-safe HTML document.

    Uses only inline styles and table-free block layout, which renders consistently across
    Gmail/Outlook/Apple Mail. `body_html` is assumed to already be escaped/trusted markup.
    """
    accent = escape(brand.accent_color)
    name = escape(brand.name)
    header = (
        f'<img src="{escape(brand.logo_url)}" alt="{name}" '
        'style="max-height:32px;max-width:180px;display:block">'
        if brand.logo_url
        else f'<span style="font-size:18px;font-weight:700;color:#ffffff">{name}</span>'
    )
    return (
        '<!DOCTYPE html><html><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1">'
        # Pin the light scheme so Gmail/Outlook dark-mode don't invert this hand-tuned design.
        '<meta name="color-scheme" content="light">'
        '<meta name="supported-color-schemes" content="light"></head>'
        '<body style="margin:0;padding:0;color-scheme:light;background:#f4f4f5;'
        "font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,"
        'Helvetica,Arial,sans-serif">'
        '<div style="max-width:560px;margin:0 auto;padding:24px 16px">'
        '<div style="background:#ffffff;border-radius:12px;overflow:hidden;'
        'border:1px solid #e4e4e7">'
        f'<div style="background:#18181b;padding:16px 24px;border-bottom:3px solid {accent}">'
        f"{header}</div>"
        '<div style="padding:24px">'
        '<h1 style="margin:0 0 16px;font-size:20px;font-weight:600;color:#18181b">'
        f"{escape(heading)}</h1>"
        f'<div style="font-size:14px;line-height:1.6;color:#3f3f46">{body_html}</div>'
        "</div></div>"
        f'<p style="margin:16px 4px 0;font-size:12px;color:#a1a1aa">Sent by {name}.</p>'
        "</div></body></html>"
    )


def _format_bytes(n: int) -> str:
    value = float(n)
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if value < 1024 or unit == "TB":
            return f"{value:.0f} {unit}" if unit == "B" else f"{value:.1f} {unit}"
        value /= 1024
    return f"{n} B"


def _format_duration(seconds: float) -> str:
    seconds = int(seconds)
    if seconds < 60:
        return f"{seconds}s"
    minutes, secs = divmod(seconds, 60)
    if minutes < 60:
        return f"{minutes}m {secs}s"
    hours, minutes = divmod(minutes, 60)
    return f"{hours}h {minutes}m"


async def send_verification_code(
    config: "EmailConfig",
    to: str,
    code: str,
    ttl_minutes: int,
    brand: EmailBrand | None = None,
) -> None:
    """Email a one-time verification code (links with verify_email on)."""
    brand = brand or EmailBrand()
    subject = "Your verification code"
    text = (
        f"Your verification code is {code}\n\n"
        f"It expires in {ttl_minutes} minutes. "
        "If you didn't request this, you can ignore this email."
    )
    body_html = (
        "<p style=\"margin:0 0 8px\">Use this code to verify your email address:</p>"
        f'<div style="margin:8px 0 16px;padding:14px;background:#f4f4f5;border-radius:8px;'
        f"font-size:30px;font-weight:700;letter-spacing:6px;text-align:center;"
        f'color:{escape(brand.accent_color)}">{escape(code)}</div>'
        f'<p style="margin:0;color:#71717a">It expires in {ttl_minutes} minutes. '
        "If you didn't request this, you can ignore this email.</p>"
    )
    html = _html_shell(brand=brand, heading="Verify your email", body_html=body_html)
    await send_email(config, to, subject, text, html)


async def send_email(
    config: "EmailConfig", to: str, subject: str, text: str, html: str | None = None
) -> None:
    """Send one email. Raises EmailError if SMTP isn't configured or the send fails."""
    if not config.configured:
        raise EmailError("Email is not configured")

    message = EmailMessage()
    message["From"] = config.smtp_from
    message["To"] = to
    message["Subject"] = subject
    message.set_content(text)
    if html:
        message.add_alternative(html, subtype="html")

    try:
        await aiosmtplib.send(
            message,
            hostname=config.smtp_host,
            port=config.smtp_port,
            username=config.smtp_username or None,
            password=config.smtp_password or None,
            use_tls=config.smtp_use_tls,
            start_tls=config.smtp_starttls if not config.smtp_use_tls else False,
        )
    except Exception as exc:  # aiosmtplib raises various SMTP errors
        log.warning("notify.email.send_failed", to=to, error=str(exc))
        raise EmailError("Could not send email") from exc
    log.info("notify.email.sent", to=to, subject=subject)


def _meta_row(label: str, value: str) -> str:
    return (
        '<tr>'
        f'<td style="padding:4px 12px 4px 0;color:#71717a;white-space:nowrap;vertical-align:top">'
        f"{escape(label)}</td>"
        f'<td style="padding:4px 0;color:#18181b">{escape(value)}</td></tr>'
    )


def render_upload_completion(
    *,
    link_name: str,
    uploader_name: str | None,
    uploader_email: str | None,
    uploader_message: str | None,
    files: list[UploadedFile],
    total_bytes: int,
    duration_seconds: float,
    brand: EmailBrand | None = None,
) -> tuple[str, str, str]:
    """Build the (subject, text body, html body) for an upload-completion notification."""
    brand = brand or EmailBrand()
    count = len(files)
    subject = f"New upload: {link_name} ({count} file{'s' if count != 1 else ''})"

    lines = [
        f'A new upload completed on "{link_name}".',
        "",
        f"Uploader name:    {uploader_name or '—'}",
        f"Uploader email:   {uploader_email or '—'}",
        f"Message:          {uploader_message or '—'}",
        "",
        f"Files ({count}):",
    ]
    for f in files:
        size = _format_bytes(f.size) if f.size is not None else "—"
        lines.append(f"  • {f.name}  ({size})")
    lines += [
        "",
        f"Total:    {_format_bytes(total_bytes)}",
        f"Duration: {_format_duration(duration_seconds)}",
    ]
    text = "\n".join(lines)

    file_rows = "".join(
        '<li style="margin:2px 0">'
        f'<span style="color:#18181b">{escape(f.name)}</span> '
        f'<span style="color:#a1a1aa">'
        f"({escape(_format_bytes(f.size) if f.size is not None else '—')})</span></li>"
        for f in files
    )
    body_html = (
        f'<p style="margin:0 0 16px">A new upload completed on '
        f'<strong>{escape(link_name)}</strong>.</p>'
        '<table role="presentation" style="border-collapse:collapse;margin:0 0 16px;width:100%">'
        + _meta_row("Uploader", uploader_name or "—")
        + _meta_row("Email", uploader_email or "—")
        + _meta_row("Message", uploader_message or "—")
        + "</table>"
        f'<p style="margin:0 0 4px;font-weight:600;color:#18181b">Files ({count})</p>'
        f'<ul style="margin:0 0 16px;padding-left:18px">{file_rows}</ul>'
        '<table role="presentation" style="border-collapse:collapse;width:100%">'
        + _meta_row("Total", _format_bytes(total_bytes))
        + _meta_row("Duration", _format_duration(duration_seconds))
        + "</table>"
    )
    html = _html_shell(brand=brand, heading="Upload complete", body_html=body_html)
    return subject, text, html


async def send_upload_completion(
    config: "EmailConfig",
    *,
    link_name: str,
    uploader_name: str | None,
    uploader_email: str | None,
    uploader_message: str | None,
    files: list[UploadedFile],
    total_bytes: int,
    duration_seconds: float,
    brand: EmailBrand | None = None,
) -> None:
    """Send the upload-completion email to the configured notify address (best-effort)."""
    if not config.configured:
        return
    subject, text, html = render_upload_completion(
        link_name=link_name,
        uploader_name=uploader_name,
        uploader_email=uploader_email,
        uploader_message=uploader_message,
        files=files,
        total_bytes=total_bytes,
        duration_seconds=duration_seconds,
        brand=brand,
    )
    try:
        await send_email(config, config.resolved_notify_email, subject, text, html)
    except EmailError:
        pass  # already logged; never let a notification failure break the upload flow


async def send_test_email(config: "EmailConfig", brand: EmailBrand | None = None) -> None:
    """Send a test email to the notify address. Raises EmailError on failure (for the UI)."""
    brand = brand or EmailBrand()
    text = "This is a test email from Portal. If you received it, SMTP is configured correctly."
    body_html = (
        '<p style="margin:0">This is a test email from Portal. '
        "If you received it, SMTP is configured correctly.</p>"
    )
    html = _html_shell(brand=brand, heading="Test email", body_html=body_html)
    await send_email(config, config.resolved_notify_email, "Portal test email", text, html)
