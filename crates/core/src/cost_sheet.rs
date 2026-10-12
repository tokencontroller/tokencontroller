//! The cost sheet: the locked account of one closed period, adding up to the
//! bill.
//!
//! One block per bill holds a line per cost object and cost kind, the
//! residual, and the bill's gross, discount, credit and net. Adjustments
//! follow, then the organization total. Seat plans and subscriptions have no
//! bill, so their usage is not on the cost sheet: reports show it as
//! estimates. Amounts are converted into the
//! reporting currency at each bill's exchange rate so that a block's lines
//! still add up to its converted gross, and are shown in the currency's
//! minor unit so that the shown lines add up to the shown totals.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::bill::{Bill, BillError, Reconciliation};
use crate::entry::Split;
use crate::money::{Currency, ExchangeRate, Money, MoneyError};
use crate::period::Period;
use crate::price::Figure;
use crate::tag_map::{CostKind, TagMap};
use crate::tags::{TagName, TagValue, Tags};

/// A cost sheet that cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CostSheetError {
    #[error(transparent)]
    Bill(#[from] BillError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("bill {bill} is for {bill_period}, not for {period}")]
    BillForOtherPeriod {
        bill: String,
        bill_period: Period,
        period: Period,
    },
    #[error("an adjustment for {adjustment_period} cannot go on the cost sheet of {period}")]
    AdjustmentForOtherPeriod {
        adjustment_period: Period,
        period: Period,
    },
}

/// A signed correction a controller posts to a closed period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjustment {
    pub period: Period,
    /// The cost object and any other tags, such as `ticket: KD-214`.
    pub tags: Tags,
    /// Plus or minus, in the reporting currency.
    pub amount: Money,
    pub reason: String,
    pub posted_by: TagValue,
    #[serde(with = "time::serde::rfc3339")]
    pub posted_at: OffsetDateTime,
    /// A token sheet submitted after its period closed.
    pub late: bool,
}

/// The splits charged on one bill.
#[derive(Debug, Clone)]
pub struct BlockRequest<'a> {
    pub bill: &'a Bill,
    /// The splits that count: with approval on, approved ones only.
    pub splits: Vec<&'a Split>,
}

/// Everything a cost sheet is built from.
#[derive(Debug, Clone)]
pub struct CostSheetRequest<'a> {
    pub period: Period,
    pub reporting_currency: Currency,
    /// The tag map the period closes with.
    pub tag_map: &'a TagMap,
    /// Extra tag names to show as columns, such as `team`.
    pub columns: &'a [TagName],
    pub blocks: Vec<BlockRequest<'a>>,
    pub adjustments: &'a [Adjustment],
}

/// One line of a block: the cost of one cost object and cost kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub cost_kind: CostKind,
    /// The value of each rung of the ladder, broadest first.
    pub rungs: Vec<Option<TagValue>>,
    /// The value of each extra column.
    pub columns: Vec<Option<TagValue>>,
    /// In the reporting currency.
    pub amount: Money,
    pub figure: Figure,
}

/// The bill side of a block, in the reporting currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillTotals {
    pub revision: u32,
    pub residual: Money,
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
    /// The reconciliation in the bill's own currency.
    pub reconciliation: Reconciliation,
    pub rate: ExchangeRate,
}

/// One bill's part of the cost sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub bill_id: String,
    pub lines: Vec<Line>,
    pub bill: BillTotals,
}

/// An adjustment on the cost sheet, with the cost kind its tags give.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustmentLine {
    pub adjustment: Adjustment,
    pub cost_kind: CostKind,
}

/// The locked account of one closed period for the whole organization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSheet {
    pub period: Period,
    pub currency: Currency,
    /// The rungs of the ladder, broadest first.
    pub rungs: Vec<TagName>,
    pub columns: Vec<TagName>,
    pub blocks: Vec<Block>,
    pub adjustments: Vec<AdjustmentLine>,
    /// Every bill's net and every adjustment.
    pub total: Money,
}

