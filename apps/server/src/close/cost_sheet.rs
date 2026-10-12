//! The cost sheet: the locked account of one closed period for the whole
//! organization, adding up to the bill (docs: Report > The cost sheet). It
//! shows cost per rung of the ladder by cost kind, project and organization
//! overhead, residual as its own line, adjustments posted after the close,
//! and the reconciliation against each bill.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::ToSchema;
use uuid::Uuid;

use super::account::Account;
use super::ladder::{CostKind, CostObject};
use super::period::Period;
use crate::bills::amount::format_amount;
use crate::bills::reconciliation::{Reconciliation, provider_figures, reconcile};
use crate::reports::csv::Csv;
use crate::reports::filter::Tags;

/// One line of cost: everything with the same cost object, cost kind and
/// extra columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CostRow {
    /// The rung the cost object sits on, such as `ticket`, or
    /// `organization`.
    pub rung: String,
    /// The cost object, such as `KD-214`; empty for the organization.
    pub cost_object: String,
    pub kind: CostKind,
    /// The value of every rung of the ladder the cost carries, such as the
    /// ticket's project, and of each extra column asked for.
    #[schema(value_type = BTreeMap<String, String>)]
    pub tags: Tags,
    #[schema(example = "41.87")]
    pub amount: String,
    pub token_entries: usize,
    /// Token entries on a model missing from the price table: counted, never
    /// valued as zero.
    pub unknown_model: usize,
    /// Whether the amount is a list-price estimate from a provider without a
    /// bill, such as a seat plan.
    pub estimate: bool,
}

/// One project's cost.
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

/// One line of the bill that is not a token entry, in the reporting
/// currency: residual, discount or credit of one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillLine {
    pub provider: String,
    pub residual: String,
    pub discount: String,
    pub credit: String,
}

/// An adjustment as the cost sheet shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdjustmentRow {
    pub id: Uuid,
    pub amount: String,
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
    /// The ladder the cost objects were read with, most specific first.
    pub ladder: Vec<String>,
    /// The extra tag names asked for as columns.
    pub columns: Vec<String>,
    pub rows: Vec<CostRow>,
    pub projects: Vec<ProjectTotal>,
    pub kinds: KindTotals,
    pub bill_lines: Vec<BillLine>,
    pub adjustments: Vec<AdjustmentRow>,
    pub reconciliation: Reconciliation,
    /// Token entries plus residual, minus discount and credit, plus
    /// adjustments.
    pub total: String,
}

#[derive(Default)]
struct Bucket {
    amount: i64,
    entries: usize,
    unknown: usize,
}

fn add(bucket: &mut Bucket, value: Option<Money>) -> Result<(), MoneyError> {
    match value {
        Some(value) => {
            bucket.amount = bucket.amount.checked_add(value.nanos()).ok_or(MoneyError::Overflow)?;
        }
        None => bucket.unknown = bucket.unknown.saturating_add(1),
    }
    bucket.entries = bucket.entries.saturating_add(1);
    Ok(())
}

/// What one cost row groups by.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    project: String,
    shown: Vec<(String, String)>,
    kind: CostKind,
    estimate: bool,
    rung: String,
    object: String,
}

/// The token entries of an account added up three ways.
#[derive(Default)]
struct Grouped {
    rows: BTreeMap<RowKey, Bucket>,
    kinds: BTreeMap<CostKind, i64>,
    /// Per project: direct, then project overhead.
    projects: BTreeMap<String, (i64, i64)>,
}

