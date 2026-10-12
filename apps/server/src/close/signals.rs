//! The close's signals on the status page: how much of this month's bills
//! token entries explain, across every organization.

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::Money;
use utoipa::ToSchema;

use super::account;
use super::period::{Period, PeriodDates};
use crate::bills::reconciliation::percent;
use crate::reports::filter::Filter;
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;

/// Attribution and residual of the open period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OpenPeriodSignals {
    #[schema(value_type = String, example = "2026-10")]
    pub period: Period,
    /// Organizations with a bill for the period so far.
    pub organizations_with_bills: usize,
    /// The share of their bills that token entries explain, in percent: the
    /// average over those organizations.
    pub attribution_percent: Option<u8>,
    /// The rest: residual, in percent of the bills.
    pub residual_percent: Option<u8>,
}

/// Computes the signals for the calendar month running now.
///
/// # Errors
///
/// A database error. An organization whose figures cannot be converted is
/// left out.
pub async fn open_period_signals(db: &PgPool) -> Result<OpenPeriodSignals, sqlx::Error> {
    let period = Period::containing(OffsetDateTime::now_utc());
    let organizations = sqlx::query_scalar!(
        "SELECT DISTINCT organization_id FROM bills WHERE period = $1",
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    let mut shares: Vec<u8> = Vec::new();
    for organization_id in &organizations {
        let loaded = account::load(
            db,
            *organization_id,
            period,
            StatusView::All,
            &Scope::everything(),
            &Filter::default(),
        )
        .await;
        let account = match loaded {
            Ok(account) => account,
            Err(account::AccountError::Database(error)) => return Err(error),
            Err(error) => {
                tracing::warn!(%organization_id, %error, "status: organization left out");
                continue;
            }
        };
        let Ok(sheet) = account.cost_sheet(&[], &std::collections::BTreeMap::new()) else {
            continue;
        };
        let currency = account.currency;
        let gross = Money::sum(sheet.blocks.iter().map(|block| block.bill.gross), currency);
        let residual = Money::sum(
            sheet.blocks.iter().map(|block| block.bill.residual),
            currency,
        );
        if let (Ok(gross), Ok(residual)) = (gross, residual)
            && let Ok(entries) = gross.checked_sub(residual)
            && let Some(share) = percent(entries, gross)
        {
            shares.push(share);
        }
    }
    let attribution = average(&shares);
    Ok(OpenPeriodSignals {
        period,
        organizations_with_bills: organizations.len(),
        attribution_percent: attribution,
        residual_percent: attribution.map(|share| 100_u8.saturating_sub(share)),
    })
}

fn average(shares: &[u8]) -> Option<u8> {
    let count = u32::try_from(shares.len())
        .ok()
        .filter(|count| *count > 0)?;
    let sum: u32 = shares.iter().map(|share| u32::from(*share)).sum();
    u8::try_from(sum.checked_div(count)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_round_down() {
        assert_eq!(average(&[]), None);
        assert_eq!(average(&[90]), Some(90));
        assert_eq!(average(&[90, 95]), Some(92));
        assert_eq!(average(&[100, 100]), Some(100));
    }
}
