"""download event client address — per-download ip + user_agent

The admin download log showed the docker bridge gateway (172.18.0.1) for every recipient: the only
IP recorded was download_sessions.ip, taken from the raw socket peer, which behind the proxy chain
is always the nearest proxy. The address is now resolved from the trusted X-Forwarded-For hop and
captured on each download_events row, so the log answers "who downloaded what" per file rather than
once per session.

Revision ID: c9d0e1f2a3b4
Revises: b8c9d0e1f2a3
Create Date: 2026-08-08 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'c9d0e1f2a3b4'
down_revision: str | None = 'b8c9d0e1f2a3'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('download_events', sa.Column('ip', sa.String(length=64), nullable=True))
    op.add_column('download_events', sa.Column('user_agent', sa.Text(), nullable=True))


def downgrade() -> None:
    op.drop_column('download_events', 'user_agent')
    op.drop_column('download_events', 'ip')
