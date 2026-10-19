//! The price table (source record 6) and the cost of one message
//! (formula 1).
//!
//! `C(e) = Σ_kind tokens_kind(e) × p(provider, model, kind, as of t(e))`.
//! A message with no counts has no cost and shows as a count error, never
//! as zero. A model missing from the table is labelled unknown, never zero.
//! The hosted service pulls one managed table in as events; an
//! organization adds or overrides rows, and the override wins.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::message::{Message, ModelRef};
use crate::money::{Currency, Money, MoneyError};
use crate::tokens::{TokenCounts, TokenKind};

/// Tokens a price is quoted for.
const TOKENS_PER_PRICE: i128 = 1_000_000;

/// Who put a row in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceSource {
    /// The managed table the hosted service pulls in, or the file a
    /// self-hosted service brings.
    Managed,
    /// A row the organization added or overrode. It wins over a managed
    /// row.
    Organization,
}

/// One row of the price table: the price per million tokens of one kind of
/// one model from a date on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceRow {
    pub provider: String,
    pub model: String,
    pub kind: TokenKind,
    pub per_million: Money,
    pub valid_from: Date,
    #[serde(default = "managed")]
    pub source: PriceSource,
}

const fn managed() -> PriceSource {
    PriceSource::Managed
}

impl PriceRow {
    /// The model the row prices.
    #[must_use]
    pub fn model_ref(&self) -> ModelRef {
        ModelRef::new(&self.provider, &self.model)
    }
}

/// Why a message has no cost figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostLabel {
    /// Priced from the table: the figure stands.
    Priced,
    /// The message has no token counts. No cost, never zero.
    CountError,
    /// The table has no price for the model, or for a kind the message used,
    /// as of the message's time. No cost, never zero.
    UnknownModel,
}

/// What one message cost, or why it has no figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCost {
    /// The cost, where the label is `Priced`.
    pub amount: Option<Money>,
    pub label: CostLabel,
}

impl MessageCost {
    /// The amount to add up: the cost, or zero in `currency` where the
    /// message has no figure. The label says which.
    #[must_use]
    pub fn amount_or_zero(self, currency: Currency) -> Money {
        self.amount.unwrap_or(Money::zero(currency))
    }
}

/// The price table in force: every live row, in one currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceTable {
    currency: Currency,
    rows: Vec<PriceRow>,
}

impl PriceTable {
    /// An empty table in a currency. The open edition ships without prices.
    #[must_use]
    pub const fn new(currency: Currency) -> Self {
        Self {
            currency,
            rows: Vec::new(),
        }
    }

    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.currency
    }

    /// Adds a row.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the price is not in the table's
    /// currency.
    pub fn insert(&mut self, row: PriceRow) -> Result<(), MoneyError> {
        if row.per_million.currency() != self.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: row.per_million.currency(),
            });
        }
        self.rows.push(row);
        Ok(())
    }

    /// Every row, in the order added.
    pub fn rows(&self) -> impl Iterator<Item = &PriceRow> {
        self.rows.iter()
    }

    /// Whether the table has any row for a model.
    #[must_use]
    pub fn knows(&self, model: &ModelRef) -> bool {
        self.rows
            .iter()
            .any(|row| row.provider == model.provider && row.model == model.model)
    }

    /// `p(provider, model, kind, as of date)`: the organization's row if it
    /// has one valid on the date, else the managed row; among those, the one
    /// valid from the latest date. `None` if no row is valid on the date.
    #[must_use]
    pub fn price(&self, model: &ModelRef, kind: TokenKind, on: Date) -> Option<Money> {
        self.rows
            .iter()
            .filter(|row| {
                row.provider == model.provider
                    && row.model == model.model
                    && row.kind == kind
                    && row.valid_from <= on
            })
            .max_by_key(|row| (row.source, row.valid_from))
            .map(|row| row.per_million)
    }

    /// The prices of every kind of a model on a date, by kind.
    #[must_use]
    pub fn prices(&self, model: &ModelRef, on: Date) -> BTreeMap<TokenKind, Money> {
        TokenKind::ALL
            .iter()
            .filter_map(|kind| self.price(model, *kind, on).map(|price| (*kind, price)))
            .collect()
    }

    /// `C(e)`: what a message cost at the prices in force at its time, or
    /// why it has no figure.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn cost(&self, message: &Message) -> Result<MessageCost, MoneyError> {
        let Some(tokens) = message.tokens else {
            return Ok(MessageCost {
                amount: None,
                label: CostLabel::CountError,
            });
        };
        let on = date_of(message.at);
        let prices = self.prices(&message.model, on);
        let needs_price = |kind: &TokenKind| tokens.get(*kind) > 0;
        if prices.is_empty()
            || TokenKind::ALL
                .iter()
                .filter(|kind| needs_price(kind))
                .any(|kind| !prices.contains_key(kind))
        {
            return Ok(MessageCost {
                amount: None,
                label: CostLabel::UnknownModel,
            });
        }
        Ok(MessageCost {
            amount: Some(cost_at(&prices, tokens, self.currency)?),
            label: CostLabel::Priced,
        })
    }
}

