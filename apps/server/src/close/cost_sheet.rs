//! The cost sheet: the locked account of one closed period for the whole
//! organization, adding up to the bill (docs: Report > The cost sheet). It
//! shows cost per rung of the ladder by cost kind, project and organization
//! overhead, residual as its own line, adjustments posted after the close,
//! and the reconciliation against each bill.
//!
//! The figures are the core's [`tokencontroller_core::CostSheet`]: one block
//! per bill, its lines converted at the bill's rate. This view shows them in
//! the currency's minor unit the way the core's CSV does, so the shown lines
//! and residual add up to the shown gross, net is gross minus discount minus
//! credit, and the total is the sum of the shown nets and adjustments.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;
use tokencontroller_core::cost_sheet::{Block, Line};
use tokencontroller_core::{CostKind, Currency, Figure, Money, MoneyError, TagName};
use utoipa::ToSchema;
use uuid::Uuid;

use super::account::{Account, AccountError};
use super::period::Period;
use crate::bills::reconciliation::{Reconciliation, reconcile};
use crate::error::ApiError;
use crate::reports::filter::Tags;

/// How the API spells a cost kind, such as `project_overhead`.
#[must_use]
pub const fn kind_name(kind: CostKind) -> &'static str {
    match kind {
        CostKind::Direct => "direct",
        CostKind::ProjectOverhead => "project_overhead",
        CostKind::OrganizationOverhead => "organization_overhead",
    }
}

/// One line of cost: everything on one bill with the same cost object,
/// cost kind and extra columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostRow {
    /// `direct`, `project_overhead` or `organization_overhead`.
    #[schema(example = "direct")]
    pub cost_kind: &'static str,
    /// The value of each rung of the ladder the cost carries, such as the
    /// ticket and its project. A rung without a value is left out.
    #[schema(value_type = BTreeMap<String, String>)]
    pub rungs: Tags,
    /// The value of each extra column asked for.
    #[schema(value_type = BTreeMap<String, String>)]
    pub columns: Tags,
    /// In the reporting currency, rounded so the block adds up.
    #[schema(example = "41.87")]
    pub amount: String,
    /// `list_price`, or `unknown_model` when part of it is on a model
    /// missing from the price table: that part sits in residual.
    #[schema(example = "list_price")]
    pub figure: &'static str,
}

/// One bill's part of the cost sheet, in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillBlock {
    /// The bill, such as `anthropic-usage_at_api_rates-2026-09`.
    pub bill: String,
    pub provider: String,
    pub revision: u32,
    /// From the bill's currency to the reporting currency.
    #[schema(example = "0.9214")]
    pub exchange_rate: String,
    pub lines: Vec<CostRow>,
    /// What no token entry explains.
    pub residual: String,
    /// The lines plus residual.
    pub gross: String,
    pub discount: String,
    pub credit: String,
    /// Gross minus discount minus credit.
    pub net: String,
}

/// One project's cost over every bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProjectTotal {
    pub project: String,
    /// On its tickets.
    pub direct: String,
    /// On the project but on no ticket.
    pub project_overhead: String,
    pub total: String,
}

/// Cost per cost kind: how specific the attribution is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct KindTotals {
    pub direct: String,
    pub project_overhead: String,
    pub organization_overhead: String,
    /// Each kind's share of all token entries, in percent.
    pub direct_percent: Option<u8>,
    pub project_overhead_percent: Option<u8>,
    pub organization_overhead_percent: Option<u8>,
}

/// An adjustment as the cost sheet shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdjustmentRow {
    pub id: Uuid,
    pub amount: String,
    /// The cost kind its tags give.
    #[schema(example = "direct")]
    pub cost_kind: &'static str,
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: Tags,
    pub reason: String,
    /// Posted for a token sheet that came in after the close.
    pub late: bool,
    /// Who posted it; `null` for a late token sheet, posted on its own.
    pub posted_by: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub posted_at: OffsetDateTime,
}

/// The cost sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostSheet {
    #[schema(value_type = String, example = "2026-09")]
    pub period: Period,
    /// False while the period is open: the figures can still change.
    pub closed: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub closed_at: Option<OffsetDateTime>,
    #[schema(value_type = String, example = "EUR")]
    pub currency: Currency,
    /// With approval on, it holds approved cost only.
    pub approval: bool,
    /// The tag map version the cost objects were read with.
    pub tag_map_version: u32,
    /// The rungs of the ladder, broadest first.
    pub rungs: Vec<String>,
    /// The extra tag names asked for as columns.
    pub columns: Vec<String>,
    pub blocks: Vec<BillBlock>,
    pub projects: Vec<ProjectTotal>,
    pub kinds: KindTotals,
    pub adjustments: Vec<AdjustmentRow>,
    pub reconciliation: Reconciliation,
    /// Every bill's net plus every adjustment.
    pub total: String,
}