fn group(account: &Account, columns: &[String], billed: &[&str]) -> Result<Grouped, MoneyError> {
    let mut grouped = Grouped::default();
    for line in &account.lines {
        let tags = &line.line.tags;
        let kind = account.ladder.cost_kind(tags);
        let (rung, object) = match account.ladder.cost_object(tags) {
            CostObject::Rung { name, value } => (name, value),
            CostObject::Organization => ("organization".to_owned(), String::new()),
        };
        let shown: Vec<(String, String)> = account
            .ladder
            .rungs()
            .iter()
            .chain(columns.iter())
            .filter_map(|name| tags.get(name).map(|value| (name.clone(), value.clone())))
            .collect();
        let key = RowKey {
            project: tags.get("project").cloned().unwrap_or_default(),
            shown,
            kind,
            estimate: !billed.contains(&line.line.provider.as_str()),
            rung,
            object,
        };
        add(grouped.rows.entry(key).or_default(), line.value)?;
        let Some(value) = line.value else { continue };
        let total = grouped.kinds.entry(kind).or_insert(0);
        *total = total.checked_add(value.nanos()).ok_or(MoneyError::Overflow)?;
        if let Some(project) = tags.get("project") {
            let entry = grouped.projects.entry(project.clone()).or_insert((0, 0));
            let slot = if kind == CostKind::Direct {
                &mut entry.0
            } else {
                &mut entry.1
            };
            *slot = slot.checked_add(value.nanos()).ok_or(MoneyError::Overflow)?;
        }
    }
    Ok(grouped)
}

/// Computes the cost sheet of an account. `columns` are extra tag names
/// shown on each row, such as `team`; rows then split by them too.
/// `names` gives the name of whoever posted each adjustment.
///
/// # Errors
///
/// [`MoneyError`] on an overflow.
pub fn compute(
    account: &Account,
    columns: &[String],
    names: &BTreeMap<Uuid, String>,
) -> Result<CostSheet, MoneyError> {
    let currency = account.currency;
    let reconciliation = reconcile(account)?;
    let figures = provider_figures(account)?;
    let billed: Vec<&str> = figures.iter().map(|figure| figure.provider.as_str()).collect();
    let Grouped { rows, kinds, projects } = group(account, columns, &billed)?;

    let money = |nanos: i64| format_amount(Money::from_nanos(nanos, currency));
    let rows = rows
        .into_iter()
        .map(|(key, bucket)| CostRow {
            rung: key.rung,
            cost_object: key.object,
            kind: key.kind,
            tags: key.shown.into_iter().collect(),
            amount: money(bucket.amount),
            token_entries: bucket.entries,
            unknown_model: bucket.unknown,
            estimate: key.estimate,
        })
        .collect();

    let projects = projects
        .into_iter()
        .map(|(project, (direct, overhead))| {
            Ok(ProjectTotal {
                project,
                direct: money(direct),
                project_overhead: money(overhead),
                total: money(direct.checked_add(overhead).ok_or(MoneyError::Overflow)?),
            })
        })
        .collect::<Result<Vec<_>, MoneyError>>()?;

    let kind_total = |kind| kinds.get(&kind).copied().unwrap_or(0);
    let all_entries = Money::from_nanos(
        kinds
            .values()
            .try_fold(0_i64, |sum, value| sum.checked_add(*value))
            .ok_or(MoneyError::Overflow)?,
        currency,
    );
    let share = |kind| {
        crate::bills::reconciliation::percent(Money::from_nanos(kind_total(kind), currency), all_entries)
    };
    let kinds_view = KindTotals {
        direct: money(kind_total(CostKind::Direct)),
        project_overhead: money(kind_total(CostKind::ProjectOverhead)),
        organization_overhead: money(kind_total(CostKind::OrganizationOverhead)),
        direct_percent: share(CostKind::Direct),
        project_overhead_percent: share(CostKind::ProjectOverhead),
        organization_overhead_percent: share(CostKind::OrganizationOverhead),
    };

    let bill_lines = figures
        .iter()
        .map(|figure| BillLine {
            provider: figure.provider.clone(),
            residual: format_amount(figure.residual_reporting),
            discount: format_amount(figure.discount_reporting),
            credit: format_amount(figure.credit_reporting),
        })
        .collect();

    let adjustments_total = Money::sum(account.adjustments.iter().map(|a| a.amount), currency)?;
    let adjustments = account
        .adjustments
        .iter()
        .map(|adjustment| AdjustmentRow {
            id: adjustment.id,
            amount: format_amount(adjustment.amount),
            tags: adjustment.tags.clone(),
            reason: adjustment.reason.clone(),
            late: adjustment.late,
            posted_by: adjustment.posted_by.and_then(|id| names.get(&id).cloned()),
            posted_at: adjustment.posted_at,
        })
        .collect();

    let mut total = all_entries;
    for figure in &figures {
        total = total
            .checked_add(figure.residual_reporting)?
            .checked_sub(figure.discount_reporting)?
            .checked_sub(figure.credit_reporting)?;
    }
    let total = total.checked_add(adjustments_total)?;

    Ok(CostSheet {
        period: account.period,
        closed: account.closed.is_some(),
        closed_at: account.closed.as_ref().map(|closed| closed.closed_at),
        currency,
        ladder: account.ladder.rungs().to_vec(),
        columns: columns.to_vec(),
        rows,
        projects,
        kinds: kinds_view,
        bill_lines,
        adjustments,
        reconciliation,
        total: format_amount(total),
    })
}

