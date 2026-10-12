//! Everything one period's account is computed from: its cost lines in the
//! reporting currency, its bills, its adjustments and its tag map. An open
//! period reads the latest of each; a closed one reads exactly what it was
//! closed with (ADR 0007). The figures themselves are the core's: bills
//! reconcile with [`Bill::reconcile`] and the cost sheet is built by
//! [`CostSheet::build`].

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use tokencontroller_core::cost_sheet::CostSheetError;
use tokencontroller_core::{
    Bill, BlockRequest, CostSheet, CostSheetRequest, Currency, ExchangeRate, Money, Split, TagMap,
    TagName, TagValue, Version,
};
use uuid::Uuid;

use super::period::{Period, PeriodDates};
use crate::bills::amount::{RateNumber, exchange_rate};
use crate::bills::rates::{Converter, RateError};
use crate::bills::{BillView, latest_bills};
use crate::error::ApiError;
use crate::reports::filter::{Filter, Tags};
use crate::reports::lines::{self, CostLine, LineQuery, StatusView};
use crate::reports::scope::Scope;

/// One bill revision as an account uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBill {
    pub view: BillView,
    pub revision_id: Uuid,
    /// The bill as the core reconciles it, with its rate to the reporting
    /// currency.
    pub bill: Bill,
}

/// A cost line with its value in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub line: CostLine,
    /// `None` for an unknown model.
    pub value: Option<Money>,
    /// The bill that charges it, as an index into [`Account::bills`]; `None`
    /// when its provider has no bill this period.
    pub bill: Option<usize>,
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
    pub tag_map_version: Version,
    pub price_table_version_id: Option<i64>,
}

/// One period's account.
#[derive(Debug, Clone)]
pub struct Account {
    pub organization_id: Uuid,
    pub period: Period,
    pub currency: Currency,
    /// Whether approval is on: for a closed period, as it was at the close.
    pub approval: bool,
    /// Whether the organization's cost source is metered, so its usage is
    /// on bills. A seat plan or subscription has none.
    pub metered: bool,
    pub closed: Option<Closed>,
    pub tag_map: TagMap,
    pub tag_map_version: Version,
    pub lines: Vec<Line>,
    /// The period's bills, in the order their providers charge token
    /// entries: per provider, the first cost source charges them.
    pub bills: Vec<AccountBill>,
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
    #[error(transparent)]
    CostSheet(#[from] CostSheetError),
    #[error("the organization does not exist")]
    NoOrganization,
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Rate(rate) => Self::Conflict(rate.to_string()),
            AccountError::CostSheet(sheet) => Self::Conflict(sheet.to_string()),
            AccountError::NoOrganization => Self::NotFound,
            AccountError::Api(api) => api,
            AccountError::Database(database) => Self::Database(database),
        }
    }
}

/// The day the period's figures convert at: its last day, or today while
/// it is still running.
fn rate_day(period: Period, today: Date) -> Date {
    period.last_day().min(today)
}

/// How the core names a bill, such as `anthropic-usage_at_api_rates-2026-09`.
#[must_use]
pub fn bill_name(view: &BillView) -> String {
    format!(
        "{}-{}-{}",
        view.provider,
        view.cost_source.as_str(),
        view.period
    )
}

struct Settings {
    currency: Currency,
    approval: bool,
    metered: bool,
}

