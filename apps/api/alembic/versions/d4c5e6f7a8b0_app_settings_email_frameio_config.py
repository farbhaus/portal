"""app_settings: email (SMTP) + Frame.io linking config

Moves email and Frame.io configuration out of .env and into the app_settings row so the admin
manages them from the Settings page. Secrets (SMTP password, Frame.io client secret) are stored
encrypted (TokenCipher ciphertext). config_seeded guards the one-time import of legacy .env values.

Revision ID: d4c5e6f7a8b0
Revises: c3b4d5e6f7a9
Create Date: 2026-06-20 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'd4c5e6f7a8b0'
down_revision: str | None = 'c3b4d5e6f7a9'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('app_settings', sa.Column('smtp_host', sa.String(length=255), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_port', sa.Integer(), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_username', sa.String(length=255), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_password_encrypted', sa.Text(), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_from', sa.String(length=255), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_use_tls', sa.Boolean(), nullable=True))
    op.add_column('app_settings', sa.Column('smtp_starttls', sa.Boolean(), nullable=True))
    op.add_column('app_settings', sa.Column('notify_email', sa.String(length=255), nullable=True))
    op.add_column('app_settings', sa.Column('frameio_client_id', sa.String(length=255), nullable=True))
    op.add_column(
        'app_settings', sa.Column('frameio_client_secret_encrypted', sa.Text(), nullable=True)
    )
    op.add_column(
        'app_settings',
        sa.Column(
            'config_seeded', sa.Boolean(), server_default=sa.text('false'), nullable=False
        ),
    )


def downgrade() -> None:
    op.drop_column('app_settings', 'config_seeded')
    op.drop_column('app_settings', 'frameio_client_secret_encrypted')
    op.drop_column('app_settings', 'frameio_client_id')
    op.drop_column('app_settings', 'notify_email')
    op.drop_column('app_settings', 'smtp_starttls')
    op.drop_column('app_settings', 'smtp_use_tls')
    op.drop_column('app_settings', 'smtp_from')
    op.drop_column('app_settings', 'smtp_password_encrypted')
    op.drop_column('app_settings', 'smtp_username')
    op.drop_column('app_settings', 'smtp_port')
    op.drop_column('app_settings', 'smtp_host')
