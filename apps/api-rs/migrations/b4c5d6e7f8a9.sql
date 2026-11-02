-- portal-migration: a3b4c5d6e7f8 -> b4c5d6e7f8a9
BEGIN;

-- The Send link Portal makes for an upload link's return folder points back at that upload link,
-- and goes with it when the upload link is deleted. One per upload link.
ALTER TABLE links ADD COLUMN return_for uuid;

ALTER TABLE links ADD CONSTRAINT links_return_for_fkey FOREIGN KEY (return_for) REFERENCES links(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX ix_links_return_for ON links (return_for);

UPDATE alembic_version SET version_num='b4c5d6e7f8a9' WHERE alembic_version.version_num = 'a3b4c5d6e7f8';

COMMIT;
