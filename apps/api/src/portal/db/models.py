"""Full Portal domain schema (SQLAlchemy 2.0 declarative).

The source/destination config columns are JSONB and type-tagged (e.g. {"type": "frameio", ...})
so phase 2 S3 backends slot in additively without a schema rewrite.
"""

import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import (
    BigInteger,
    Boolean,
    DateTime,
    ForeignKey,
    Integer,
    LargeBinary,
    String,
    Text,
    func,
)
from sqlalchemy.dialects.postgresql import JSONB, UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from portal.db.base import Base, TimestampMixin


def _uuid_pk() -> Mapped[uuid.UUID]:
    return mapped_column(
        UUID(as_uuid=True), primary_key=True, server_default=func.gen_random_uuid()
    )


class User(Base, TimestampMixin):
    __tablename__ = "users"

    id: Mapped[uuid.UUID] = _uuid_pk()
    email: Mapped[str] = mapped_column(String(255), unique=True, index=True, nullable=False)
    password_hash: Mapped[str] = mapped_column(String(255), nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    # TOTP 2FA (opt-in second factor for password logins). Secret is AES-GCM encrypted at rest.
    totp_secret_encrypted: Mapped[str | None] = mapped_column(Text)
    totp_enabled: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    # One-time recovery codes, stored as sha256 hashes; entries are removed as they're used.
    totp_recovery_codes: Mapped[list[Any] | None] = mapped_column(JSONB)

    frameio_connections: Mapped[list["FrameioConnection"]] = relationship(
        back_populates="user", cascade="all, delete-orphan"
    )
    webauthn_credentials: Mapped[list["WebauthnCredential"]] = relationship(
        back_populates="user", cascade="all, delete-orphan"
    )


class WebauthnCredential(Base, TimestampMixin):
    """A registered passkey (WebAuthn credential) for passwordless login."""

    __tablename__ = "webauthn_credentials"

    id: Mapped[uuid.UUID] = _uuid_pk()
    user_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True, nullable=False
    )
    # Base64url-encoded credential id (the authenticator's handle), unique per credential.
    credential_id: Mapped[str] = mapped_column(String(512), unique=True, index=True, nullable=False)
    public_key: Mapped[str] = mapped_column(Text, nullable=False)  # base64
    sign_count: Mapped[int] = mapped_column(BigInteger, default=0, nullable=False)
    transports: Mapped[list[Any] | None] = mapped_column(JSONB)
    name: Mapped[str | None] = mapped_column(String(255))
    last_used_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))

    user: Mapped[User] = relationship(back_populates="webauthn_credentials")


class AppSettings(Base, TimestampMixin):
    """Single-row store for app-wide configuration: branding, email (SMTP) and Frame.io linking.

    Email + Frame.io config live here (not .env) so the admin manages them from the Settings page.
    Secrets (SMTP password, Frame.io client secret) are encrypted at rest with TokenCipher.
    `config_seeded` guards the one-time import of any pre-existing .env values on first startup.
    """

    __tablename__ = "app_settings"

    id: Mapped[uuid.UUID] = _uuid_pk()
    logo_png: Mapped[bytes | None] = mapped_column(LargeBinary)
    logo_content_type: Mapped[str | None] = mapped_column(String(64))
    logo_updated_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    brand_display_name: Mapped[str | None] = mapped_column(String(255))
    brand_accent_color: Mapped[str | None] = mapped_column(String(32))

    # Email (SMTP). smtp_password_encrypted holds a TokenCipher ciphertext.
    smtp_host: Mapped[str | None] = mapped_column(String(255))
    smtp_port: Mapped[int | None] = mapped_column(Integer)
    smtp_username: Mapped[str | None] = mapped_column(String(255))
    smtp_password_encrypted: Mapped[str | None] = mapped_column(Text)
    smtp_from: Mapped[str | None] = mapped_column(String(255))
    smtp_use_tls: Mapped[bool | None] = mapped_column(Boolean)
    smtp_starttls: Mapped[bool | None] = mapped_column(Boolean)
    notify_email: Mapped[str | None] = mapped_column(String(255))

    # Frame.io / Adobe IMS OAuth. frameio_client_secret_encrypted is a TokenCipher ciphertext.
    frameio_client_id: Mapped[str | None] = mapped_column(String(255))
    frameio_client_secret_encrypted: Mapped[str | None] = mapped_column(Text)

    # True once any legacy .env email/Frame.io values have been imported into this row.
    config_seeded: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)


class FrameioConnection(Base, TimestampMixin):
    __tablename__ = "frameio_connections"

    id: Mapped[uuid.UUID] = _uuid_pk()
    user_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True, nullable=False
    )
    adobe_ims_user_id: Mapped[str | None] = mapped_column(String(255))
    adobe_email: Mapped[str | None] = mapped_column(String(255))
    access_token_encrypted: Mapped[str | None] = mapped_column(Text)
    refresh_token_encrypted: Mapped[str | None] = mapped_column(Text)
    token_expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    # cached workspace/account list
    workspaces: Mapped[dict[str, Any] | None] = mapped_column(JSONB)

    user: Mapped[User] = relationship(back_populates="frameio_connections")


