"""app_settings (global branding logo)

Revision ID: c1b2d3e4f5a7
Revises: b2e4c6a8d013
Create Date: 2026-06-20 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'c1b2d3e4f5a7'
down_revision: str | None = 'b2e4c6a8d013'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.create_table(
        'app_settings',
        sa.Column('id', sa.UUID(), server_default=sa.text('gen_random_uuid()'), nullable=False),
        sa.Column('logo_png', sa.LargeBinary(), nullable=True),
        sa.Column('logo_content_type', sa.String(length=64), nullable=True),
        sa.Column('logo_updated_at', sa.DateTime(timezone=True), nullable=True),
        sa.Column('brand_display_name', sa.String(length=255), nullable=True),
        sa.Column('brand_accent_color', sa.String(length=32), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.text('now()'), nullable=False),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.text('now()'), nullable=False),
        sa.PrimaryKeyConstraint('id'),
    )


def downgrade() -> None:
    op.drop_table('app_settings')
