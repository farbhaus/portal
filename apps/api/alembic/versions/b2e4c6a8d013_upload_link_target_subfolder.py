"""upload link target subfolder + template

Revision ID: b2e4c6a8d013
Revises: a1c2d3e4f5a6
Create Date: 2026-06-19 00:00:00.000000

"""
from collections.abc import Sequence

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'b2e4c6a8d013'
down_revision: str | None = 'a1c2d3e4f5a6'
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column('upload_links', sa.Column('target_folder_id', sa.String(length=255), nullable=True))
    op.add_column('upload_links', sa.Column('target_folder_name', sa.String(length=255), nullable=True))
    op.add_column('upload_links', sa.Column('subfolder_template', sa.Text(), nullable=True))


def downgrade() -> None:
    op.drop_column('upload_links', 'subfolder_template')
    op.drop_column('upload_links', 'target_folder_name')
    op.drop_column('upload_links', 'target_folder_id')