class Destination(Base, TimestampMixin):
    __tablename__ = "destinations"

    id: Mapped[uuid.UUID] = _uuid_pk()
    display_name: Mapped[str] = mapped_column(String(255), nullable=False)
    # {"type": "frameio", "account_id", "workspace_id", "project_id", "folder_id"}
    config: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False)
    # Subtitle line shown on the upload page. The logo and accent come from global branding.
    subtitle: Mapped[str | None] = mapped_column(Text)

    upload_links: Mapped[list["UploadLink"]] = relationship(
        back_populates="destination", cascade="all, delete-orphan"
    )


class UploadLink(Base, TimestampMixin):
    __tablename__ = "upload_links"

    id: Mapped[uuid.UUID] = _uuid_pk()
    token: Mapped[str] = mapped_column(String(64), unique=True, index=True, nullable=False)
    destination_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("destinations.id", ondelete="CASCADE"), index=True, nullable=False
    )
    expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    password_hash: Mapped[str | None] = mapped_column(String(255))
    max_file_size: Mapped[int | None] = mapped_column(BigInteger)
    allowed_extensions: Mapped[list[Any] | None] = mapped_column(JSONB)
    # {"name": bool, "email": bool, "message": bool}
    uploader_fields_required: Mapped[dict[str, Any] | None] = mapped_column(JSONB)
    # When true, the email field is required AND must be verified by a one-time code.
    verify_email: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    # Optional base subfolder under the destination (a Frame.io folder); null = root.
    target_folder_id: Mapped[str | None] = mapped_column(String(255))
    target_folder_name: Mapped[str | None] = mapped_column(String(255))
    # Optional path template applied beneath the base folder, e.g. "{date}/{uploader_name}".
    subfolder_template: Mapped[str | None] = mapped_column(Text)
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    # Per-link text overrides (fall back to the destination / global branding when null). The
    # logo and accent come from global branding.
    brand_display_name: Mapped[str | None] = mapped_column(String(255))
    brand_subtitle: Mapped[str | None] = mapped_column(Text)

    destination: Mapped[Destination] = relationship(back_populates="upload_links")
    sessions: Mapped[list["UploadSession"]] = relationship(
        back_populates="upload_link", cascade="all, delete-orphan"
    )


class UploadSession(Base, TimestampMixin):
    __tablename__ = "upload_sessions"

    id: Mapped[uuid.UUID] = _uuid_pk()
    upload_link_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("upload_links.id", ondelete="CASCADE"), index=True, nullable=False
    )
    uploader_name: Mapped[str | None] = mapped_column(String(255))
    uploader_email: Mapped[str | None] = mapped_column(String(255))
    uploader_message: Mapped[str | None] = mapped_column(Text)
    status: Mapped[str] = mapped_column(String(32), default="in_progress", nullable=False)
    frameio_asset_ids: Mapped[list[Any] | None] = mapped_column(JSONB)
    total_bytes: Mapped[int | None] = mapped_column(BigInteger)
    # Client-reported progress from the uploader page's heartbeat; total_bytes stays "final
    # total, written at completion".
    uploaded_bytes: Mapped[int | None] = mapped_column(BigInteger)
    started_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    # Last liveness signal (heartbeat / file registration). The worker sweep flags in_progress
    # sessions as abandoned on coalesce(last_activity_at, started_at, created_at).
    last_activity_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))

    upload_link: Mapped[UploadLink] = relationship(back_populates="sessions")


class SyncRule(Base, TimestampMixin):
    __tablename__ = "sync_rules"

    id: Mapped[uuid.UUID] = _uuid_pk()
    name: Mapped[str] = mapped_column(String(255), nullable=False)
    # {"type": "frameio", "account_id", "workspace_id", "project_id", "folder_id"}
    source_config: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False)
    destination_path: Mapped[str] = mapped_column(Text, nullable=False)
    conflict_policy: Mapped[str] = mapped_column(
        String(32), default="rename_suffix", nullable=False
    )  # skip | overwrite | rename_suffix
    path_template: Mapped[str | None] = mapped_column(Text)
    enabled: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    webhook_secret: Mapped[str | None] = mapped_column(Text)
    frameio_webhook_id: Mapped[str | None] = mapped_column(String(255))

    jobs: Mapped[list["SyncJob"]] = relationship(
        back_populates="sync_rule", cascade="all, delete-orphan"
    )


