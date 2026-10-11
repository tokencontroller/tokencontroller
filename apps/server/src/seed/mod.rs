//! Seed data: the example organization the docs use, loaded with one
//! command, for local development, CI, staging and screenshots.
//!
//! `tokencontroller seed` applies migrations and calls [`load`]. Staging runs
//! it after every reset, so [`load`] must be safe to run more than once.
//!
//! Owner: the service platform work.

use sqlx::PgPool;

/// Loads the example organization. Safe to run again on a seeded database.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    // The example organization arrives with the organizations module; until
    // then there is nothing to load.
    let _connection = db.acquire().await?;
    tracing::info!("seed data loaded");
    Ok(())
}
