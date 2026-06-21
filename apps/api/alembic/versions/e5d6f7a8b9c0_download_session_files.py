"""download session resolved files snapshot

Revision ID: e5d6f7a8b9c0
Revises: d4c5e6f7a8b0
Create Date: 2026-06-21 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects.postgresql import JSONB


# revision identifiers, used by Alembic.
revision: str = 'e5d6f7a8b9c0'
down_revision: str | None = 'd4c5e6f7a8b0'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('download_sessions', sa.Column('files', JSONB(), nullable=True))


def downgrade() -> None:
    op.drop_column('download_sessions', 'files')
