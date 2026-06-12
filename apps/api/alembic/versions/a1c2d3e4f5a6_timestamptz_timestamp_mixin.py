"""make TimestampMixin created_at/updated_at timezone-aware (timestamptz)

Revision ID: a1c2d3e4f5a6
Revises: f590a1d0c76b
Create Date: 2026-06-12 08:00:00.000000

These columns were created as TIMESTAMP WITHOUT TIME ZONE, so they read back tz-naive and broke
arithmetic against aware datetimes. Existing values were written by ``now()`` as UTC wall-clock,
so reinterpret them ``AT TIME ZONE 'UTC'`` when widening to timestamptz.
"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


revision: str = 'a1c2d3e4f5a6'
down_revision: str | None = 'f590a1d0c76b'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


# Every table mixing in TimestampMixin.
_TABLES = (
    "users",
    "frameio_connections",
    "destinations",
    "upload_links",
    "upload_sessions",
    "sync_rules",
    "sync_jobs",
    "download_links",
    "download_sessions",
)
_COLUMNS = ("created_at", "updated_at")


def upgrade() -> None:
    for table in _TABLES:
        for col in _COLUMNS:
            op.alter_column(
                table,
                col,
                type_=sa.DateTime(timezone=True),
                postgresql_using=f"{col} AT TIME ZONE 'UTC'",
                existing_nullable=False,
                existing_server_default=sa.text("now()"),
            )


def downgrade() -> None:
    for table in _TABLES:
        for col in _COLUMNS:
            op.alter_column(
                table,
                col,
                type_=sa.DateTime(timezone=False),
                postgresql_using=f"{col} AT TIME ZONE 'UTC'",
                existing_nullable=False,
                existing_server_default=sa.text("now()"),
            )