/// A decimal amount with the other sign: discount and credit reduce the
/// bill, so the amount column of the CSV adds up to the total.
fn negate(amount: &str) -> String {
    if amount.chars().all(|c| c == '0' || c == '.') {
        amount.to_owned()
    } else if let Some(positive) = amount.strip_prefix('-') {
        positive.to_owned()
    } else {
        format!("-{amount}")
    }
}

/// The cost sheet as one CSV file: a `line` column says whether a row is
/// cost, residual, discount, credit, an adjustment or the total.
#[must_use]
pub fn to_csv(sheet: &CostSheet) -> Csv {
    let mut header: Vec<String> = vec!["line".to_owned(), "period".to_owned()];
    header.extend(sheet.ladder.iter().cloned());
    header.extend(sheet.columns.iter().cloned());
    header.extend(
        ["cost_kind", "provider", "reason", "estimate", "token_entries", "amount", "currency"]
            .map(str::to_owned),
    );
    let tag_count = sheet.ladder.len().saturating_add(sheet.columns.len());
    let tag_names: Vec<&String> = sheet.ladder.iter().chain(sheet.columns.iter()).collect();
    let period = sheet.period.to_string();
    let currency = sheet.currency.code();
    let mut csv = Csv::new(header);

    let tag_cells = |tags: &Tags| -> Vec<String> {
        tag_names
            .iter()
            .map(|name| tags.get(*name).cloned().unwrap_or_default())
            .collect()
    };
    let blank_tags = vec![String::new(); tag_count];

    for row in &sheet.rows {
        let mut cells = vec!["cost".to_owned(), period.clone()];
        cells.extend(tag_cells(&row.tags));
        cells.extend([
            row.kind.as_str().to_owned(),
            String::new(),
            String::new(),
            if row.estimate { "yes" } else { "no" }.to_owned(),
            row.token_entries.to_string(),
            row.amount.clone(),
            currency.clone(),
        ]);
        csv.row(cells);
    }
    for line in &sheet.bill_lines {
        for (kind, amount) in [
            ("residual", &line.residual),
            ("discount", &line.discount),
            ("credit", &line.credit),
        ] {
            let mut cells = vec![kind.to_owned(), period.clone()];
            cells.extend(blank_tags.iter().cloned());
            cells.extend([
                String::new(),
                line.provider.clone(),
                String::new(),
                "no".to_owned(),
                String::new(),
                if kind == "residual" {
                    amount.clone()
                } else {
                    negate(amount)
                },
                currency.clone(),
            ]);
            csv.row(cells);
        }
    }
    for adjustment in &sheet.adjustments {
        let mut cells = vec![
            if adjustment.late { "late_adjustment" } else { "adjustment" }.to_owned(),
            period.clone(),
        ];
        cells.extend(tag_cells(&adjustment.tags));
        cells.extend([
            String::new(),
            String::new(),
            adjustment.reason.clone(),
            "no".to_owned(),
            String::new(),
            adjustment.amount.clone(),
            currency.clone(),
        ]);
        csv.row(cells);
    }
    let mut cells = vec!["total".to_owned(), period];
    cells.extend(blank_tags);
    cells.extend([
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        sheet.total.clone(),
        currency,
    ]);
    csv.row(cells);
    csv
}