async fn settings(db: &PgPool, organization_id: Uuid) -> Result<Settings, AccountError> {
    let row = sqlx::query!(
        "SELECT reporting_currency, approval, cost_source FROM organizations WHERE id = $1",
        organization_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AccountError::NoOrganization)?;
    Ok(Settings {
        currency: Currency::new(&row.reporting_currency)
            .map_err(|_| AccountError::NoOrganization)?,
        approval: row.approval,
        metered: !matches!(row.cost_source.as_deref(), Some("seat" | "subscription")),
    })
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
    let settings = settings(db, organization_id).await?;
    let currency = settings.currency;
    let closed = closed_info(db, organization_id, period).await?;
    let day = rate_day(period, OffsetDateTime::now_utc().date());

    let (tag_map_version, tag_map) = match &closed {
        Some(closed) => (
            closed.tag_map_version,
            crate::tags::tag_map_version(db, organization_id, closed.tag_map_version).await?,
        ),
        None => crate::tags::tag_map_in_force(db, organization_id).await?,
    };

    let (bills, restated) = match &closed {
        Some(_) => closed_bills(db, organization_id, period, currency).await?,
        None => (
            open_bills(db, organization_id, period, currency, day).await?,
            Vec::new(),
        ),
    };

    let statuses = if closed.is_some() {
        vec![lines::EntryStatus::Locked.as_str().to_owned()]
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

    let charging = charging_bills(&bills);
    let mut converter = Converter::new(currency, day);
    for bill in &bills {
        if let Ok(rate) = bill.bill.rate_to(currency) {
            converter.with_bill_rate(&bill.bill.provider, rate);
        }
    }
    if closed.is_some() {
        for rate in closed_rates(db, organization_id, period).await? {
            converter.with_rate(rate);
        }
    } else {
        let mut pairs: BTreeSet<(Currency, Currency)> = BTreeSet::new();
        for line in &cost_lines {
            pairs.insert((line.currency, currency));
            if let Some(bill) = charging
                .get(line.provider.as_str())
                .and_then(|index| bills.get(*index))
            {
                pairs.insert((line.currency, bill.bill.currency()));
            }
        }
        let pairs: Vec<(Currency, Currency)> = pairs.into_iter().collect();
        converter.load_rates(db, &pairs).await?;
    }

    let lines = cost_lines
        .into_iter()
        .map(|line| {
            let value = line
                .list_value
                .map(|amount| converter.convert(Some(&line.provider), amount))
                .transpose()?;
            let bill = charging.get(line.provider.as_str()).copied();
            Ok(Line { line, value, bill })
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
        approval: closed
            .as_ref()
            .map_or(settings.approval, |closed| closed.approval),
        metered: settings.metered,
        closed,
        tag_map,
        tag_map_version,
        lines,
        bills,
        restated,
        adjustments,
        converter,
    })
}

/// Per provider, the bill that charges its token entries: the first in
/// cost source order.
fn charging_bills(bills: &[AccountBill]) -> BTreeMap<&str, usize> {
    let mut charging = BTreeMap::new();
    for (index, bill) in bills.iter().enumerate() {
        charging.entry(bill.bill.provider.as_str()).or_insert(index);
    }
    charging
}

impl Account {
    /// The lines that count on the cost sheet: with approval on, approved
    /// ones only (the core's [`Status::counts`]).
    pub fn counted(&self) -> impl Iterator<Item = &Line> {
        self.lines
            .iter()
            .filter(|line| line.line.status.core().counts(self.approval))
    }

    /// Providers whose counted token entries are on no bill this period.
    #[must_use]
    pub fn providers_without_bill(&self) -> BTreeMap<String, usize> {
        let mut providers: BTreeMap<String, usize> = BTreeMap::new();
        for line in self.counted().filter(|line| line.bill.is_none()) {
            let count = providers.entry(line.line.provider.clone()).or_insert(0);
            *count = count.saturating_add(1);
        }
        providers
    }

    /// Each bill's counted splits, converted into the bill's currency where
    /// the token entry is in another one.
    ///
    /// # Errors
    ///
    /// [`RateError`] when a rate between the two is missing.
    pub fn bill_splits(&self) -> Result<Vec<Vec<Split>>, RateError> {
        let mut blocks: Vec<Vec<Split>> = vec![Vec::new(); self.bills.len()];
        for line in self.counted() {
            let Some((bill, splits)) = line
                .bill
                .and_then(|index| self.bills.get(index).zip(blocks.get_mut(index)))
            else {
                continue;
            };
            let mut split = line.line.split();
            let to = bill.bill.currency();
            if split.value.list.currency() != to {
                let rate = self.converter.rate(split.value.list.currency(), to)?;
                split.value.list = rate.convert(split.value.list)?;
                split.value.billed = split
                    .value
                    .billed
                    .map(|billed| rate.convert(billed))
                    .transpose()?;
            }
            splits.push(split);
        }
        Ok(blocks)
    }

    /// The adjustments as the core puts them on the cost sheet. `names` gives
    /// the email of whoever posted each; a late token sheet posts on its own.
    #[must_use]
    pub fn core_adjustments(
        &self,
        names: &BTreeMap<Uuid, String>,
    ) -> Vec<tokencontroller_core::Adjustment> {
        let service = TagValue::new("Token Controller").ok();
        self.adjustments
            .iter()
            .filter_map(|adjustment| {
                let posted_by = adjustment
                    .posted_by
                    .and_then(|id| names.get(&id))
                    .and_then(|name| TagValue::new(name).ok())
                    .or_else(|| service.clone())?;
                Some(tokencontroller_core::Adjustment {
                    period: self.period,
                    tags: core_tags(&adjustment.tags),
                    amount: adjustment.amount,
                    reason: adjustment.reason.clone(),
                    posted_by,
                    posted_at: adjustment.posted_at,
                    late: adjustment.late,
                })
            })
            .collect()
    }

    /// The core's cost sheet of this account, with `columns` as extra tag
    /// columns.
    ///
    /// # Errors
    ///
    /// [`AccountError`] when a rate is missing or a bill does not add up.
    pub fn cost_sheet(
        &self,
        columns: &[TagName],
        names: &BTreeMap<Uuid, String>,
    ) -> Result<CostSheet, AccountError> {
        let splits = self.bill_splits()?;
        let adjustments = self.core_adjustments(names);
        let blocks = self
            .bills
            .iter()
            .zip(&splits)
            .map(|(bill, splits)| BlockRequest {
                bill: &bill.bill,
                splits: splits.iter().collect(),
            })
            .collect();
        Ok(CostSheet::build(&CostSheetRequest {
            period: self.period,
            reporting_currency: self.currency,
            tag_map: &self.tag_map,
            columns,
            blocks,
            adjustments: &adjustments,
        })?)
    }
}

/// Tags as the core reads them. A tag the core cannot read is left out; the
/// checks on the way in prevent those.
#[must_use]
pub fn core_tags(tags: &Tags) -> tokencontroller_core::Tags {
    tags.iter()
        .filter_map(|(name, value)| Some((TagName::new(name).ok()?, TagValue::new(value).ok()?)))
        .collect()
}

/// How the period was closed, or `None` while it is open.
///
/// # Errors
///
/// A database error.
pub async fn closed_info(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
) -> Result<Option<Closed>, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT closed_at, closed_by, approval, tag_map_version, price_table_version_id
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
        tag_map_version: u32::try_from(row.tag_map_version)
            .ok()
            .and_then(Version::new)
            .unwrap_or(Version::FIRST),
        price_table_version_id: row.price_table_version_id,
    }))
}

