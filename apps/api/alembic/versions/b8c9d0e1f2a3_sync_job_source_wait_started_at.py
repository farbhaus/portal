"""sync_jobs.source_wait_started_at

Gives the source-readiness deadline its own clock. It used to run from created_at, which reconcile's
self-heal never resets, so a dead-lettered job was re-picked, found itself days past the window and
dead-lettered again on the same tick — forever.

Revision ID: b8c9d0e1f2a3
Revises: a7b8c9d0e1f2
Create Date: 2026-07-29 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'b8c9d0e1f2a3'
down_revision: str | None = 'a7b8c9d0e1f2'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        'sync_jobs', sa.Column('source_wait_started_at', sa.DateTime(timezone=True), nullable=True)
    )


def downgrade() -> None:
    op.drop_column('sync_jobs', 'source_wait_started_at')
