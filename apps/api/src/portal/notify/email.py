"""Email notifications over SMTP (aiosmtplib).

Config is env-first (lib.config Settings: SMTP_*). Email is optional — when SMTP_HOST/SMTP_FROM
are unset, email_configured is false and callers skip sending. Sends are best-effort from the
request path (fire via BackgroundTasks); failures are logged, never surfaced to uploaders.
"""

from dataclasses import dataclass
from email.message import EmailMessage

import aiosmtplib

from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("notify.email")


class EmailError(Exception):
    """An SMTP send failed (or email isn't configured)."""


@dataclass(frozen=True)
class UploadedFile:
    name: str
    size: int | None


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


async def send_email(to: str, subject: str, text: str, html: str | None = None) -> None:
    """Send one email. Raises EmailError if SMTP isn't configured or the send fails."""
    settings = get_settings()
    if not settings.email_configured:
        raise EmailError("Email is not configured")

    message = EmailMessage()
    message["From"] = settings.smtp_from
    message["To"] = to
    message["Subject"] = subject
    message.set_content(text)
    if html:
        message.add_alternative(html, subtype="html")

    try:
        await aiosmtplib.send(
            message,
            hostname=settings.smtp_host,
            port=settings.smtp_port,
            username=settings.smtp_username or None,
            password=settings.smtp_password or None,
            use_tls=settings.smtp_use_tls,
            start_tls=settings.smtp_starttls if not settings.smtp_use_tls else False,
        )
    except Exception as exc:  # aiosmtplib raises various SMTP errors
        log.warning("notify.email.send_failed", to=to, error=str(exc))
        raise EmailError("Could not send email") from exc
    log.info("notify.email.sent", to=to, subject=subject)


def render_upload_completion(
    *,
    link_name: str,
    uploader_name: str | None,
    uploader_email: str | None,
    uploader_message: str | None,
    files: list[UploadedFile],
    total_bytes: int,
    duration_seconds: float,
) -> tuple[str, str]:
    """Build the (subject, text body) for an upload-completion notification."""
    subject = f"New upload: {link_name} ({len(files)} file{'s' if len(files) != 1 else ''})"
    lines = [
        f"A new upload completed on \"{link_name}\".",
        "",
        f"Uploader name:    {uploader_name or '—'}",
        f"Uploader email:   {uploader_email or '—'}",
        f"Message:          {uploader_message or '—'}",
        "",
        f"Files ({len(files)}):",
    ]
    for f in files:
        size = _format_bytes(f.size) if f.size is not None else "—"
        lines.append(f"  • {f.name}  ({size})")
    lines += [
        "",
        f"Total:    {_format_bytes(total_bytes)}",
        f"Duration: {_format_duration(duration_seconds)}",
    ]
    return subject, "\n".join(lines)


async def send_upload_completion(
    *,
    link_name: str,
    uploader_name: str | None,
    uploader_email: str | None,
    uploader_message: str | None,
    files: list[UploadedFile],
    total_bytes: int,
    duration_seconds: float,
) -> None:
    """Send the upload-completion email to the configured notify address (best-effort)."""
    settings = get_settings()
    if not settings.email_configured:
        return
    subject, text = render_upload_completion(
        link_name=link_name,
        uploader_name=uploader_name,
        uploader_email=uploader_email,
        uploader_message=uploader_message,
        files=files,
        total_bytes=total_bytes,
        duration_seconds=duration_seconds,
    )
    try:
        await send_email(settings.resolved_notify_email, subject, text)
    except EmailError:
        pass  # already logged; never let a notification failure break the upload flow


async def send_test_email() -> None:
    """Send a test email to the notify address. Raises EmailError on failure (for the UI)."""
    settings = get_settings()
    await send_email(
        settings.resolved_notify_email,
        "Portal test email",
        "This is a test email from Portal. If you received it, SMTP is configured correctly.",
    )