impl CostSheet {
    /// Builds the cost sheet of a period.
    ///
    /// # Errors
    ///
    /// [`CostSheetError`] if a bill or adjustment is for another period, a
    /// bill does not add up, a rate is missing, or amounts are in different
    /// currencies.
    pub fn build(request: &CostSheetRequest<'_>) -> Result<Self, CostSheetError> {
        let currency = request.reporting_currency;
        let rungs: Vec<TagName> = request.tag_map.ladder().iter().rev().cloned().collect();
        let mut columns: Vec<TagName> = Vec::new();
        for column in request.columns {
            if !rungs.contains(column) && !columns.contains(column) {
                columns.push(column.clone());
            }
        }
        if let Some(adjustment) = request
            .adjustments
            .iter()
            .find(|adjustment| adjustment.period != request.period)
        {
            return Err(CostSheetError::AdjustmentForOtherPeriod {
                adjustment_period: adjustment.period,
                period: request.period,
            });
        }

        let blocks = request
            .blocks
            .iter()
            .map(|block| build_block(request, block, &rungs, &columns))
            .collect::<Result<Vec<_>, _>>()?;
        let mut total = Money::zero(currency);
        for block in &blocks {
            total = total.checked_add(block.bill.net)?;
        }
        for adjustment in request.adjustments {
            total = total.checked_add(adjustment.amount)?;
        }
        Ok(Self {
            period: request.period,
            currency,
            rungs,
            columns,
            blocks,
            adjustments: request
                .adjustments
                .iter()
                .map(|adjustment| AdjustmentLine {
                    adjustment: adjustment.clone(),
                    cost_kind: request.tag_map.cost_kind(&adjustment.tags),
                })
                .collect(),
            total,
        })
    }

    /// The cost sheet as CSV, one row per line, with amounts in the minor
    /// unit. Within a block the shown lines and residual add up to the shown
    /// gross, net is gross minus discount minus credit, and the total row is
    /// the sum of the shown nets and adjustments.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`].
    pub fn to_csv(&self) -> Result<String, MoneyError> {
        let mut csv = Csv {
            sheet: self,
            rows: Vec::new(),
            shown_total: 0,
        };
        let mut header = vec!["bill".to_owned(), "line".to_owned(), "cost_kind".to_owned()];
        header.extend(
            self.rungs
                .iter()
                .chain(&self.columns)
                .map(ToString::to_string),
        );
        header.extend(["amount".to_owned(), "note".to_owned()]);
        csv.rows.push(header);
        for block in &self.blocks {
            csv.block(block)?;
        }
        csv.adjustments()?;
        csv.summary("", "total", csv.shown_total);

        let mut text = String::new();
        for row in csv.rows {
            let cells: Vec<String> = row.iter().map(|cell| csv_cell(cell)).collect();
            let _ignored = writeln!(text, "{}", cells.join(","));
        }
        Ok(text)
    }
}

/// Writes a cost sheet's rows, keeping the shown total.
struct Csv<'a> {
    sheet: &'a CostSheet,
    rows: Vec<Vec<String>>,
    /// The sum of the shown nets and adjustments.
    shown_total: i64,
}