/// What the lines of one block add to the project and kind totals, in
/// minor units.
#[derive(Default)]
struct Totals {
    kinds: BTreeMap<CostKind, i64>,
    /// Per project: direct, then project overhead.
    projects: BTreeMap<String, (i64, i64)>,
}

impl Totals {
    fn add(&mut self, line: &Line, rungs: &[TagName], minor: i64) -> Result<(), MoneyError> {
        let kind = self.kinds.entry(line.cost_kind).or_insert(0);
        *kind = kind.checked_add(minor).ok_or(MoneyError::Overflow)?;
        let project = rungs
            .iter()
            .zip(&line.rungs)
            .find(|(name, _)| **name == TagName::project())
            .and_then(|(_, value)| value.as_ref());
        if let Some(project) = project {
            let entry = self.projects.entry(project.to_string()).or_insert((0, 0));
            let slot = if line.cost_kind == CostKind::Direct {
                &mut entry.0
            } else {
                &mut entry.1
            };
            *slot = slot.checked_add(minor).ok_or(MoneyError::Overflow)?;
        }
        Ok(())
    }
}

fn named(names: &[TagName], values: &[Option<tokencontroller_core::TagValue>]) -> Tags {
    names
        .iter()
        .zip(values)
        .filter_map(|(name, value)| Some((name.to_string(), value.as_ref()?.to_string())))
        .collect()
}

fn block_view(
    block: &Block,
    provider: &str,
    rungs: &[TagName],
    columns: &[TagName],
    currency: Currency,
    totals: &mut Totals,
) -> Result<(BillBlock, i64), MoneyError> {
    let bill = &block.bill;
    let mut amounts: Vec<Money> = block.lines.iter().map(|line| line.amount).collect();
    amounts.push(bill.residual);
    let shown = Money::round_all_to_minor_units(&amounts, currency)?;
    let format = |minor: i64| currency.format_minor_units(minor);
    let mut lines = Vec::new();
    for (line, minor) in block.lines.iter().zip(&shown) {
        totals.add(line, rungs, *minor)?;
        lines.push(CostRow {
            cost_kind: kind_name(line.cost_kind),
            rungs: named(rungs, &line.rungs),
            columns: named(columns, &line.columns),
            amount: format(*minor),
            figure: match line.figure {
                Figure::UnknownModel => "unknown_model",
                Figure::Estimate => "estimate",
                Figure::Billed => "billed",
                Figure::ListPrice => "list_price",
            },
        });
    }
    let gross = shown
        .iter()
        .try_fold(0_i64, |sum, minor| sum.checked_add(*minor))
        .ok_or(MoneyError::Overflow)?;
    let discount = bill.discount.round_to_minor_units();
    let credit = bill.credit.round_to_minor_units();
    let net = gross
        .checked_sub(discount)
        .and_then(|net| net.checked_sub(credit))
        .ok_or(MoneyError::Overflow)?;
    let view = BillBlock {
        bill: block.bill_id.clone(),
        provider: provider.to_owned(),
        revision: bill.revision,
        exchange_rate: crate::bills::amount::rate_number(bill.rate).to_string(),
        lines,
        residual: format(shown.last().copied().unwrap_or_default()),
        gross: format(gross),
        discount: format(discount),
        credit: format(credit),
        net: format(net),
    };
    Ok((view, net))
}

/// The adjustment rows of the core's sheet, each rounded to minor units as
/// the core's CSV shows them, added to `total`.
fn adjustment_rows(
    account: &Account,
    core: &tokencontroller_core::CostSheet,
    names: &BTreeMap<Uuid, String>,
    total: &mut i64,
) -> Result<Vec<AdjustmentRow>, MoneyError> {
    let currency = account.currency;
    let amounts: Vec<Money> = core
        .adjustments
        .iter()
        .map(|line| line.adjustment.amount)
        .collect();
    let shown = Money::round_all_to_minor_units(&amounts, currency)?;
    let mut rows = Vec::new();
    for ((adjustment, line), minor) in account
        .adjustments
        .iter()
        .zip(&core.adjustments)
        .zip(&shown)
    {
        *total = total.checked_add(*minor).ok_or(MoneyError::Overflow)?;
        rows.push(AdjustmentRow {
            id: adjustment.id,
            amount: currency.format_minor_units(*minor),
            cost_kind: kind_name(line.cost_kind),
            tags: adjustment.tags.clone(),
            reason: adjustment.reason.clone(),
            late: adjustment.late,
            posted_by: adjustment.posted_by.and_then(|id| names.get(&id).cloned()),
            posted_at: adjustment.posted_at,
        });
    }
    Ok(rows)
}

