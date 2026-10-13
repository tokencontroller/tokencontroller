//! Seed data for the close and reports: last month's bills, two token
//! budgets and the closed period for the example organization, so a fresh
//! instance shows one finished month. Safe to run twice.

use std::collections::BTreeMap;

use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money};

use super::period::{Period, PeriodDates};
use crate::bills::{BillSource, CostSource, NewBill, record_bill};
use crate::budgets::token_budget::{Applies, Draft, insert};
use crate::error::ApiError;
use crate::reports::filter::Filter;
use crate::seed::ids;

/// Loads this area's part of the example organization: a bill per provider
/// for last month that the token entries almost reach (the rest is the
/// residual), a token budget for a ticket and one for a team, then the close
/// of last month.
///
/// # Errors
///
/// Fails if a query fails or a bill does not add up.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let last_month = Period::of_date(now.date())
        .previous()
        .map_err(|_| anyhow::anyhow!("no month before {now}"))?;
    let usd = Currency::new("USD")?;
    bills(db, last_month, usd).await?;
    token_budgets(db, last_month, usd).await?;
    match super::close(db, ids::ORGANIZATION, last_month, ids::NADIA, now).await {
        Ok(()) => tracing::info!(period = %last_month, "seed data loaded: closed period"),
        Err(ApiError::Conflict(reason)) => {
            tracing::info!(period = %last_month, reason, "seed left the period as it is");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// One bill per provider: what the token entries report plus 1.5 percent,
/// so the reconciliation shows a small residual.
async fn bills(db: &PgPool, period: Period, usd: Currency) -> anyhow::Result<()> {
    let from = period.starts_at();
    let to = period
        .ends_at()
        .map_err(|_| anyhow::anyhow!("period out of range"))?;
    let reported = sqlx::query!(
        r#"SELECT provider, COALESCE(SUM(reported_cost_nanos), 0)::BIGINT AS "nanos!"
           FROM token_entries
           WHERE organization_id = $1 AND started_at >= $2 AND started_at < $3
           GROUP BY provider ORDER BY provider"#,
        ids::ORGANIZATION,
        from,
        to
    )
    .fetch_all(db)
    .await?;
    for row in reported {
        let provider = row.provider;
        let cents = row.nanos.checked_div(10_000_000).unwrap_or_default();
        let residual = cents
            .saturating_mul(15)
            .checked_div(1000)
            .unwrap_or_default();
        let net = Money::from_minor_units(cents.saturating_add(residual), usd)?;
        let zero = Money::from_minor_units(0, usd)?;
        let bill = NewBill {
            period,
            provider: provider.clone(),
            cost_source: CostSource::UsageAtApiRates,
            gross: net,
            discount: zero,
            credit: zero,
            net,
            exchange_rate: None,
            source: BillSource::Pulled,
            lines: Vec::new(),
            recorded_by: None,
            is_final: true,
        };
        if let Err(error) = record_bill(db, ids::ORGANIZATION, bill).await? {
            anyhow::bail!("seed bill from {provider} refused: {error}");
        }
    }
    tracing::info!(period = %period, "seed data loaded: bills");
    Ok(())
}

/// A token budget for the checkout ticket and a monthly one for team web.
async fn token_budgets(db: &PgPool, period: Period, usd: Currency) -> anyhow::Result<()> {
    let existing = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM token_budgets WHERE organization_id = $1"#,
        ids::ORGANIZATION
    )
    .fetch_one(db)
    .await?;
    if existing > 0 {
        return Ok(());
    }
    let ticket = Draft {
        name: Some("Checkout redirect loop".to_owned()),
        filter: Filter(BTreeMap::from([("ticket".to_owned(), "KD-214".to_owned())])),
        applies: Applies::new(None, false).map_err(anyhow::Error::msg)?,
        amount: Money::parse("120.00", usd)?,
    };
    insert(db, ids::ORGANIZATION, &ticket, None, ids::PRIYA).await?;
    let team = Draft {
        name: Some("Team web, monthly".to_owned()),
        filter: Filter(BTreeMap::from([("team".to_owned(), "web".to_owned())])),
        applies: Applies::new(Some(period), true).map_err(anyhow::Error::msg)?,
        amount: Money::parse("450.00", usd)?,
    };
    insert(db, ids::ORGANIZATION, &team, None, ids::PRIYA).await?;
    tracing::info!("seed data loaded: token budgets");
    Ok(())
}
