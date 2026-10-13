
-- Deterministic fixtures for the byte-for-byte differential. Fixed ids and timestamps so
-- responses that embed them are reproducible. Applied identically to both services.
BEGIN;

-- Global branding (single row).
DELETE FROM app_settings;
INSERT INTO app_settings (id, brand_display_name, brand_accent_color, config_seeded, created_at, updated_at)
VALUES ('00000000-0000-4000-8000-000000000001', 'Acme Post', '#ff8800', true,
        '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');

-- One destination with a cached Frame.io breadcrumb under config.path.
DELETE FROM audit_log;
DELETE FROM sync_jobs;
DELETE FROM sync_rules;
DELETE FROM download_events;
DELETE FROM download_sessions;
DELETE FROM upload_sessions;
DELETE FROM download_links;
DELETE FROM upload_links;
DELETE FROM destinations;
DELETE FROM frameio_connections;
INSERT INTO destinations (id, display_name, config, subtitle, created_at, updated_at)
VALUES ('00000000-0000-4000-8000-000000000010', 'Acme Deliveries',
        '{"type":"frameio","account_id":"acct-fixed","workspace_id":"ws-fixed","project_id":"proj-fixed","folder_id":"fold-fixed","path":[{"id":"proj-fixed","name":"Acme Project"},{"id":"fold-fixed","name":"Incoming"}]}'::jsonb,
        'Send your rushes here',
        '2026-01-02T09:00:00+00:00', '2026-01-02T09:00:00+00:00');

-- Two upload links, one of each state, with fixed timestamps and no password.
INSERT INTO upload_links (id, token, destination_id, expires_at, password_hash, max_file_size,
                          allowed_extensions, uploader_fields_required, revoked_at,
                          brand_display_name, brand_subtitle, verify_email, target_folder_id,
                          target_folder_name, subfolder_template, created_at, updated_at)
VALUES
 ('00000000-0000-4000-8000-000000000100', 'up-fixed-token-aaaaaaaaaaaaaaaa', '00000000-0000-4000-8000-000000000010',
  NULL, NULL, 4294967296, '["mov","mxf","wav"]'::jsonb, '{"name":true,"email":false,"message":false}'::jsonb,
  NULL, 'Acme Rushes', 'Drop your camera files', false, NULL, NULL, NULL,
  '2026-02-01T10:00:00+00:00', '2026-02-01T10:00:00+00:00'),
 ('00000000-0000-4000-8000-000000000101', 'up-fixed-token-bbbbbbbbbbbbbbbb', '00000000-0000-4000-8000-000000000010',
  '2026-01-15T00:00:00+00:00', NULL, NULL, NULL, '{"name":false,"email":true,"message":false}'::jsonb,
  NULL, NULL, NULL, false, NULL, NULL, NULL,
  '2026-01-05T08:30:00+00:00', '2026-01-05T08:30:00+00:00');
-- One download link (single-file source) so the download list has a deterministic row.
INSERT INTO download_links (id, token, source, expires_at, password_hash, max_downloads,
                            downloads_count, viewer_fields_required, revoked_at, allow_preview,
                            brand_display_name, brand_subtitle, verify_email, created_at, updated_at)
VALUES ('00000000-0000-4000-8000-000000000200', 'dl-fixed-token-cccccccccccccccc',
        '{"type":"file","account_id":"acct-fixed","file_id":"file-fixed-1","name":"A001_C001.mxf"}'::jsonb,
        NULL, NULL, 5, 2, '{"name":true,"email":true}'::jsonb, NULL, true,
        'Acme Review', 'Watch the cut', false,
        '2026-02-03T11:00:00+00:00', '2026-02-03T11:00:00+00:00');

-- One completed upload session (feeds the dashboard transfer list).
INSERT INTO upload_sessions (id, upload_link_id, uploader_name, uploader_email, uploader_message,
                             status, frameio_asset_ids, total_bytes, started_at, completed_at,
                             created_at, updated_at, uploaded_bytes, last_activity_at)
VALUES ('00000000-0000-4000-8000-000000000300', '00000000-0000-4000-8000-000000000100',
        'Dana Camera', 'dana@example.com', 'Act 1 rushes', 'completed',
        '[{"file_id":"asset-1","name":"A001_C001.mxf","size":1073741824}]'::jsonb, 1073741824,
        '2026-02-04T09:00:00+00:00', '2026-02-04T09:12:00+00:00',
        '2026-02-04T09:00:00+00:00', '2026-02-04T09:12:00+00:00', 1073741824,
        '2026-02-04T09:12:00+00:00');

-- One download session + one completed download event.
INSERT INTO download_sessions (id, download_link_id, viewer_name, viewer_email, ip, user_agent,
                               started_at, created_at, updated_at, files)
VALUES ('00000000-0000-4000-8000-000000000400', '00000000-0000-4000-8000-000000000200',
        'Reviewer One', 'review@example.com', '203.0.113.7', 'Mozilla/5.0',
        '2026-02-05T10:00:00+00:00', '2026-02-05T10:00:00+00:00',
        '2026-02-05T10:05:00+00:00', NULL);

INSERT INTO download_events (id, download_session_id, frameio_file_id, file_name, completed,
                             bytes_served, started_at, ip, user_agent)
VALUES ('00000000-0000-4000-8000-000000000401', '00000000-0000-4000-8000-000000000400',
        'file-fixed-1', 'A001_C001.mxf', true, 1073741824, '2026-02-05T10:00:00+00:00',
        '203.0.113.7', 'Mozilla/5.0');

-- One sync rule + one completed sync job.
INSERT INTO sync_rules (id, name, source_config, destination_path, conflict_policy, path_template,
                        enabled, webhook_secret, frameio_webhook_id, created_at, updated_at)
VALUES ('00000000-0000-4000-8000-000000000500', 'Rushes to NAS',
        '{"type":"frameio","account_id":"acct-fixed","folder_id":"fold-fixed"}'::jsonb,
        '/mnt/nas/rushes', 'overwrite', NULL, true, NULL, NULL,
        '2026-01-20T00:00:00+00:00', '2026-01-20T00:00:00+00:00');

INSERT INTO sync_jobs (id, sync_rule_id, frameio_file_id, status, bytes, sha256, error,
                       retry_count, started_at, completed_at, created_at, updated_at,
                       source_wait_started_at)
VALUES ('00000000-0000-4000-8000-000000000501', '00000000-0000-4000-8000-000000000500',
        'asset-1', 'done', 1073741824, NULL, NULL, 0,
        '2026-02-06T08:00:00+00:00', '2026-02-06T08:20:00+00:00',
        '2026-02-06T08:00:00+00:00', '2026-02-06T08:20:00+00:00', NULL);
-- Audit log rows for the activity view (one with a user, one without).
INSERT INTO audit_log (id, user_id, action, detail, created_at)
VALUES ('00000000-0000-4000-8000-000000000600',
        (SELECT id FROM users ORDER BY created_at LIMIT 1),
        'upload_link.created', '{"upload_link_id":"00000000-0000-4000-8000-000000000100"}'::jsonb,
        '2026-02-01T10:00:00+00:00'),
       ('00000000-0000-4000-8000-000000000601', NULL, 'webhook.received', NULL,
        '2026-02-07T12:00:00+00:00');
COMMIT;