/// The totals per cost kind, with each kind's share of all of them.
fn kind_totals(totals: &Totals, currency: Currency) -> Result<KindTotals, MoneyError> {
    let format = |minor: i64| currency.format_minor_units(minor);
    let kind = |kind: CostKind| totals.kinds.get(&kind).copied().unwrap_or(0);
    let all = totals
        .kinds
        .values()
        .try_fold(0_i64, |sum, minor| sum.checked_add(*minor))
        .ok_or(MoneyError::Overflow)?;
    let share = |kind_minor: i64| {
        let minor = |value: i64| Money::from_minor_units(value, currency).ok();
        crate::bills::reconciliation::percent(minor(kind_minor)?, minor(all)?)
    };
    Ok(KindTotals {
        direct: format(kind(CostKind::Direct)),
        project_overhead: format(kind(CostKind::ProjectOverhead)),
        organization_overhead: format(kind(CostKind::OrganizationOverhead)),
        direct_percent: share(kind(CostKind::Direct)),
        project_overhead_percent: share(kind(CostKind::ProjectOverhead)),
        organization_overhead_percent: share(kind(CostKind::OrganizationOverhead)),
    })
}

/// Computes the cost sheet of an account. `columns` are extra tag names
/// shown on each row, such as `team`; rows then split by them too.
/// `names` gives the name of whoever posted each adjustment, `emails`
/// their email for the core.
///
/// # Errors
///
/// [`AccountError`] when a rate is missing or a bill does not add up.
pub fn compute(
    account: &Account,
    columns: &[String],
    names: &BTreeMap<Uuid, String>,
    emails: &BTreeMap<Uuid, String>,
) -> Result<CostSheet, AccountError> {
    let currency = account.currency;
    let columns = columns
        .iter()
        .map(|column| {
            TagName::new(column)
                .map_err(|error| AccountError::Api(ApiError::BadRequest(error.to_string())))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let core = account.cost_sheet(&columns, emails)?;
    let reconciliation = reconcile(account, &core.blocks)?;
    let money_error = |error: MoneyError| AccountError::Api(ApiError::Internal(error.into()));

    let mut totals = Totals::default();
    let mut blocks = Vec::new();
    let mut total: i64 = 0;
    for (block, bill) in core.blocks.iter().zip(&account.bills) {
        let (view, net) = block_view(
            block,
            &bill.bill.provider,
            &core.rungs,
            &core.columns,
            currency,
            &mut totals,
        )
        .map_err(money_error)?;
        total = total
            .checked_add(net)
            .ok_or(MoneyError::Overflow)
            .map_err(money_error)?;
        blocks.push(view);
    }

    let adjustments = adjustment_rows(account, &core, names, &mut total).map_err(money_error)?;

    let format = |minor: i64| currency.format_minor_units(minor);
    let projects = totals
        .projects
        .iter()
        .map(|(project, (direct, overhead))| {
            Ok(ProjectTotal {
                project: project.clone(),
                direct: format(*direct),
                project_overhead: format(*overhead),
                total: format(direct.checked_add(*overhead).ok_or(MoneyError::Overflow)?),
            })
        })
        .collect::<Result<Vec<_>, MoneyError>>()
        .map_err(money_error)?;
    Ok(CostSheet {
        period: account.period,
        closed: account.closed.is_some(),
        closed_at: account.closed.as_ref().map(|closed| closed.closed_at),
        currency,
        approval: account.approval,
        tag_map_version: account.tag_map_version.number(),
        rungs: core.rungs.iter().map(ToString::to_string).collect(),
        columns: core.columns.iter().map(ToString::to_string).collect(),
        blocks,
        projects,
        kinds: kind_totals(&totals, currency).map_err(money_error)?,
        adjustments,
        reconciliation,
        total: format(total),
    })
}

/// The cost sheet of an account as the core writes it: one row per line,
/// with a `bill`, `line`, `cost_kind`, one column per rung and extra
/// column, `amount` and `note`.
///
/// # Errors
///
/// As [`compute`].
pub fn to_csv(
    account: &Account,
    columns: &[String],
    emails: &BTreeMap<Uuid, String>,
) -> Result<String, AccountError> {
    let columns = columns
        .iter()
        .map(|column| {
            TagName::new(column)
                .map_err(|error| AccountError::Api(ApiError::BadRequest(error.to_string())))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let core = account.cost_sheet(&columns, emails)?;
    core.to_csv()
        .map_err(|error| AccountError::Api(ApiError::Internal(error.into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_kinds_are_spelled_as_the_api_spells_them() {
        assert_eq!(kind_name(CostKind::Direct), "direct");
        assert_eq!(kind_name(CostKind::ProjectOverhead), "project_overhead");
        assert_eq!(
            kind_name(CostKind::OrganizationOverhead),
            "organization_overhead"
        );
    }
}