impl Csv<'_> {
    fn format(&self, minor: i64) -> String {
        self.sheet.currency.format_minor_units(minor)
    }

    fn add_to_total(&mut self, minor: i64) -> Result<(), MoneyError> {
        self.shown_total = self
            .shown_total
            .checked_add(minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(())
    }

    fn row(
        &mut self,
        bill: &str,
        line: &str,
        kind: &str,
        tags: Vec<String>,
        minor: i64,
        note: &str,
    ) {
        let mut row = vec![bill.to_owned(), line.to_owned(), kind.to_owned()];
        row.extend(tags);
        row.extend([self.format(minor), note.to_owned()]);
        self.rows.push(row);
    }

    /// The number of rung and column cells in a row.
    fn tag_cells(&self) -> usize {
        self.sheet
            .rungs
            .len()
            .saturating_add(self.sheet.columns.len())
    }

    fn summary(&mut self, bill: &str, line: &str, minor: i64) {
        self.row(
            bill,
            line,
            "",
            vec![String::new(); self.tag_cells()],
            minor,
            "",
        );
    }

    fn block(&mut self, block: &Block) -> Result<(), MoneyError> {
        let bill = &block.bill;
        let mut amounts: Vec<Money> = block.lines.iter().map(|line| line.amount).collect();
        amounts.push(bill.residual);
        let shown = Money::round_all_to_minor_units(&amounts, self.sheet.currency)?;
        for (line, minor) in block.lines.iter().zip(&shown) {
            let tags = line
                .rungs
                .iter()
                .chain(&line.columns)
                .map(|value| value.as_ref().map(ToString::to_string).unwrap_or_default())
                .collect();
            let note = match line.figure {
                Figure::UnknownModel => "unknown model",
                Figure::Estimate => "estimate",
                Figure::Billed | Figure::ListPrice => "",
            };
            self.row(
                &block.bill_id,
                "cost",
                line.cost_kind.label(),
                tags,
                *minor,
                note,
            );
        }
        let block_sum = shown
            .iter()
            .try_fold(0_i64, |sum, minor| sum.checked_add(*minor))
            .ok_or(MoneyError::Overflow)?;
        let residual = shown.last().copied().unwrap_or_default();
        let discount = bill.discount.round_to_minor_units();
        let credit = bill.credit.round_to_minor_units();
        let net = block_sum
            .checked_sub(discount)
            .and_then(|net| net.checked_sub(credit))
            .ok_or(MoneyError::Overflow)?;
        self.summary(&block.bill_id, "residual", residual);
        self.summary(&block.bill_id, "gross", block_sum);
        self.summary(&block.bill_id, "discount", discount);
        self.summary(&block.bill_id, "credit", credit);
        self.summary(&block.bill_id, "net", net);
        self.add_to_total(net)
    }

    fn adjustments(&mut self) -> Result<(), MoneyError> {
        let sheet = self.sheet;
        let amounts: Vec<Money> = sheet
            .adjustments
            .iter()
            .map(|line| line.adjustment.amount)
            .collect();
        let shown = Money::round_all_to_minor_units(&amounts, sheet.currency)?;
        for (
            AdjustmentLine {
                adjustment,
                cost_kind,
            },
            minor,
        ) in sheet.adjustments.iter().zip(&shown)
        {
            let line = if adjustment.late {
                "late adjustment"
            } else {
                "adjustment"
            };
            let tags = sheet
                .rungs
                .iter()
                .chain(&sheet.columns)
                .map(|name| {
                    adjustment
                        .tags
                        .get(name)
                        .map(ToString::to_string)
                        .unwrap_or_default()
                })
                .collect();
            self.row(
                "",
                line,
                cost_kind.label(),
                tags,
                *minor,
                &adjustment.reason,
            );
            self.add_to_total(*minor)?;
        }
        Ok(())
    }
}

/// Quotes a CSV cell when it holds a comma, a quote or a line break.
fn csv_cell(cell: &str) -> String {
    if cell.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_owned()
    }
}

/// The key a split's cost is grouped by.
type LineKey = (CostKind, Vec<Option<TagValue>>, Vec<Option<TagValue>>);

