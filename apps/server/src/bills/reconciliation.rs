//! Reconciliation: per period and per bill, token entries plus residual
//! equal the bill (docs: Account > Bills, reconciliation). Each provider's
//! bills are checked on their own, in the bill's currency, against the list
//! value of that provider's token entries; residual is the part no token
//! entry explains and is always shown. Tags play no part.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::{IntoParams, ToSchema};

use super::BillView;
use super::amount::format_amount;
use crate::AppState;
use crate::close::account::{self, Account};
use crate::close::period::Period;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::Filter;
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

/// One provider's check: its bills against its token entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProviderCheck {
    pub provider: String,
    /// The bills of the period from this provider, latest revision (or, for
    /// a closed period, the revision it closed with).
    pub bills: Vec<BillView>,
    #[schema(value_type = String, example = "USD")]
    pub currency: Currency,
    /// The bills' gross, at list price before discount and credit.
    pub gross: String,
    pub discount: String,
    pub credit: String,
    pub net: String,
    /// The list value of the provider's token entries.
    pub token_entries: String,
    /// Gross minus token entries: what no token entry explains.
    pub residual: String,
    /// How many token entries were counted.
    pub token_entry_count: usize,
    /// Token entries on a model missing from the price table. Their cost is
    /// unknown, so it sits in residual, never as zero.
    pub unknown_model_count: usize,
    /// The share of the gross that token entries explain, in percent.
    pub attribution_percent: Option<u8>,
}

/// Token entries from a provider without a bill this period, such as a seat
/// plan: their list value is an estimate at API prices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Estimate {
    pub provider: String,
    /// In the reporting currency.
    pub token_entries: String,
    pub token_entry_count: usize,
    pub unknown_model_count: usize,
}

/// The organization-wide figures, in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Totals {
    pub token_entries: String,
    pub residual: String,
    pub discount: String,
    pub credit: String,
    /// What the bills charged: token entries plus residual minus discount
    /// and credit.
    pub net: String,
    /// List value of token entries without a bill, labelled an estimate.
    pub estimates: String,
}

/// A period's reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Reconciliation {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    pub closed: bool,
    #[schema(value_type = String, example = "EUR")]
    pub reporting_currency: Currency,
    pub providers: Vec<ProviderCheck>,
    pub estimates: Vec<Estimate>,
    pub totals: Totals,
    /// Bills a provider restated after the period closed. The closed figures
    /// stay as they were; a controller can post an adjustment.
    pub restated_after_close: Vec<BillView>,
}

/// The figures of one provider, in money, for the cost sheet and status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFigures {
    pub provider: String,
    pub currency: Currency,
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
    pub entries: Money,
    pub residual: Money,
    /// The same, in the reporting currency.
    pub entries_reporting: Money,
    pub residual_reporting: Money,
    pub discount_reporting: Money,
    pub credit_reporting: Money,
    pub net_reporting: Money,
    pub entry_count: usize,
    pub unknown_count: usize,
}

