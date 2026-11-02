-- portal-migration: f2a3b4c5d6e7 -> a3b4c5d6e7f8
BEGIN;

-- Brand profiles ("Other brands") go: every link shows the app-wide brand.
ALTER TABLE links DROP COLUMN brand_profile_id;

DROP TABLE brand_profiles;

UPDATE alembic_version SET version_num='a3b4c5d6e7f8' WHERE alembic_version.version_num = 'f2a3b4c5d6e7';

COMMIT;