fn build_block(
    request: &CostSheetRequest<'_>,
    block: &BlockRequest<'_>,
    rungs: &[TagName],
    columns: &[TagName],
) -> Result<Block, CostSheetError> {
    let currency = request.reporting_currency;
    let bill = block.bill;
    if bill.period != request.period {
        return Err(CostSheetError::BillForOtherPeriod {
            bill: bill.id.clone(),
            bill_period: bill.period,
            period: request.period,
        });
    }
    // (amount in the bill's currency, any unknown model)
    let mut lines: BTreeMap<LineKey, (Money, bool)> = BTreeMap::new();
    for split in &block.splits {
        let key: LineKey = (
            request.tag_map.cost_kind(&split.tags),
            rungs
                .iter()
                .map(|rung| split.tags.get(rung).cloned())
                .collect(),
            columns
                .iter()
                .map(|column| split.tags.get(column).cloned())
                .collect(),
        );
        let line = lines
            .entry(key)
            .or_insert((Money::zero(bill.currency()), false));
        line.0 = line.0.checked_add(split.value.list)?;
        line.1 |= !split.value.is_complete();
    }
    let reconciliation = bill.reconcile(lines.values().map(|(amount, _)| *amount))?;
    let rate = bill.rate_to(currency)?;

    let mut amounts: Vec<Money> = lines.values().map(|(amount, _)| *amount).collect();
    amounts.push(reconciliation.residual);
    let converted = rate.convert_all(&amounts)?;
    let lines: Vec<Line> = lines
        .into_iter()
        .zip(&converted)
        .map(|((key, (_, unknown)), amount)| Line {
            cost_kind: key.0,
            rungs: key.1,
            columns: key.2,
            amount: *amount,
            figure: if unknown {
                Figure::UnknownModel
            } else {
                Figure::ListPrice
            },
        })
        .collect();
    let gross = Money::sum(converted.iter().copied(), currency)?;
    let discount = rate.convert(bill.discount)?;
    let credit = rate.convert(bill.credit)?;
    Ok(Block {
        bill_id: bill.id.clone(),
        lines,
        bill: BillTotals {
            revision: bill.revision,
            residual: converted.last().copied().unwrap_or(Money::zero(currency)),
            gross,
            discount,
            credit,
            net: gross.checked_sub(discount)?.checked_sub(credit)?,
            reconciliation,
            rate,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ModelRef;
    use crate::placement::PlacedBy;
    use crate::price::Valuation;
    use crate::status::CostSource;
    use time::macros::datetime;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn split(pairs: &[&str], amount: &str) -> Split {
        Split {
            tags: tags(pairs),
            value: Valuation {
                list: usd(amount),
                billed: None,
                unknown_models: std::collections::BTreeSet::new(),
            },
            placed_by: PlacedBy::Nothing,
            ticket_not_synced: false,
        }
    }

    fn october() -> Period {
        Period::parse("2026-10").unwrap()
    }

    fn bill(gross: &str, discount: &str, credit: &str, net: &str) -> Bill {
        Bill {
            id: "anthropic-2026-10".to_owned(),
            provider: "anthropic".to_owned(),
            cost_source: CostSource::UsageAtApiRates,
            period: october(),
            gross: usd(gross),
            discount: usd(discount),
            credit: usd(credit),
            net: usd(net),
            exchange_rate: None,
            revision: 1,
        }
    }

    fn splits() -> Vec<Split> {
        vec![
            split(
                &[
                    "person=mara",
                    "team=web",
                    "ticket=KD-214",
                    "project=storefront",
                ],
                "41.87",
            ),
            split(
                &[
                    "person=mara",
                    "team=web",
                    "ticket=KD-214",
                    "project=storefront",
                ],
                "12.10",
            ),
            split(&["person=lena", "team=data", "project=storefront"], "9.75"),
            split(&["person=mara", "team=web"], "60.38"),
        ]
    }

    fn request<'a>(
        tag_map: &'a TagMap,
        blocks: Vec<BlockRequest<'a>>,
        adjustments: &'a [Adjustment],
        columns: &'a [TagName],
    ) -> CostSheetRequest<'a> {
        CostSheetRequest {
            period: october(),
            reporting_currency: Currency::USD,
            tag_map,
            columns,
            blocks,
            adjustments,
        }
    }

    #[test]
    fn a_bill_block_has_its_lines_residual_and_bill() {
        let bill = bill("150", "10", "5", "135");
        let splits = splits();
        let tag_map = TagMap::default();
        let columns = [TagName::new("team").unwrap(), TagName::project()];
        let sheet = CostSheet::build(&request(
            &tag_map,
            vec![BlockRequest {
                bill: &bill,
                splits: splits.iter().collect(),
            }],
            &[],
            &columns,
        ))
        .unwrap();
        assert_eq!(sheet.columns, vec![TagName::new("team").unwrap()]);
        let block = sheet.blocks.first().unwrap();
        assert_eq!(block.lines.len(), 3);
        let totals = &block.bill;
        assert_eq!(totals.reconciliation.explained, usd("124.10"));
        assert_eq!(totals.residual, usd("25.90"));
        assert_eq!(totals.gross, usd("150"));
        assert_eq!(totals.net, usd("135"));
        assert_eq!(sheet.total, usd("135"));
        assert_eq!(
            sheet.to_csv().unwrap(),
            "bill,line,cost_kind,project,ticket,team,amount,note\n\
             anthropic-2026-10,cost,direct,storefront,KD-214,web,53.97,\n\
             anthropic-2026-10,cost,project overhead,storefront,,data,9.75,\n\
             anthropic-2026-10,cost,organization overhead,,,web,60.38,\n\
             anthropic-2026-10,residual,,,,,25.90,\n\
             anthropic-2026-10,gross,,,,,150.00,\n\
             anthropic-2026-10,discount,,,,,10.00,\n\
             anthropic-2026-10,credit,,,,,5.00,\n\
             anthropic-2026-10,net,,,,,135.00,\n\
             ,total,,,,,135.00,\n"
        );
    }

    #[test]
    fn a_bill_in_another_currency_is_converted_and_still_adds_up() {
        let mut bill = bill("100", "0", "0", "100");
        bill.exchange_rate =
            Some(ExchangeRate::new(Currency::USD, Currency::EUR, "0.333333333").unwrap());
        let thirds = [
            split(&["person=a"], "33.333333333"),
            split(&["person=b", "project=x"], "33.333333333"),
            split(&["person=c", "ticket=X-1"], "33.333333334"),
        ];
        let tag_map = TagMap::default();
        let mut request = request(
            &tag_map,
            vec![BlockRequest {
                bill: &bill,
                splits: thirds.iter().collect(),
            }],
            &[],
            &[],
        );
        request.reporting_currency = Currency::EUR;
        let sheet = CostSheet::build(&request).unwrap();
        let block = sheet.blocks.first().unwrap();
        let totals = &block.bill;
        assert_eq!(totals.residual, Money::zero(Currency::EUR));
        let lines = Money::sum(block.lines.iter().map(|line| line.amount), Currency::EUR).unwrap();
        assert_eq!(lines, totals.gross);
        assert_eq!(
            totals.gross,
            Money::parse("33.3333333", Currency::EUR).unwrap()
        );
        let csv = sheet.to_csv().unwrap();
        assert!(csv.contains(",gross,,,,33.33,\n"), "{csv}");
        let shown: Vec<&str> = csv
            .lines()
            .filter(|row| row.contains(",cost,"))
            .map(|row| row.split(',').nth(5).unwrap())
            .collect();
        assert_eq!(shown, vec!["11.11", "11.11", "11.11"]);

        request.reporting_currency = Currency::new("GBP").unwrap();
        assert!(matches!(
            CostSheet::build(&request),
            Err(CostSheetError::Bill(BillError::Money(_)))
        ));
    }

    #[test]
    fn unknown_models_are_noted_on_their_line() {
        let mut unknown = split(&["person=mara", "ticket=KD-1"], "1");
        unknown
            .value
            .unknown_models
            .insert(ModelRef::new("acme", "mystery"));
        let known = split(&["person=mara"], "2");
        let bill = bill("5", "0", "0", "5");
        let tag_map = TagMap::default();
        let sheet = CostSheet::build(&request(
            &tag_map,
            vec![BlockRequest {
                bill: &bill,
                splits: vec![&unknown, &known],
            }],
            &[],
            &[],
        ))
        .unwrap();
        let figures: Vec<Figure> = sheet
            .blocks
            .first()
            .unwrap()
            .lines
            .iter()
            .map(|line| line.figure)
            .collect();
        assert_eq!(figures, vec![Figure::UnknownModel, Figure::ListPrice]);
        assert!(sheet.to_csv().unwrap().contains(",1.00,unknown model\n"));
    }

    #[test]
    fn adjustments_are_their_own_lines_and_count_in_the_total() {
        let bill = bill("100", "0", "0", "100");
        let splits = splits();
        let adjustments = vec![
            Adjustment {
                period: october(),
                tags: tags(&["ticket=KD-214", "project=storefront"]),
                amount: usd("-4.5"),
                reason: "Wrong model, says Mara".to_owned(),
                posted_by: TagValue::new("priya").unwrap(),
                posted_at: datetime!(2026-11-12 09:00 UTC),
                late: false,
            },
            Adjustment {
                period: october(),
                tags: tags(&["person=jonas"]),
                amount: usd("7.25"),
                reason: "Token sheet submitted after the close".to_owned(),
                posted_by: TagValue::new("jonas").unwrap(),
                posted_at: datetime!(2026-11-13 09:00 UTC),
                late: true,
            },
        ];
        let tag_map = TagMap::default();
        let sheet = CostSheet::build(&request(
            &tag_map,
            vec![BlockRequest {
                bill: &bill,
                splits: splits.iter().collect(),
            }],
            &adjustments,
            &[],
        ))
        .unwrap();
        assert_eq!(sheet.total, usd("102.75"));
        let csv = sheet.to_csv().unwrap();
        assert!(
            csv.contains(
                "\n,adjustment,direct,storefront,KD-214,-4.50,\"Wrong model, says Mara\"\n"
            ),
            "{csv}"
        );
        assert!(csv.contains("\n,late adjustment,organization overhead,,,7.25,Token sheet submitted after the close\n"), "{csv}");
        assert!(csv.ends_with(",total,,,,102.75,\n"), "{csv}");

        let mut other = adjustments;
        if let Some(first) = other.first_mut() {
            first.period = Period::parse("2026-09").unwrap();
        }
        assert!(matches!(
            CostSheet::build(&request(&tag_map, Vec::new(), &other, &[])),
            Err(CostSheetError::AdjustmentForOtherPeriod { .. })
        ));
    }

    #[test]
    fn a_bill_for_another_period_is_refused() {
        let mut bill = bill("1", "0", "0", "1");
        bill.period = Period::parse("2026-09").unwrap();
        let tag_map = TagMap::default();
        assert!(matches!(
            CostSheet::build(&request(
                &tag_map,
                vec![BlockRequest {
                    bill: &bill,
                    splits: Vec::new(),
                }],
                &[],
                &[],
            )),
            Err(CostSheetError::BillForOtherPeriod { .. })
        ));
    }

    #[test]
    fn shown_cents_add_up_to_the_shown_gross() {
        let bill = bill("0.03", "0", "0", "0.03");
        let thirds = [
            split(&["person=a"], "0.006666667"),
            split(&["person=b"], "0.006666667"),
            split(&["person=c"], "0.006666666"),
        ];
        let tag_map = TagMap::new(TagName::person(), Vec::new()).unwrap();
        let columns = [TagName::person()];
        let sheet = CostSheet::build(&request(
            &tag_map,
            vec![BlockRequest {
                bill: &bill,
                splits: thirds.iter().collect(),
            }],
            &[],
            &columns,
        ))
        .unwrap();
        assert_eq!(
            sheet.to_csv().unwrap(),
            "bill,line,cost_kind,person,amount,note\n\
             anthropic-2026-10,cost,organization overhead,a,0.01,\n\
             anthropic-2026-10,cost,organization overhead,b,0.00,\n\
             anthropic-2026-10,cost,organization overhead,c,0.01,\n\
             anthropic-2026-10,residual,,,0.01,\n\
             anthropic-2026-10,gross,,,0.03,\n\
             anthropic-2026-10,discount,,,0.00,\n\
             anthropic-2026-10,credit,,,0.00,\n\
             anthropic-2026-10,net,,,0.03,\n\
             ,total,,,0.03,\n"
        );
    }

    #[test]
    fn cells_with_commas_quotes_or_breaks_are_quoted() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_cell("cr\r"), "\"cr\r\"");
    }
}