/// The date of a moment in UTC.
#[must_use]
pub fn date_of(moment: OffsetDateTime) -> Date {
    moment.to_offset(time::UtcOffset::UTC).date()
}

/// Tokens times prices per million, summed exactly and rounded half away
/// from zero to the nano-unit once for the whole message. A kind without a
/// price counts only when it has no tokens.
fn cost_at(
    prices: &BTreeMap<TokenKind, Money>,
    tokens: TokenCounts,
    currency: Currency,
) -> Result<Money, MoneyError> {
    let mut scaled = 0_i128;
    for kind in TokenKind::ALL {
        let Some(price) = prices.get(&kind) else {
            continue;
        };
        let part = i128::from(tokens.get(kind))
            .checked_mul(i128::from(price.nanos()))
            .ok_or(MoneyError::Overflow)?;
        scaled = scaled.checked_add(part).ok_or(MoneyError::Overflow)?;
    }
    let whole = scaled
        .checked_div(TOKENS_PER_PRICE)
        .ok_or(MoneyError::Overflow)?;
    let rest = scaled
        .checked_rem(TOKENS_PER_PRICE)
        .ok_or(MoneyError::Overflow)?;
    let rounded = if rest
        .unsigned_abs()
        .checked_mul(2)
        .ok_or(MoneyError::Overflow)?
        < TOKENS_PER_PRICE.unsigned_abs()
    {
        whole
    } else if scaled.is_negative() {
        whole.checked_sub(1).ok_or(MoneyError::Overflow)?
    } else {
        whole.checked_add(1).ok_or(MoneyError::Overflow)?
    };
    i64::try_from(rounded)
        .map(|nanos| Money::from_nanos(nanos, currency))
        .map_err(|_| MoneyError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, SessionFacts, SessionId, Signals};
    use crate::tags::{TagValue, Tags};
    use time::macros::{date, datetime};

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn sonnet() -> ModelRef {
        ModelRef::new("anthropic", "claude-sonnet-4-5")
    }

    fn row(kind: TokenKind, price: &str, from: Date, source: PriceSource) -> PriceRow {
        PriceRow {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-5".to_owned(),
            kind,
            per_million: usd(price),
            valid_from: from,
            source,
        }
    }

    fn table() -> PriceTable {
        let mut table = PriceTable::new(Currency::USD);
        let from = date!(2026 - 01 - 01);
        for (kind, price) in [
            (TokenKind::Input, "3"),
            (TokenKind::Output, "15"),
            (TokenKind::CacheWrite, "3.75"),
            (TokenKind::CacheRead, "0.30"),
        ] {
            table
                .insert(row(kind, price, from, PriceSource::Managed))
                .unwrap();
        }
        table
    }

    fn message(tokens: Option<TokenCounts>, at: OffsetDateTime) -> Message {
        Message {
            session_id: SessionId::new("s1").unwrap(),
            message_id: MessageId::new("m1").unwrap(),
            person: TagValue::new("mara").unwrap(),
            model: sonnet(),
            tokens,
            at,
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            reported_cost: None,
            tags: Tags::new(),
            signals: Signals::default(),
            session: SessionFacts::default(),
        }
    }

    const MARCH: OffsetDateTime = datetime!(2026-03-10 12:00 UTC);

    #[test]
    fn cost_is_tokens_times_the_price_per_million() {
        let tokens = TokenCounts {
            input: 1_000_000,
            output: 100_000,
            cache_write: 10_000,
            cache_read: 1_000,
        };
        // 3 + 1.5 + 0.0375 + 0.0003
        let cost = table().cost(&message(Some(tokens), MARCH)).unwrap();
        assert_eq!(cost.amount, Some(usd("4.5378")));
        assert_eq!(cost.label, CostLabel::Priced);
        assert_eq!(cost.amount_or_zero(Currency::USD), usd("4.5378"));
    }

    #[test]
    fn a_message_without_counts_is_a_count_error_not_zero() {
        let cost = table().cost(&message(None, MARCH)).unwrap();
        assert_eq!(cost.amount, None);
        assert_eq!(cost.label, CostLabel::CountError);
        assert_eq!(cost.amount_or_zero(Currency::USD), usd("0"));
        // Zero counts are counts: the cost is zero and priced.
        let zero = table()
            .cost(&message(Some(TokenCounts::default()), MARCH))
            .unwrap();
        assert_eq!(zero.amount, Some(usd("0")));
        assert_eq!(zero.label, CostLabel::Priced);
    }

    #[test]
    fn an_unknown_model_is_labelled_not_zero() {
        let mut unknown = message(
            Some(TokenCounts {
                input: 1_000,
                ..TokenCounts::default()
            }),
            MARCH,
        );
        unknown.model = ModelRef::new("acme", "mystery-1");
        let cost = table().cost(&unknown).unwrap();
        assert_eq!(cost.amount, None);
        assert_eq!(cost.label, CostLabel::UnknownModel);
        assert!(!table().knows(&unknown.model));
        assert!(table().knows(&sonnet()));
    }

    #[test]
    fn a_kind_without_a_price_is_unknown_only_when_it_has_tokens() {
        let mut table = PriceTable::new(Currency::USD);
        table
            .insert(row(
                TokenKind::Input,
                "3",
                date!(2026 - 01 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        let input_only = message(
            Some(TokenCounts {
                input: 1_000_000,
                ..TokenCounts::default()
            }),
            MARCH,
        );
        assert_eq!(table.cost(&input_only).unwrap().amount, Some(usd("3")));
        let with_output = message(
            Some(TokenCounts {
                input: 1_000_000,
                output: 1,
                ..TokenCounts::default()
            }),
            MARCH,
        );
        assert_eq!(
            table.cost(&with_output).unwrap().label,
            CostLabel::UnknownModel
        );
    }

    #[test]
    fn the_price_as_of_the_message_time_is_the_latest_valid_row() {
        let mut table = table();
        table
            .insert(row(
                TokenKind::Input,
                "6",
                date!(2026 - 03 - 15),
                PriceSource::Managed,
            ))
            .unwrap();
        let on_march_10 = date!(2026 - 03 - 10);
        assert_eq!(
            table.price(&sonnet(), TokenKind::Input, on_march_10),
            Some(usd("3"))
        );
        assert_eq!(
            table.price(&sonnet(), TokenKind::Input, date!(2026 - 03 - 15)),
            Some(usd("6"))
        );
        assert_eq!(
            table.price(&sonnet(), TokenKind::Input, date!(2025 - 12 - 31)),
            None
        );
        let tokens = TokenCounts {
            input: 1_000_000,
            ..TokenCounts::default()
        };
        let before = table
            .cost(&message(Some(tokens), datetime!(2026-03-14 23:59 UTC)))
            .unwrap();
        assert_eq!(before.amount, Some(usd("3")));
        let after = table
            .cost(&message(Some(tokens), datetime!(2026-03-15 00:00 UTC)))
            .unwrap();
        assert_eq!(after.amount, Some(usd("6")));
        let too_early = table
            .cost(&message(Some(tokens), datetime!(2025-12-31 23:59 UTC)))
            .unwrap();
        assert_eq!(too_early.label, CostLabel::UnknownModel);
    }

    #[test]
    fn the_organizations_override_wins_over_the_managed_row() {
        let mut table = table();
        table
            .insert(row(
                TokenKind::Input,
                "2",
                date!(2026 - 02 - 01),
                PriceSource::Organization,
            ))
            .unwrap();
        // A newer managed row does not beat an older override.
        table
            .insert(row(
                TokenKind::Input,
                "9",
                date!(2026 - 03 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        assert_eq!(
            table.price(&sonnet(), TokenKind::Input, date!(2026 - 03 - 10)),
            Some(usd("2"))
        );
        // Before the override was valid, the managed row stands.
        assert_eq!(
            table.price(&sonnet(), TokenKind::Input, date!(2026 - 01 - 15)),
            Some(usd("3"))
        );
        assert_eq!(table.prices(&sonnet(), date!(2026 - 03 - 10)).len(), 4);
        assert_eq!(table.rows().count(), 6);
    }

    #[test]
    fn a_fraction_of_a_nano_unit_rounds_half_away_from_zero() {
        let prices: BTreeMap<TokenKind, Money> = [
            (TokenKind::Input, Money::from_nanos(1, Currency::USD)),
            (TokenKind::Output, Money::from_nanos(-1, Currency::USD)),
        ]
        .into_iter()
        .collect();
        let count = |input, output| TokenCounts {
            input,
            output,
            ..TokenCounts::default()
        };
        assert_eq!(
            cost_at(&prices, count(500_000, 0), Currency::USD)
                .unwrap()
                .nanos(),
            1
        );
        assert_eq!(
            cost_at(&prices, count(499_999, 0), Currency::USD)
                .unwrap()
                .nanos(),
            0
        );
        assert_eq!(
            cost_at(&prices, count(0, 500_000), Currency::USD)
                .unwrap()
                .nanos(),
            -1
        );
        assert_eq!(
            cost_at(&prices, count(0, 499_999), Currency::USD)
                .unwrap()
                .nanos(),
            0
        );
        let huge: BTreeMap<TokenKind, Money> =
            [(TokenKind::Input, Money::from_nanos(i64::MAX, Currency::USD))]
                .into_iter()
                .collect();
        assert_eq!(
            cost_at(&huge, count(u64::MAX, 0), Currency::USD),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn prices_by_kind_list_only_the_kinds_with_a_row() {
        let mut table = PriceTable::new(Currency::USD);
        assert!(table.prices(&sonnet(), date!(2026 - 03 - 10)).is_empty());
        table
            .insert(row(
                TokenKind::Output,
                "15",
                date!(2026 - 01 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        let prices = table.prices(&sonnet(), date!(2026 - 03 - 10));
        assert_eq!(prices.len(), 1);
        assert_eq!(prices.get(&TokenKind::Output), Some(&usd("15")));
        assert_eq!(prices.get(&TokenKind::Input), None);
        assert!(table.prices(&sonnet(), date!(2025 - 12 - 31)).is_empty());
    }

    #[test]
    fn rows_must_be_in_the_tables_currency() {
        let mut table = PriceTable::new(Currency::USD);
        let mut euro = row(
            TokenKind::Input,
            "3",
            date!(2026 - 01 - 01),
            PriceSource::Managed,
        );
        euro.per_million = Money::zero(Currency::EUR);
        assert!(matches!(
            table.insert(euro),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        assert_eq!(table.currency(), Currency::USD);
        assert_eq!(table.rows().count(), 0);
    }

    #[test]
    fn rows_read_from_json_as_managed_by_default() {
        let json = r#"{"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "input",
            "per_million": {"nanos": 3000000000, "currency": "USD"}, "valid_from": "2026-01-01"}"#;
        let row: PriceRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.source, PriceSource::Managed);
        assert_eq!(row.model_ref(), sonnet());
        assert_eq!(row.valid_from, date!(2026 - 01 - 01));
        let json = serde_json::to_string(&table()).unwrap();
        assert_eq!(serde_json::from_str::<PriceTable>(&json).unwrap(), table());
        assert_eq!(
            date_of(datetime!(2026-04-01 01:00 +02:00)),
            date!(2026 - 03 - 31)
        );
    }
}
