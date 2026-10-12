//! Everything one period's account is computed from: its cost lines in the
//! reporting currency, its bills, its adjustments and its ladder. An open
//! period reads the latest of each; a closed one reads exactly what it was
//! closed with (ADR 0007).

use std::collections::BTreeSet;

use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use tokencontroller_core::{Currency, Money};
use uuid::Uuid;

use super::ladder::Ladder;
use super::period::Period;
use crate::bills::amount::ExchangeRate;
use crate::bills::rates::{Converter, RateError};
use crate::bills::{BillView, latest_bills};
use crate::reports::filter::{Filter, Tags};
use crate::reports::lines::{self, CostLine, LineQuery, StatusView};
use crate::reports::scope::Scope;

/// One bill as an account uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    pub view: BillView,
    pub revision_id: Uuid,
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
    /// To the reporting currency.
    pub rate: ExchangeRate,
}

/// A cost line with its value in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub line: CostLine,
    /// `None` for an unknown model.
    pub value: Option<Money>,
}

/// A signed correction to a closed period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment {
    pub id: Uuid,
    pub amount: Money,
    pub tags: Tags,
    pub reason: String,
    pub late: bool,
    pub token_entry_id: Option<Uuid>,
    pub posted_by: Option<Uuid>,
    pub posted_at: OffsetDateTime,
}

/// How a period was closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    pub closed_at: OffsetDateTime,
    pub closed_by: Uuid,
    pub approval: bool,
    pub tag_map_version_id: Option<Uuid>,
    pub price_table_version_id: Option<Uuid>,
}

/// One period's account.
#[derive(Debug, Clone)]
pub struct Account {
    pub organization_id: Uuid,
    pub period: Period,
    pub currency: Currency,
    pub closed: Option<Closed>,
    pub ladder: Ladder,
    pub lines: Vec<Line>,
    pub bills: Vec<Bill>,
    /// Bills restated after the close: the latest revision differs from the
    /// one the period closed with.
    pub restated: Vec<BillView>,
    pub adjustments: Vec<Adjustment>,
    pub converter: Converter,
}

/// Why an account could not be computed.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    Rate(#[from] RateError),
    #[error("the organization does not exist")]
    NoOrganization,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<AccountError> for crate::error::ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Rate(rate) => Self::Conflict(rate.to_string()),
            AccountError::NoOrganization => Self::NotFound,
            AccountError::Database(database) => Self::Database(database),
        }
    }
}

/// The day the period's figures convert at: its last day, or today while
/// it is still running.
fn rate_day(period: Period, today: Date) -> Date {
    period.last_day().min(today)
}

/// Loads a period's account, as `scope` may see it and limited to `filter`.
/// The cost sheet and reconciliation read it with [`Scope::everything`] and
/// no filter.
///
/// # Errors
///
/// [`AccountError::Rate`] when a figure cannot be converted to the
/// reporting currency, or a database error.
pub async fn load(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    view: StatusView,
    scope: &Scope,
    filter: &Filter,
) -> Result<Account, AccountError> {
    let currency = crate::bills::reporting_currency(db, organization_id)
        .await?
        .ok_or(AccountError::NoOrganization)?;
    let closed = closed_info(db, organization_id, period).await?;
    let today = OffsetDateTime::now_utc().date();

    let ladder = match &closed {
        Some(closed) => match closed.tag_map_version_id {
            Some(version) => ladder_of_version(db, version).await?,
            None => Ladder::default(),
        },
        None => current_ladder(db, organization_id).await?.1,
    };

    let (bills, restated) = match &closed {
        Some(_) => closed_bills(db, organization_id, period).await?,
        None => (open_bills(db, organization_id, period, currency, rate_day(period, today)).await?, Vec::new()),
    };

    let statuses = if closed.is_some() {
        vec!["locked".to_owned()]
    } else {
        view.statuses()
    };
    let query = LineQuery {
        organization_id,
        from: period.starts_at(),
        until: period.ends_at().map_err(|_| AccountError::NoOrganization)?,
        statuses,
        filter: filter.clone(),
        with_late: false,
    };
    let cost_lines = lines::fetch(db, &query, scope).await?;

    let mut converter = Converter::new(currency, rate_day(period, today));
    for bill in &bills {
        converter.with_bill_rate(&bill.view.provider, bill.view.currency, bill.rate);
    }
    if closed.is_some() {
        for (from, rate) in closed_rates(db, organization_id, period).await? {
            converter.with_rate(from, rate);
        }
    } else {
        let currencies: BTreeSet<Currency> = cost_lines
            .iter()
            .filter_map(|line| line.list_value.map(Money::currency))
            .collect();
        let currencies: Vec<Currency> = currencies.into_iter().collect();
        converter.load_rates(db, &currencies).await?;
    }

    let lines = cost_lines
        .into_iter()
        .map(|line| {
            let value = line
                .list_value
                .map(|amount| converter.convert(Some(&line.provider), amount))
                .transpose()?;
            Ok(Line { line, value })
        })
        .collect::<Result<Vec<_>, RateError>>()?;

    let adjustments = if closed.is_some() {
        adjustments(db, organization_id, period, currency).await?
    } else {
        Vec::new()
    };

    Ok(Account {
        organization_id,
        period,
        currency,
        closed,
        ladder,
        lines,
        bills,
        restated,
        adjustments,
        converter,
    })
}