/// A stored revision as the core reads it.
struct RevisionFigures {
    revision_id: Uuid,
    gross: i64,
    discount: i64,
    credit: i64,
    net: i64,
    rate: Option<i64>,
}

fn account_bill(
    view: BillView,
    figures: &RevisionFigures,
    reporting: Currency,
) -> Result<AccountBill, AccountError> {
    let currency = view.currency;
    let money = |nanos: i64| Money::from_nanos(nanos, currency);
    let exchange_rate = match figures.rate {
        Some(nanos) if currency != reporting => Some(
            exchange_rate(
                currency,
                reporting,
                RateNumber::from_nanos(nanos).map_err(RateError::from)?,
            )
            .map_err(RateError::from)?,
        ),
        _ => None,
    };
    let bill = Bill {
        id: bill_name(&view),
        provider: view.provider.clone(),
        cost_source: view.cost_source.core(),
        period: view.period,
        gross: money(figures.gross),
        discount: money(figures.discount),
        credit: money(figures.credit),
        net: money(figures.net),
        exchange_rate,
        revision: u32::try_from(view.revision).unwrap_or(1),
    };
    Ok(AccountBill {
        view,
        revision_id: figures.revision_id,
        bill,
    })
}

async fn open_bills(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    reporting: Currency,
    day: Date,
) -> Result<Vec<AccountBill>, AccountError> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT ON (b.id) b.id AS bill_id, r.id AS revision_id, r.gross_nanos,
                  r.discount_nanos, r.credit_nanos, r.net_nanos, r.exchange_rate_nanos
           FROM bills b JOIN bill_revisions r ON r.bill_id = b.id
           WHERE b.organization_id = $1 AND b.period = $2
           ORDER BY b.id, r.revision DESC"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    let mut views = latest_bills(db, organization_id, Some(period)).await?;
    views.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then(a.cost_source.cmp(&b.cost_source))
    });
    let mut bills = Vec::new();
    for view in views {
        let Some(row) = rows.iter().find(|row| row.bill_id == view.id) else {
            continue;
        };
        let rate = match row.exchange_rate_nanos {
            Some(nanos) => Some(nanos),
            None if view.currency == reporting => None,
            None => Some(
                crate::bills::amount::rate_number(
                    crate::bills::rates::between(db, view.currency, reporting, day).await??,
                )
                .nanos(),
            ),
        };
        let figures = RevisionFigures {
            revision_id: row.revision_id,
            gross: row.gross_nanos,
            discount: row.discount_nanos,
            credit: row.credit_nanos,
            net: row.net_nanos,
            rate,
        };
        bills.push(account_bill(view, &figures, reporting)?);
    }
    Ok(bills)
}

