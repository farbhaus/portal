"""user TOTP 2FA columns

Revision ID: c2b3d4e5f6a8
Revises: c1b2d3e4f5a7
Create Date: 2026-06-20 00:01:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql


# revision identifiers, used by Alembic.
revision: str = 'c2b3d4e5f6a8'
down_revision: str | None = 'c1b2d3e4f5a7'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('users', sa.Column('totp_secret_encrypted', sa.Text(), nullable=True))
    op.add_column(
        'users',
        sa.Column('totp_enabled', sa.Boolean(), nullable=False, server_default=sa.false()),
    )
    op.add_column('users', sa.Column('totp_recovery_codes', postgresql.JSONB(), nullable=True))


def downgrade() -> None:
    op.drop_column('users', 'totp_recovery_codes')
    op.drop_column('users', 'totp_enabled')
    op.drop_column('users', 'totp_secret_encrypted')