/// How the period was closed, or `None` while it is open.
///
/// # Errors
///
/// A database error.
pub async fn closed_info(db: &PgPool, organization_id: Uuid, period: Period) -> Result<Option<Closed>, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT closed_at, closed_by, approval, tag_map_version_id, price_table_version_id
           FROM closed_periods WHERE organization_id = $1 AND period = $2"#,
        organization_id,
        period.first_day(),
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| Closed {
        closed_at: row.closed_at,
        closed_by: row.closed_by,
        approval: row.approval,
        tag_map_version_id: row.tag_map_version_id,
        price_table_version_id: row.price_table_version_id,
    }))
}

/// The tag map version in force and its ladder. An organization that never
/// changed its tag map has the default ladder and no version yet.
///
/// # Errors
///
/// A database error.
pub async fn current_ladder(db: &PgPool, organization_id: Uuid) -> Result<(Option<Uuid>, Ladder), sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT id, ladder FROM tag_map_versions WHERE organization_id = $1
           ORDER BY created_at DESC, id DESC LIMIT 1"#,
        organization_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map_or_else(
        || (None, Ladder::default()),
        |row| (Some(row.id), Ladder::new(row.ladder)),
    ))
}

async fn ladder_of_version(db: &PgPool, version_id: Uuid) -> Result<Ladder, sqlx::Error> {
    let ladder = sqlx::query_scalar!("SELECT ladder FROM tag_map_versions WHERE id = $1", version_id)
        .fetch_one(db)
        .await?;
    Ok(Ladder::new(ladder))
}

fn bill_figures(view: BillView, revision_id: Uuid, nanos: [i64; 4], rate: ExchangeRate) -> Bill {
    let currency = view.currency;
    let [gross, discount, credit, net] = nanos.map(|n| Money::from_nanos(n, currency));
    Bill {
        view,
        revision_id,
        gross,
        discount,
        credit,
        net,
        rate,
    }
}

async fn open_bills(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    reporting: Currency,
    day: Date,
) -> Result<Vec<Bill>, AccountError> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT ON (b.id) b.id AS bill_id, r.id AS revision_id, r.gross_nanos,
                  r.discount_nanos, r.credit_nanos, r.net_nanos, r.exchange_rate_nanos, r.currency
           FROM bills b JOIN bill_revisions r ON r.bill_id = b.id
           WHERE b.organization_id = $1 AND b.period = $2
           ORDER BY b.id, r.revision DESC"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    let views = latest_bills(db, organization_id, Some(period)).await?;
    let mut bills = Vec::new();
    for view in views {
        let Some(row) = rows.iter().find(|row| row.bill_id == view.id) else {
            continue;
        };
        let rate = match row.exchange_rate_nanos.and_then(|nanos| ExchangeRate::from_nanos(nanos).ok()) {
            Some(rate) => rate,
            None => crate::bills::rates::between(db, view.currency, reporting, day).await??,
        };
        bills.push(bill_figures(
            view,
            row.revision_id,
            [row.gross_nanos, row.discount_nanos, row.credit_nanos, row.net_nanos],
            rate,
        ));
    }
    Ok(bills)
}

async fn closed_bills(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<(Vec<Bill>, Vec<BillView>), AccountError> {
    let rows = sqlx::query!(
        r#"SELECT r.id AS revision_id, r.bill_id, r.revision, r.gross_nanos, r.discount_nanos,
                  r.credit_nanos, r.net_nanos, c.exchange_rate_nanos
           FROM closed_period_bills c JOIN bill_revisions r ON r.id = c.bill_revision_id
           WHERE c.organization_id = $1 AND c.period = $2"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    let latest = latest_bills(db, organization_id, Some(period)).await?;
    let mut bills = Vec::new();
    for row in rows {
        let history = crate::bills::revisions(db, organization_id, row.bill_id).await?;
        let Some(view) = history.into_iter().find(|view| view.revision == row.revision) else {
            continue;
        };
        let rate = ExchangeRate::from_nanos(row.exchange_rate_nanos)
            .map_err(|error| AccountError::Rate(RateError::Amount(error)))?;
        bills.push(bill_figures(
            view,
            row.revision_id,
            [row.gross_nanos, row.discount_nanos, row.credit_nanos, row.net_nanos],
            rate,
        ));
    }
    let restated = latest
        .into_iter()
        .filter(|view| {
            !bills
                .iter()
                .any(|bill| bill.view.id == view.id && bill.view.revision == view.revision)
        })
        .collect();
    Ok((bills, restated))
}

async fn closed_rates(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<Vec<(Currency, ExchangeRate)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT currency, exchange_rate_nanos FROM closed_period_rates
           WHERE organization_id = $1 AND period = $2"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some((
                Currency::new(&row.currency).ok()?,
                ExchangeRate::from_nanos(row.exchange_rate_nanos).ok()?,
            ))
        })
        .collect())
}

async fn adjustments(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    currency: Currency,
) -> Result<Vec<Adjustment>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT id, amount_nanos, currency, tags, reason, late, token_entry_id, posted_by, posted_at
           FROM adjustments WHERE organization_id = $1 AND period = $2
           ORDER BY posted_at, id"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|row| row.currency == currency.code())
        .map(|row| Adjustment {
            id: row.id,
            amount: Money::from_nanos(row.amount_nanos, currency),
            tags: Filter::from_json(&row.tags).0,
            reason: row.reason,
            late: row.late,
            token_entry_id: row.token_entry_id,
            posted_by: row.posted_by,
            posted_at: row.posted_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn a_running_period_converts_at_todays_rate() {
        let september: Period = "2026-09".parse().unwrap();
        assert_eq!(rate_day(september, date!(2026 - 09 - 12)), date!(2026 - 09 - 12));
        assert_eq!(rate_day(september, date!(2026 - 10 - 03)), date!(2026 - 09 - 30));
    }
}
