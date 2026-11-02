//! Schema setup + forward migrations — `portal-api migrate`, run by the container entrypoint
//! before the API and worker start.
//!
//! The image ships:
//! - `schema.sql`: the full schema at the head revision, stamped with it — applied to an empty
//!   database;
//! - `migrations/<to>.sql`: one hand-written SQL file per revision step — applied in order to a
//!   database at an older revision.
//!
//! The revision lives in the `alembic_version` table, named after the tool the schema was first
//! authored with. A database at a revision with no path to the head (older than the oldest
//! migration here, or newer than this image) is refused rather than run against a schema the
//! code doesn't match. `tools/schema_check.sh` checks that `baseline.sql` plus the migrations
//! reproduces `schema.sql`. See migrations/README.md.

use sqlx::{Connection, PgConnection, Row};

/// The head schema, stamped with its Alembic revision.
const SCHEMA: &str = include_str!("../schema.sql");

/// One revision step: applying `sql` to a database at `from` brings it to `to`.
pub struct Migration {
    pub from: &'static str,
    pub to: &'static str,
    pub sql: &'static str,
}

/// Every step after the oldest revision this image can upgrade, in order. To add one, see
/// migrations/README.md — e.g.
/// `Migration { from: "c9d0e1f2a3b4", to: "d0e1f2a3b4c5", sql: include_str!("../migrations/d0e1f2a3b4c5.sql") },`
pub const MIGRATIONS: &[Migration] = &[
    Migration { from: "b8c9d0e1f2a3", to: "c9d0e1f2a3b4", sql: include_str!("../migrations/c9d0e1f2a3b4.sql") },
    Migration { from: "c9d0e1f2a3b4", to: "d0e1f2a3b4c5", sql: include_str!("../migrations/d0e1f2a3b4c5.sql") },
    Migration { from: "d0e1f2a3b4c5", to: "e1f2a3b4c5d6", sql: include_str!("../migrations/e1f2a3b4c5d6.sql") },
    Migration { from: "e1f2a3b4c5d6", to: "f2a3b4c5d6e7", sql: include_str!("../migrations/f2a3b4c5d6e7.sql") },
    Migration { from: "f2a3b4c5d6e7", to: "a3b4c5d6e7f8", sql: include_str!("../migrations/a3b4c5d6e7f8.sql") },
    Migration { from: "a3b4c5d6e7f8", to: "b4c5d6e7f8a9", sql: include_str!("../migrations/b4c5d6e7f8a9.sql") },
];

/// The revision `schema.sql` is stamped with.
pub fn head_revision() -> &'static str {
    stamped_revision(SCHEMA).expect("schema.sql must end with its alembic_version stamp")
}

fn stamped_revision(schema: &str) -> Option<&str> {
    schema.lines().rev().find_map(|line| {
        line.strip_prefix("INSERT INTO public.alembic_version (version_num) VALUES ('")?
            .strip_suffix("');")
    })
}

/// The steps that take a database at `current` to `head`, or None if there is no such path.
pub fn plan<'a>(migrations: &'a [Migration], current: &str, head: &str) -> Option<Vec<&'a Migration>> {
    let mut steps = Vec::new();
    let mut at = current;
    while at != head {
        let step = migrations.iter().find(|m| m.from == at)?;
        if steps.len() > migrations.len() {
            return None; // a cycle in the list
        }
        steps.push(step);
        at = step.to;
    }
    Some(steps)
}

/// pg_dump output carries psql meta-commands (`\restrict` lines) that only psql understands.
fn strip_psql_meta(sql: &str) -> String {
    sql.lines().filter(|l| !l.starts_with('\\')).collect::<Vec<_>>().join("\n")
}

