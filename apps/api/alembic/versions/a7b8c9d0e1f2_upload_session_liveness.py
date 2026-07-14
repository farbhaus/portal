"""upload session liveness — heartbeat timestamp + client-reported progress

Backs the abandoned-upload sweep (issue #41): the uploader page heartbeats last_activity_at /
uploaded_bytes, and a worker cron flags silent in_progress sessions as "abandoned" so they stop
showing on the dashboard as "Uploading now" forever.

Revision ID: a7b8c9d0e1f2
Revises: f6e7d8c9b0a1
Create Date: 2026-07-11 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'a7b8c9d0e1f2'
down_revision: str | None = 'f6e7d8c9b0a1'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('upload_sessions', sa.Column('uploaded_bytes', sa.BigInteger(), nullable=True))
    op.add_column(
        'upload_sessions', sa.Column('last_activity_at', sa.DateTime(timezone=True), nullable=True)
    )


def downgrade() -> None:
    op.drop_column('upload_sessions', 'last_activity_at')
    op.drop_column('upload_sessions', 'uploaded_bytes')
