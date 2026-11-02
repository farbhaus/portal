# Schema migrations

`portal-api migrate` runs when the container starts and brings the database to the schema this
image expects:

- `../schema.sql` — the full schema at the head revision, stamped with it. Applied to an empty
  database.
- `<to>.sql` here — one file per revision step, applied in order to a database at an older
  revision.
- `../baseline.sql` — the schema at the oldest revision the image can upgrade (`b8c9d0e1f2a3`).
  Not shipped: the schema check starts from it.

A database at a revision with no path to the head is refused at startup. The revision lives in the
`alembic_version` table, named after the tool the schema was first authored with; the steps here
are plain SQL.

## Adding a migration

1. Pick the next revision id: 12 hex characters, unused so far (the chain so far runs
   `b8c9d0e1f2a3` → … → the stamp at the end of `../schema.sql`).
2. Write `<to>.sql` here. It runs against every deployment, in one transaction, and moves the
   stamp itself:

   ```sql
   -- portal-migration: <from> -> <to>
   BEGIN;

   ALTER TABLE links ADD COLUMN example TEXT;

   UPDATE alembic_version SET version_num='<to>' WHERE alembic_version.version_num = '<from>';

   COMMIT;
   ```

3. Register it at the end of `MIGRATIONS` in `src/migrate.rs`:
   `Migration { from: "<from>", to: "<to>", sql: include_str!("../migrations/<to>.sql") },`
4. Regenerate the snapshot: `tools/schema_check.sh --write`, then review the diff of
   `../schema.sql` — it should change exactly as your migration says.

`tools/schema_check.sh` (run in CI, needs only Docker) loads `baseline.sql` into a throwaway
Postgres, applies every file here in order, and fails when the result differs from `schema.sql`,
when a file doesn't move `alembic_version` to its own revision, when the files don't form one
chain, or when a file isn't registered in `MIGRATIONS`.