async fn closed_bills(
    db: &PgPool,
    organization_id: Uuid,
    period: Period,
    reporting: Currency,
) -> Result<(Vec<AccountBill>, Vec<BillView>), AccountError> {
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
        let Some(view) = history
            .into_iter()
            .find(|view| view.revision == row.revision)
        else {
            continue;
        };
        let figures = RevisionFigures {
            revision_id: row.revision_id,
            gross: row.gross_nanos,
            discount: row.discount_nanos,
            credit: row.credit_nanos,
            net: row.net_nanos,
            rate: Some(row.exchange_rate_nanos),
        };
        bills.push(account_bill(view, &figures, reporting)?);
    }
    bills.sort_by(|a, b| {
        a.view
            .provider
            .cmp(&b.view.provider)
            .then(a.view.cost_source.cmp(&b.view.cost_source))
    });
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
) -> Result<Vec<ExchangeRate>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT from_currency, to_currency, exchange_rate_nanos FROM closed_period_rates
           WHERE organization_id = $1 AND period = $2"#,
        organization_id,
        period.first_day(),
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            exchange_rate(
                Currency::new(&row.from_currency).ok()?,
                Currency::new(&row.to_currency).ok()?,
                RateNumber::from_nanos(row.exchange_rate_nanos).ok()?,
            )
            .ok()
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
        let september = Period::parse("2026-09").unwrap();
        assert_eq!(
            rate_day(september, date!(2026 - 09 - 12)),
            date!(2026 - 09 - 12)
        );
        assert_eq!(
            rate_day(september, date!(2026 - 10 - 03)),
            date!(2026 - 09 - 30)
        );
    }

    #[test]
    fn adjustment_tags_the_core_cannot_read_are_left_out() {
        let tags: Tags = [("ticket", "KD-214"), ("Bad Name", "x"), ("team", " ")]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let core = core_tags(&tags);
        assert_eq!(core.len(), 1);
        assert_eq!(core.get_str("ticket").map(TagValue::as_str), Some("KD-214"));
    }
}
