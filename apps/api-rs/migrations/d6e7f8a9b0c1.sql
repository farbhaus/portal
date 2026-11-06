-- portal-migration: c5d6e7f8a9b0 -> d6e7f8a9b0c1
BEGIN;

-- The names FioTransfer's Imports and Exports folders get when a link doesn't name its own.
-- NULL is the default: 1_DRIVE and 0_EXPORTS.
ALTER TABLE app_settings ADD COLUMN fiotransfer_imports_folder character varying(255);
ALTER TABLE app_settings ADD COLUMN fiotransfer_exports_folder character varying(255);

-- Whether Portal keeps a Send link on an upload link's Exports folder. Deleting that Send link
-- turns it off, so it doesn't come back.
ALTER TABLE links ADD COLUMN send_link boolean DEFAULT true NOT NULL;

-- Links saved by Portal 2.2 and 2.3 stored `folder.sync` as `{}` / `download` / `upload` /
-- `name`, which only FioTransfer 2.x reads. Bring them back to the shape every FioTransfer reads:
-- a `turbosort` project folder, and `mode`, `drive` and `exports`. The names are the ones
-- FioTransfer 2.x already used for them, so its folders on disk stay where they are. Links in
-- that shape already (no `download`/`upload`/`name`, with a `mode`) are not touched.
CREATE TEMP TABLE sync_2_2 ON COMMIT DROP AS
    SELECT id FROM links
     WHERE kind = 'upload'
       AND jsonb_typeof(folder->'sync') = 'object'
       AND (NOT folder->'sync' ? 'mode' OR folder->'sync' ?| ARRAY['download', 'upload', 'name']);

UPDATE links SET folder = jsonb_set(folder, '{sync}',
        ((folder->'sync') - 'download') || jsonb_build_object('drive', folder->'sync'->'download'))
 WHERE id IN (SELECT id FROM sync_2_2)
   AND jsonb_typeof(folder->'sync'->'download') = 'object' AND NOT folder->'sync' ? 'drive';

UPDATE links SET folder = jsonb_set(folder, '{sync}',
        ((folder->'sync') - 'upload') || jsonb_build_object('exports', folder->'sync'->'upload'))
 WHERE id IN (SELECT id FROM sync_2_2)
   AND jsonb_typeof(folder->'sync'->'upload') = 'object' AND NOT folder->'sync' ? 'exports';

UPDATE links SET folder = jsonb_set(folder, '{sync}', (folder->'sync') - 'download' - 'upload')
 WHERE id IN (SELECT id FROM sync_2_2);

-- 2.x named the project folder after `sync.name` first, then `turbosort`.
UPDATE links SET folder = jsonb_set(folder, '{turbosort}', to_jsonb(btrim(folder->'sync'->>'name')))
 WHERE id IN (SELECT id FROM sync_2_2)
   AND jsonb_typeof(folder->'sync'->'name') = 'string' AND btrim(folder->'sync'->>'name') <> '';

UPDATE links SET folder = folder #- '{sync,name}' WHERE id IN (SELECT id FROM sync_2_2);

-- 2.x downloaded every synced link into its IN folder, and an upload lane without a folder did
-- nothing.
UPDATE links SET folder = jsonb_set(folder, '{sync,drive}', '{}'::jsonb)
 WHERE id IN (SELECT id FROM sync_2_2) AND jsonb_typeof(folder->'sync'->'drive') IS DISTINCT FROM 'object';

UPDATE links SET folder = folder #- '{sync,exports}'
 WHERE id IN (SELECT id FROM sync_2_2)
   AND folder->'sync' ? 'exports'
   AND COALESCE(folder->'sync'->'exports'->>'folder_id', '') = '';

-- A lane without a name was FioTransfer 2.x's IN or OUT.
UPDATE links SET folder = jsonb_set(folder, '{sync,drive,local}', '"IN"')
 WHERE id IN (SELECT id FROM sync_2_2) AND COALESCE(btrim(folder->'sync'->'drive'->>'local'), '') = '';

UPDATE links SET folder = jsonb_set(folder, '{sync,exports,local}', '"OUT"')
 WHERE id IN (SELECT id FROM sync_2_2)
   AND jsonb_typeof(folder->'sync'->'exports') = 'object'
   AND COALESCE(btrim(folder->'sync'->'exports'->>'local'), '') = '';

UPDATE links SET folder = jsonb_set(folder, '{sync,mode}', '"add_only"') WHERE id IN (SELECT id FROM sync_2_2);

-- No project folder: 2.x used the link's name (as the API shows it: its own, else the job
-- folder, else the folder, else the page title) made into one folder name.
UPDATE links SET folder = jsonb_set(folder, '{turbosort}', to_jsonb(left(btrim(regexp_replace(
        COALESCE(
            NULLIF(btrim(name), ''),
            NULLIF(btrim(regexp_replace(COALESCE(folder->'path'->1->>'name', folder->'path'->0->>'name', ''), ' \(root\)$', '')), ''),
            NULLIF(btrim(folder->>'folder_name'), ''),
            NULLIF(btrim(brand_display_name), ''),
            'Untitled'),
        '[/\\%[:cntrl:]]', '-', 'g')), 255)))
 WHERE id IN (SELECT id FROM sync_2_2) AND COALESCE(btrim(folder->>'turbosort'), '') = '';

UPDATE alembic_version SET version_num='d6e7f8a9b0c1' WHERE alembic_version.version_num = 'c5d6e7f8a9b0';

COMMIT;
