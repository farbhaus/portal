"""webauthn credentials (passkeys)

Revision ID: c3b4d5e6f7a9
Revises: c2b3d4e5f6a8
Create Date: 2026-06-20 00:02:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql


# revision identifiers, used by Alembic.
revision: str = 'c3b4d5e6f7a9'
down_revision: str | None = 'c2b3d4e5f6a8'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.create_table(
        'webauthn_credentials',
        sa.Column('id', sa.UUID(), server_default=sa.text('gen_random_uuid()'), nullable=False),
        sa.Column('user_id', sa.UUID(), nullable=False),
        sa.Column('credential_id', sa.String(length=512), nullable=False),
        sa.Column('public_key', sa.Text(), nullable=False),
        sa.Column('sign_count', sa.BigInteger(), nullable=False),
        sa.Column('transports', postgresql.JSONB(), nullable=True),
        sa.Column('name', sa.String(length=255), nullable=True),
        sa.Column('last_used_at', sa.DateTime(timezone=True), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.text('now()'), nullable=False),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.text('now()'), nullable=False),
        sa.ForeignKeyConstraint(['user_id'], ['users.id'], ondelete='CASCADE'),
        sa.PrimaryKeyConstraint('id'),
    )
    op.create_index(
        op.f('ix_webauthn_credentials_user_id'), 'webauthn_credentials', ['user_id'], unique=False
    )
    op.create_index(
        op.f('ix_webauthn_credentials_credential_id'),
        'webauthn_credentials',
        ['credential_id'],
        unique=True,
    )


def downgrade() -> None:
    op.drop_index(op.f('ix_webauthn_credentials_credential_id'), table_name='webauthn_credentials')
    op.drop_index(op.f('ix_webauthn_credentials_user_id'), table_name='webauthn_credentials')
    op.drop_table('webauthn_credentials')
