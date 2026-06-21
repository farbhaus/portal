"""drop dead per-entity logo/accent branding columns

The logo and accent come from global branding (app_settings) since the P10 redesign; the
per-destination / per-link logo and accent overrides were no longer set by any UI and were
ignored by the public-page resolution. Drop them. The per-link display_name and subtitle remain.

Revision ID: f6e7d8c9b0a1
Revises: e5d6f7a8b9c0
Create Date: 2026-06-21 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'f6e7d8c9b0a1'
down_revision: str | None = 'e5d6f7a8b9c0'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.drop_column('destinations', 'logo_url')
    op.drop_column('destinations', 'accent_color')
    op.drop_column('upload_links', 'brand_logo_url')
    op.drop_column('upload_links', 'brand_accent_color')
    op.drop_column('download_links', 'brand_logo_url')
    op.drop_column('download_links', 'brand_accent_color')


def downgrade() -> None:
    op.add_column('download_links', sa.Column('brand_accent_color', sa.String(length=32), nullable=True))
    op.add_column('download_links', sa.Column('brand_logo_url', sa.Text(), nullable=True))
    op.add_column('upload_links', sa.Column('brand_accent_color', sa.String(length=32), nullable=True))
    op.add_column('upload_links', sa.Column('brand_logo_url', sa.Text(), nullable=True))
    op.add_column('destinations', sa.Column('accent_color', sa.String(length=32), nullable=True))
    op.add_column('destinations', sa.Column('logo_url', sa.Text(), nullable=True))