class SyncJob(Base, TimestampMixin):
    __tablename__ = "sync_jobs"

    id: Mapped[uuid.UUID] = _uuid_pk()
    sync_rule_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("sync_rules.id", ondelete="CASCADE"), index=True, nullable=False
    )
    frameio_file_id: Mapped[str] = mapped_column(String(255), index=True, nullable=False)
    status: Mapped[str] = mapped_column(String(32), default="pending", nullable=False)
    bytes: Mapped[int | None] = mapped_column(BigInteger)
    sha256: Mapped[str | None] = mapped_column(String(64))
    error: Mapped[str | None] = mapped_column(Text)
    retry_count: Mapped[int] = mapped_column(Integer, default=0, nullable=False)
    started_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))

    sync_rule: Mapped[SyncRule] = relationship(back_populates="jobs")


class WebhookEvent(Base):
    __tablename__ = "webhook_events"

    id: Mapped[uuid.UUID] = _uuid_pk()
    source: Mapped[str] = mapped_column(String(32), default="frameio", nullable=False)
    event_type: Mapped[str | None] = mapped_column(String(128))
    raw: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False)
    signature_valid: Mapped[bool | None] = mapped_column(Boolean)
    processed: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    processed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    error: Mapped[str | None] = mapped_column(Text)
    received_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )


class AuditLog(Base):
    __tablename__ = "audit_log"

    id: Mapped[uuid.UUID] = _uuid_pk()
    user_id: Mapped[uuid.UUID | None] = mapped_column(
        ForeignKey("users.id", ondelete="SET NULL"), index=True
    )
    action: Mapped[str] = mapped_column(String(128), nullable=False)
    detail: Mapped[dict[str, Any] | None] = mapped_column(JSONB)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )


class DownloadLink(Base, TimestampMixin):
    __tablename__ = "download_links"

    id: Mapped[uuid.UUID] = _uuid_pk()
    token: Mapped[str] = mapped_column(String(64), unique=True, index=True, nullable=False)
    # {"type":"file","account_id","file_id"} |
    # {"type":"folder","account_id","folder_id","recursive":bool} |
    # {"type":"selection","account_id","file_ids":[...]}
    source: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False)
    expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    password_hash: Mapped[str | None] = mapped_column(String(255))
    # Total downloads allowed across all files in the link (null = unlimited).
    max_downloads: Mapped[int | None] = mapped_column(Integer)
    downloads_count: Mapped[int] = mapped_column(Integer, default=0, nullable=False)
    # {"name": bool, "email": bool}
    viewer_fields_required: Mapped[dict[str, Any] | None] = mapped_column(JSONB)
    # When true, the email field is required AND must be verified by a one-time code.
    verify_email: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    allow_preview: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    # Per-link text (the logo and accent come from global branding).
    brand_display_name: Mapped[str | None] = mapped_column(String(255))
    brand_subtitle: Mapped[str | None] = mapped_column(Text)

    sessions: Mapped[list["DownloadSession"]] = relationship(
        back_populates="download_link", cascade="all, delete-orphan"
    )


class DownloadSession(Base, TimestampMixin):
    __tablename__ = "download_sessions"

    id: Mapped[uuid.UUID] = _uuid_pk()
    download_link_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("download_links.id", ondelete="CASCADE"), index=True, nullable=False
    )
    viewer_name: Mapped[str | None] = mapped_column(String(255))
    viewer_email: Mapped[str | None] = mapped_column(String(255))
    ip: Mapped[str | None] = mapped_column(String(64))
    user_agent: Mapped[str | None] = mapped_column(Text)
    # Snapshot of the link's resolved files at session start ([{id, name, size}, ...]) so minting a
    # per-file download URL needs no extra Frame.io calls (and folder sources aren't re-listed).
    files: Mapped[list[Any] | None] = mapped_column(JSONB)
    started_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))

    download_link: Mapped[DownloadLink] = relationship(back_populates="sessions")
    events: Mapped[list["DownloadEvent"]] = relationship(
        back_populates="download_session", cascade="all, delete-orphan"
    )


class DownloadEvent(Base):
    __tablename__ = "download_events"

    id: Mapped[uuid.UUID] = _uuid_pk()
    download_session_id: Mapped[uuid.UUID] = mapped_column(
        ForeignKey("download_sessions.id", ondelete="CASCADE"), index=True, nullable=False
    )
    frameio_file_id: Mapped[str] = mapped_column(String(255), index=True, nullable=False)
    file_name: Mapped[str | None] = mapped_column(String(512))
    completed: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    bytes_served: Mapped[int | None] = mapped_column(BigInteger)
    started_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )

    download_session: Mapped[DownloadSession] = relationship(back_populates="events")


class EmailVerification(Base):
    """A pending one-time email-verification code. One active row per email (upsert on resend);
    deleted on success. Backs the 2FA gate on links with verify_email."""

    __tablename__ = "email_verifications"

    id: Mapped[uuid.UUID] = _uuid_pk()
    email: Mapped[str] = mapped_column(String(255), unique=True, index=True, nullable=False)
    code_hash: Mapped[str] = mapped_column(String(64), nullable=False)  # sha256 hex of the code
    expires_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    attempts: Mapped[int] = mapped_column(Integer, default=0, nullable=False)
    last_sent_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