/// Bring the database to the head schema (or fail with an operator-facing message), then
/// re-encrypt any secret an earlier Rust build stored under its mismatched key.
pub async fn migrate(settings: &crate::config::Settings) -> anyhow::Result<()> {
    // A dedicated connection: pg_dump's preamble resets search_path for the session, which must
    // not leak into the pool the API uses.
    let mut conn = PgConnection::connect(&settings.database_url).await?;
    if schema(&mut conn).await? == Schema::Created {
        return Ok(()); // a fresh database holds no secrets to re-encrypt
    }
    let rekeyed = crate::crypto::rekey_legacy_values(&mut conn, &settings.token_encryption_key).await?;
    if rekeyed > 0 {
        tracing::info!("migrate: re-encrypted {rekeyed} secret(s) written under the old Rust key derivation");
    }
    Ok(())
}

#[derive(PartialEq)]
enum Schema {
    /// Applied `schema.sql` to an empty database.
    Created,
    /// The database already had a schema (now at the head revision).
    Existing,
}

async fn schema(conn: &mut PgConnection) -> anyhow::Result<Schema> {
    let head = head_revision();

    let has_schema: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(&mut *conn)
        .await?;
    if !has_schema {
        tracing::info!("migrate: empty database, applying schema at {head}");
        // AssertSqlSafe: the shipped schema file, not user input.
        sqlx::raw_sql(sqlx::AssertSqlSafe(strip_psql_meta(SCHEMA))).execute(&mut *conn).await?;
        // pg_dump's preamble emptied search_path for this session; put it back for what follows.
        sqlx::query("RESET search_path").execute(&mut *conn).await?;
        return Ok(Schema::Created);
    }

    let current: Option<String> = sqlx::query("SELECT version_num FROM alembic_version")
        .fetch_optional(&mut *conn)
        .await?
        .map(|r| r.get("version_num"));
    let Some(current) = current else {
        // Created by an earlier Rust image, before schema.sql carried its revision: that image
        // only ever applied the same snapshot, so this is the head schema.
        tracing::info!("migrate: stamping schema revision {head}");
        sqlx::query("INSERT INTO alembic_version (version_num) VALUES ($1)")
            .bind(head)
            .execute(&mut *conn)
            .await?;
        return Ok(Schema::Existing);
    };
    if current == head {
        tracing::info!("migrate: schema is at {head}");
        return Ok(Schema::Existing);
    }
    let Some(steps) = plan(MIGRATIONS, &current, head) else {
        anyhow::bail!(
            "the database is at schema revision {current}, but this image expects {head} and has no \
             migration from {current}. If it is older, upgrade with an earlier Portal release first; \
             if it is newer, use a Portal image at least as new as the one that last ran on it."
        );
    };
    for step in steps {
        tracing::info!("migrate: {} -> {}", step.from, step.to);
        // Each file wraps itself in BEGIN/COMMIT and moves alembic_version (migrations/README.md);
        // this update is then a no-op, kept for files that don't.
        sqlx::raw_sql(sqlx::AssertSqlSafe(strip_psql_meta(step.sql))).execute(&mut *conn).await?;
        sqlx::query("UPDATE alembic_version SET version_num = $1 WHERE version_num = $2")
            .bind(step.to)
            .bind(step.from)
            .execute(&mut *conn)
            .await?;
    }
    tracing::info!("migrate: schema is at {head}");
    Ok(Schema::Existing)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: &[Migration] = &[
        Migration { from: "a", to: "b", sql: "" },
        Migration { from: "b", to: "c", sql: "" },
    ];

    #[test]
    fn plans_the_chain_to_head() {
        let path: Vec<&str> = plan(STEPS, "a", "c").unwrap().iter().map(|m| m.to).collect();
        assert_eq!(path, ["b", "c"]);
        assert!(plan(STEPS, "c", "c").unwrap().is_empty());
        assert!(plan(STEPS, "z", "c").is_none()); // older than any migration here
        assert!(plan(STEPS, "c", "a").is_none()); // newer than this image
    }

    #[test]
    fn shipped_schema_and_migrations_agree() {
        let head = head_revision();
        assert_eq!(head.len(), 12, "{head}");
        // The chain must end at the revision schema.sql is stamped with.
        if let Some(first) = MIGRATIONS.first() {
            assert!(plan(MIGRATIONS, first.from, head).is_some(), "migrations don't lead to {head}");
        }
        assert!(!strip_psql_meta(SCHEMA).lines().any(|l| l.starts_with('\\')));
    }
}
