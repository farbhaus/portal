-- portal-migration: b4c5d6e7f8a9 -> c5d6e7f8a9b0
BEGIN;

-- The project an uploader names when the link asks for one; `{project}` in the link's
-- subfolder template renders it.
ALTER TABLE upload_sessions ADD COLUMN uploader_project character varying(255);

UPDATE alembic_version SET version_num='c5d6e7f8a9b0' WHERE alembic_version.version_num = 'b4c5d6e7f8a9';

COMMIT;
