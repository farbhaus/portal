-- portal-migration: e1f2a3b4c5d6 -> f2a3b4c5d6e7
BEGIN;

-- Portal no longer copies Frame.io files to local storage: FioTransfer syncs each link on the
-- user's own machine. Sync rules, their jobs (the copy-once ledger) and the Frame.io webhook
-- events that fed them go.
DROP TABLE sync_jobs;

DROP TABLE sync_rules;

DROP TABLE webhook_events;

UPDATE alembic_version SET version_num='f2a3b4c5d6e7' WHERE alembic_version.version_num = 'e1f2a3b4c5d6';

COMMIT;
