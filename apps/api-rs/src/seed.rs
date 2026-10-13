
//! First-boot admin seed — the port of `portal.seed.seed_admin`. Idempotent: does nothing if any
//! user exists. Run from the entrypoint (`portal-api seed`) before the API starts, so the
//! supervised workers never race an empty users table.

use sqlx::PgPool;

pub async fn seed_admin(pool: &PgPool) -> anyhow::Result<()> {
    let email = std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());
    let password = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "change-me".to_string());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        tracing::info!("seed: {count} user(s) already present, skipping");
        return Ok(());
    }
    let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)?;
    // is_active has no DB default (SQLAlchemy sets it at the model level); set it here.
    sqlx::query("INSERT INTO users (email, password_hash, is_active) VALUES ($1, $2, true)")
        .bind(&email)
        .bind(&hash)
        .execute(pool)
        .await?;
    tracing::info!("seed: created admin {email}");
    Ok(())
}