/// Computes every provider's check of an account.
///
/// # Errors
///
/// [`MoneyError`] on an overflow or when a provider's bills are in more
/// than one currency.
pub fn provider_figures(account: &Account) -> Result<Vec<ProviderFigures>, MoneyError> {
    let mut providers: BTreeMap<&str, Vec<&account::Bill>> = BTreeMap::new();
    for bill in &account.bills {
        providers.entry(bill.view.provider.as_str()).or_default().push(bill);
    }
    let reporting = account.currency;
    providers
        .into_iter()
        .map(|(provider, bills)| {
            let currency = bills.first().map_or(reporting, |bill| bill.view.currency);
            let sum = |pick: fn(&account::Bill) -> Money| Money::sum(bills.iter().map(|bill| pick(bill)), currency);
            let sum_reporting = |pick: fn(&account::Bill) -> Money| -> Result<Money, MoneyError> {
                let converted = bills
                    .iter()
                    .map(|bill| {
                        bill.rate
                            .convert(pick(bill), reporting)
                            .map_err(|_| MoneyError::Overflow)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Money::sum(converted, reporting)
            };
            let gross = sum(|bill| bill.gross)?;
            let lines: Vec<_> = account
                .lines
                .iter()
                .filter(|line| line.line.provider == provider)
                .collect();
            let known: Vec<_> = lines.iter().filter_map(|line| line.line.list_value.zip(line.value)).collect();
            let entries = Money::sum(
                known
                    .iter()
                    .map(|(list, _)| *list)
                    .filter(|list| list.currency() == currency),
                currency,
            )?;
            let entries_reporting = Money::sum(known.iter().map(|(_, value)| *value), reporting)?;
            let gross_reporting = sum_reporting(|bill| bill.gross)?;
            Ok(ProviderFigures {
                provider: provider.to_owned(),
                currency,
                gross,
                discount: sum(|bill| bill.discount)?,
                credit: sum(|bill| bill.credit)?,
                net: sum(|bill| bill.net)?,
                entries,
                residual: gross.checked_sub(entries)?,
                entries_reporting,
                residual_reporting: gross_reporting.checked_sub(entries_reporting)?,
                discount_reporting: sum_reporting(|bill| bill.discount)?,
                credit_reporting: sum_reporting(|bill| bill.credit)?,
                net_reporting: gross_reporting
                    .checked_sub(sum_reporting(|bill| bill.discount)?)?
                    .checked_sub(sum_reporting(|bill| bill.credit)?)?,
                entry_count: lines.len(),
                unknown_count: lines.iter().filter(|line| line.value.is_none()).count(),
            })
        })
        .collect()
}

/// The share of `whole` that `part` is, in whole percent rounded down, or
/// `None` for nothing to share.
#[must_use]
pub fn percent(part: Money, whole: Money) -> Option<u8> {
    if whole.nanos() <= 0 {
        return None;
    }
    let share = i128::from(part.nanos().max(0))
        .checked_mul(100)?
        .checked_div(i128::from(whole.nanos()))?;
    u8::try_from(share.min(100)).ok()
}

/// Computes the reconciliation of an account.
///
/// # Errors
///
/// [`MoneyError`] on an overflow.
pub fn reconcile(account: &Account) -> Result<Reconciliation, MoneyError> {
    let reporting = account.currency;
    let figures = provider_figures(account)?;
    let providers = figures
        .iter()
        .map(|figure| ProviderCheck {
            provider: figure.provider.clone(),
            bills: account
                .bills
                .iter()
                .filter(|bill| bill.view.provider == figure.provider)
                .map(|bill| bill.view.clone())
                .collect(),
            currency: figure.currency,
            gross: format_amount(figure.gross),
            discount: format_amount(figure.discount),
            credit: format_amount(figure.credit),
            net: format_amount(figure.net),
            token_entries: format_amount(figure.entries),
            residual: format_amount(figure.residual),
            token_entry_count: figure.entry_count,
            unknown_model_count: figure.unknown_count,
            attribution_percent: percent(figure.entries, figure.gross),
        })
        .collect();

    let mut estimates: BTreeMap<&str, (Money, usize, usize)> = BTreeMap::new();
    for line in &account.lines {
        if figures.iter().any(|figure| figure.provider == line.line.provider) {
            continue;
        }
        let entry = estimates
            .entry(line.line.provider.as_str())
            .or_insert((Money::zero(reporting), 0, 0));
        if let Some(value) = line.value {
            entry.0 = entry.0.checked_add(value)?;
        } else {
            entry.2 = entry.2.saturating_add(1);
        }
        entry.1 = entry.1.saturating_add(1);
    }
    let estimates_total = Money::sum(estimates.values().map(|(amount, _, _)| *amount), reporting)?;
    let estimates = estimates
        .into_iter()
        .map(|(provider, (amount, count, unknown))| Estimate {
            provider: provider.to_owned(),
            token_entries: format_amount(amount),
            token_entry_count: count,
            unknown_model_count: unknown,
        })
        .collect();

    let total = |pick: fn(&ProviderFigures) -> Money| Money::sum(figures.iter().map(pick), reporting);
    Ok(Reconciliation {
        period: account.period,
        closed: account.closed.is_some(),
        reporting_currency: reporting,
        providers,
        estimates,
        totals: Totals {
            token_entries: format_amount(total(|f| f.entries_reporting)?),
            residual: format_amount(total(|f| f.residual_reporting)?),
            discount: format_amount(total(|f| f.discount_reporting)?),
            credit: format_amount(total(|f| f.credit_reporting)?),
            net: format_amount(total(|f| f.net_reporting)?),
            estimates: format_amount(estimates_total),
        },
        restated_after_close: account.restated.clone(),
    })
}

/// Which period to reconcile.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReconciliationQuery {
    /// The period, such as `2026-09`.
    #[param(value_type = String, example = "2026-09")]
    pub period: Period,
}

/// Each bill of a period against its token entries, with residual.
#[utoipa::path(
    get,
    path = "/reconciliation",
    tag = "bills",
    params(ReconciliationQuery),
    responses(
        (status = OK, description = "The period's reconciliation", body = Reconciliation),
        (status = FORBIDDEN, description = "Only a controller can read the reconciliation", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "An exchange rate is missing", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn get_reconciliation(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ReconciliationQuery>,
) -> Result<Json<Reconciliation>, ApiError> {
    caller.require_role(Role::Controller)?;
    let account = account::load(
        &state.db,
        caller.organization_id,
        query.period,
        StatusView::All,
        &Scope::everything(),
        &Filter::default(),
    )
    .await?;
    let reconciliation = reconcile(&account).map_err(|error| ApiError::Internal(error.into()))?;
    Ok(Json(reconciliation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_down_and_stays_in_range() {
        let usd = |cents| Money::from_minor_units(cents, Currency::USD).unwrap();
        assert_eq!(percent(usd(95), usd(100)), Some(95));
        assert_eq!(percent(usd(999), usd(1000)), Some(99));
        assert_eq!(percent(usd(120), usd(100)), Some(100));
        assert_eq!(percent(usd(-5), usd(100)), Some(0));
        assert_eq!(percent(usd(5), usd(0)), None);
        assert_eq!(percent(usd(5), usd(-10)), None);
    }
}
