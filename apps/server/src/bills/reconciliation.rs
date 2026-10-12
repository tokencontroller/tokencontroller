//! Reconciliation: per period and per bill, token entries plus residual
//! equal the bill (docs: Account > Bills, reconciliation). Each bill is
//! checked on its own, in its own currency, against the list value of the
//! token entries it charges; residual is the part no token entry explains
//! and is always shown. Tags play no part. The check is the core's
//! [`tokencontroller_core::Bill::reconcile`], through the cost sheet it
//! builds.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use tokencontroller_core::cost_sheet::Block;
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::{IntoParams, ToSchema};

use super::BillView;
use super::amount::format_amount;
use crate::AppState;
use crate::close::account::{self, Account, AccountError};
use crate::close::period::Period;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::Filter;
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

/// One bill against the token entries it charges, in the bill's currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillCheck {
    /// The revision checked: the latest, or for a closed period the one it
    /// closed with.
    pub bill: BillView,
    #[schema(value_type = String, example = "USD")]
    pub currency: Currency,
    /// The bill's gross, at list price before discount and credit.
    pub gross: String,
    pub discount: String,
    pub credit: String,
    pub net: String,
    /// The list value of the token entries it charges.
    pub token_entries: String,
    /// Gross minus token entries: what no token entry explains. Negative
    /// when token entries come to more than the bill.
    pub residual: String,
    /// How many token entry splits were counted.
    pub token_entry_count: usize,
    /// Token entries on a model missing from the price table. Their cost is
    /// unknown, so it sits in residual, never as zero.
    pub unknown_model_count: usize,
    /// The share of the gross that token entries explain, in percent.
    pub attribution_percent: Option<u8>,
}

/// Token entries from a provider without a bill this period. On a seat plan
/// or a subscription that is expected: their list value is an estimate at
/// API prices and stays off the cost sheet. On a metered plan the bill is
/// still missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct WithoutBill {
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
    /// List value of token entries on no bill, labelled an estimate.
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
    /// Whether the organization's usage is metered, so every provider's
    /// usage needs a bill. False on a seat plan or a subscription.
    pub metered: bool,
    pub bills: Vec<BillCheck>,
    pub without_bill: Vec<WithoutBill>,
    pub totals: Totals,
    /// Bills a provider restated after the period closed. The closed figures
    /// stay as they were; a controller can post an adjustment.
    pub restated_after_close: Vec<BillView>,
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

/// Splits and unknown models counted per bill, and per provider without a
/// bill: (list value in the reporting currency, splits, unknown models).
type Counts = (
    BTreeMap<usize, (usize, usize)>,
    BTreeMap<String, (Money, usize, usize)>,
);

fn counts(account: &Account) -> Result<Counts, MoneyError> {
    let mut on_bills: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut without: BTreeMap<String, (Money, usize, usize)> = BTreeMap::new();
    for line in account.counted() {
        let unknown = usize::from(line.value.is_none());
        if let Some(index) = line.bill {
            let entry = on_bills.entry(index).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(unknown);
        } else {
            let entry = without.entry(line.line.provider.clone()).or_insert((
                Money::zero(account.currency),
                0,
                0,
            ));
            if let Some(value) = line.value {
                entry.0 = entry.0.checked_add(value)?;
            }
            entry.1 = entry.1.saturating_add(1);
            entry.2 = entry.2.saturating_add(unknown);
        }
    }
    Ok((on_bills, without))
}

/// The reconciliation of an account, from the core's cost sheet blocks.
///
/// # Errors
///
/// [`AccountError`] when a rate is missing or a bill does not add up.
pub fn reconcile(account: &Account, blocks: &[Block]) -> Result<Reconciliation, AccountError> {
    let reporting = account.currency;
    let money_error = |error: MoneyError| AccountError::Api(ApiError::Internal(error.into()));
    let (on_bills, without) = counts(account).map_err(money_error)?;
    let bills = account
        .bills
        .iter()
        .zip(blocks)
        .enumerate()
        .map(|(index, (bill, block))| {
            let (count, unknown) = on_bills.get(&index).copied().unwrap_or((0, 0));
            let checked = block.bill.reconciliation;
            BillCheck {
                bill: bill.view.clone(),
                currency: bill.bill.currency(),
                gross: format_amount(bill.bill.gross),
                discount: format_amount(bill.bill.discount),
                credit: format_amount(bill.bill.credit),
                net: format_amount(bill.bill.net),
                token_entries: format_amount(checked.explained),
                residual: format_amount(checked.residual),
                token_entry_count: count,
                unknown_model_count: unknown,
                attribution_percent: percent(checked.explained, bill.bill.gross),
            }
        })
        .collect();

    let sum = |pick: fn(&Block) -> Money| Money::sum(blocks.iter().map(pick), reporting);
    let gross = sum(|block| block.bill.gross).map_err(money_error)?;
    let residual = sum(|block| block.bill.residual).map_err(money_error)?;
    let estimates = Money::sum(without.values().map(|(amount, _, _)| *amount), reporting)
        .map_err(money_error)?;
    let totals = Totals {
        token_entries: format_amount(gross.checked_sub(residual).map_err(money_error)?),
        residual: format_amount(residual),
        discount: format_amount(sum(|block| block.bill.discount).map_err(money_error)?),
        credit: format_amount(sum(|block| block.bill.credit).map_err(money_error)?),
        net: format_amount(sum(|block| block.bill.net).map_err(money_error)?),
        estimates: format_amount(estimates),
    };
    Ok(Reconciliation {
        period: account.period,
        closed: account.closed.is_some(),
        reporting_currency: reporting,
        metered: account.metered,
        bills,
        without_bill: without
            .into_iter()
            .map(|(provider, (amount, count, unknown))| WithoutBill {
                provider,
                token_entries: format_amount(amount),
                token_entry_count: count,
                unknown_model_count: unknown,
            })
            .collect(),
        totals,
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
    let sheet = account.cost_sheet(&[], &BTreeMap::new())?;
    Ok(Json(reconcile(&account, &sheet.blocks)?))
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
