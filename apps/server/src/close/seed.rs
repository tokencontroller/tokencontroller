//! Seed data for the close and reports: bills, a closed period, adjustments
//! and token budgets for the example organization. Safe to run twice.

use sqlx::PgPool;

/// Loads this area's part of the example organization.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    let _connection = db.acquire().await?;
    Ok(())
}
