//! The price table (source record 6) and the cost of one message
//! (formula 1).
//!
//! `C(e) = Σ over kinds k with a price: tokens_k(e) × p(c(e), provider(e),
//! model(e), k, as of t(e))`. The contract's own row wins over the
//! organization's, which wins over the managed table. A message with no
//! provider is priced when every provider in the table charges the same for
//! its model. A message with no counts is a count error; one the table
//! cannot price is "not priced yet". Neither is ever a silent zero. A kind
//! with tokens and no price is listed and leaves the rest priced.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::message::{Message, ModelRef};
use crate::money::{Currency, Money, MoneyError};
use crate::tags::TagValue;
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
/// one model from a date on, optionally only under one contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceRow {
    pub provider: String,
    pub model: String,
    pub kind: TokenKind,
    pub per_million: Money,
    pub valid_from: Date,
    #[serde(default = "managed")]
    pub source: PriceSource,
    /// The contract whose own price this is. It wins over every row
    /// without a contract, for messages under that contract only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<TagValue>,
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
    /// The table cannot price the message yet: it has no row for the model
    /// under the message's provider, or the provider is unknown and the
    /// providers in the table charge differently. Its own line on every
    /// report with its token counts, never zero.
    NotPricedYet,
}

impl CostLabel {
    /// The label as reports write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Priced => "priced",
            Self::CountError => "count error",
            Self::NotPricedYet => "not priced yet",
        }
    }
}

/// What one message cost, or why it has no figure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCost {
    /// The cost, where the label is `Priced`.
    pub amount: Option<Money>,
    pub label: CostLabel,
    /// Kinds with tokens and no price, left out of the amount.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unpriced_kinds: BTreeSet<TokenKind>,
}

impl MessageCost {
    const fn without(label: CostLabel) -> Self {
        Self {
            amount: None,
            label,
            unpriced_kinds: BTreeSet::new(),
        }
    }

    /// The amount to add up: the cost, or zero in `currency` where the
    /// message has no figure. The label says which.
    #[must_use]
    pub fn amount_or_zero(&self, currency: Currency) -> Money {
        self.amount.unwrap_or_else(|| Money::zero(currency))
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

    /// Whether the table has any row for a model of a provider valid on a
    /// date.
    #[must_use]
    pub fn knows(&self, provider: &str, model: &str, on: Date) -> bool {
        self.rows
            .iter()
            .any(|row| row.provider == provider && row.model == model && row.valid_from <= on)
    }

    /// The providers with a row for a model valid on a date, not counting
    /// the rows of a contract.
    #[must_use]
    pub fn providers_of(&self, model: &str, on: Date) -> BTreeSet<&str> {
        self.rows
            .iter()
            .filter(|row| row.model == model && row.valid_from <= on && row.contract.is_none())
            .map(|row| row.provider.as_str())
            .collect()
    }

    /// `p(c, provider, model, kind, as of date)`: among the rows valid on
    /// the date, the contract's own row if it has one, else the
    /// organization's, else the managed one; among those, the one valid
    /// from the latest date. A kind with no row of its own takes the price
    /// of the kind it is priced as ([`TokenKind::priced_as`]). `None` if no
    /// row is valid on the date.
    #[must_use]
    pub fn price(
        &self,
        provider: &str,
        model: &str,
        kind: &TokenKind,
        on: Date,
        contract: Option<&TagValue>,
    ) -> Option<Money> {
        self.own_price(provider, model, kind, on, contract)
            .or_else(|| {
                kind.priced_as()
                    .and_then(|alias| self.own_price(provider, model, &alias, on, contract))
            })
    }

    fn own_price(
        &self,
        provider: &str,
        model: &str,
        kind: &TokenKind,
        on: Date,
        contract: Option<&TagValue>,
    ) -> Option<Money> {
        self.rows
            .iter()
            .filter(|row| {
                row.provider == provider
                    && row.model == model
                    && row.kind == *kind
                    && row.valid_from <= on
                    && row
                        .contract
                        .as_ref()
                        .is_none_or(|own| Some(own) == contract)
            })
            .max_by_key(|row| (row.contract.is_some(), row.source, row.valid_from))
            .map(|row| row.per_million)
    }

    /// `C(e)` of a message as it was sent, under no contract.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn cost(&self, message: &Message) -> Result<MessageCost, MoneyError> {
        self.cost_of(
            message.tokens.as_ref(),
            message.model.provider.as_deref(),
            &message.model.model,
            message.at,
            None,
        )
    }

    /// `C(e)`: what counts of a model cost at the prices in force at a
    /// moment, under a provider and a contract when known, or why they have
    /// no figure.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn cost_of(
        &self,
        tokens: Option<&TokenCounts>,
        provider: Option<&str>,
        model: &str,
        at: OffsetDateTime,
        contract: Option<&TagValue>,
    ) -> Result<MessageCost, MoneyError> {
        let Some(tokens) = tokens else {
            return Ok(MessageCost::without(CostLabel::CountError));
        };
        let on = date_of(at);
        let providers: Vec<&str> = match provider {
            Some(provider) if self.knows(provider, model, on) => vec![provider],
            Some(_) => Vec::new(),
            None => self.providers_of(model, on).into_iter().collect(),
        };
        if providers.is_empty() {
            return Ok(MessageCost::without(CostLabel::NotPricedYet));
        }
        let mut scaled = 0_i128;
        let mut priced_any = false;
        let mut unpriced_kinds = BTreeSet::new();
        for (kind, count) in tokens.summed() {
            let mut prices = providers
                .iter()
                .map(|provider| self.price(provider, model, kind, on, contract));
            let Some(price) = prices.next() else {
                return Ok(MessageCost::without(CostLabel::NotPricedYet));
            };
            if prices.any(|other| other != price) {
                return Ok(MessageCost::without(CostLabel::NotPricedYet));
            }
            let Some(price) = price else {
                unpriced_kinds.insert(kind.clone());
                continue;
            };
            priced_any = true;
            let part = i128::from(count)
                .checked_mul(i128::from(price.nanos()))
                .ok_or(MoneyError::Overflow)?;
            scaled = scaled.checked_add(part).ok_or(MoneyError::Overflow)?;
        }
        if !priced_any && !unpriced_kinds.is_empty() {
            return Ok(MessageCost::without(CostLabel::NotPricedYet));
        }
        Ok(MessageCost {
            amount: Some(per_million(scaled, self.currency)?),
            label: CostLabel::Priced,
            unpriced_kinds,
        })
    }

    /// The prices of every kind a model has a row for on a date, under a
    /// provider, by kind.
    #[must_use]
    pub fn prices(&self, model: &ModelRef, on: Date) -> BTreeMap<TokenKind, Money> {
        let Some(provider) = model.provider.as_deref() else {
            return BTreeMap::new();
        };
        let kinds: BTreeSet<&TokenKind> = self
            .rows
            .iter()
            .filter(|row| row.provider == provider && row.model == model.model)
            .map(|row| &row.kind)
            .collect();
        kinds
            .into_iter()
            .filter_map(|kind| {
                self.price(provider, &model.model, kind, on, None)
                    .map(|price| (kind.clone(), price))
            })
            .collect()
    }
}

/// The date of a moment in UTC.
#[must_use]
pub fn date_of(moment: OffsetDateTime) -> Date {
    moment.to_offset(time::UtcOffset::UTC).date()
}

/// A sum of tokens times prices per million, rounded half away from zero
/// to the nano-unit once for the whole message.
fn per_million(scaled: i128, currency: Currency) -> Result<Money, MoneyError> {
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
    use crate::tags::Tags;
    use time::macros::{date, datetime};

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    const SONNET: &str = "claude-sonnet-4-5";

    fn sonnet() -> ModelRef {
        ModelRef::new("anthropic", SONNET)
    }

    fn row(kind: TokenKind, price: &str, from: Date, source: PriceSource) -> PriceRow {
        PriceRow {
            provider: "anthropic".to_owned(),
            model: SONNET.to_owned(),
            kind,
            per_million: usd(price),
            valid_from: from,
            source,
            contract: None,
        }
    }

    fn table() -> PriceTable {
        let mut table = PriceTable::new(Currency::USD);
        let from = date!(2026 - 01 - 01);
        for (kind, price) in [
            (TokenKind::INPUT, "3"),
            (TokenKind::OUTPUT, "15"),
            (TokenKind::CACHE_WRITE, "3.75"),
            (TokenKind::CACHE_READ, "0.30"),
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

    fn input(count: u64) -> TokenCounts {
        TokenCounts::new().with(TokenKind::INPUT, count)
    }

    const MARCH: OffsetDateTime = datetime!(2026-03-10 12:00 UTC);
    const ON: Date = date!(2026 - 03 - 10);

    #[test]
    fn cost_is_tokens_times_the_price_per_million() {
        let tokens = TokenCounts::new()
            .with(TokenKind::INPUT, 1_000_000)
            .with(TokenKind::OUTPUT, 100_000)
            .with(TokenKind::CACHE_WRITE, 10_000)
            .with(TokenKind::CACHE_READ, 1_000);
        // 3 + 1.5 + 0.0375 + 0.0003
        let cost = table().cost(&message(Some(tokens), MARCH)).unwrap();
        assert_eq!(cost.amount, Some(usd("4.5378")));
        assert_eq!(cost.label, CostLabel::Priced);
        assert!(cost.unpriced_kinds.is_empty());
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
            .cost(&message(Some(TokenCounts::new()), MARCH))
            .unwrap();
        assert_eq!(zero.amount, Some(usd("0")));
        assert_eq!(zero.label, CostLabel::Priced);
    }

    #[test]
    fn a_model_the_table_does_not_have_is_not_priced_yet() {
        let mut unknown = message(Some(input(1_000)), MARCH);
        unknown.model = ModelRef::new("acme", "mystery-1");
        let cost = table().cost(&unknown).unwrap();
        assert_eq!(cost.amount, None);
        assert_eq!(cost.label, CostLabel::NotPricedYet);
        assert_eq!(cost.label.as_str(), "not priced yet");
        assert!(table().knows("anthropic", SONNET, ON));
        assert!(!table().knows("anthropic", SONNET, date!(2025 - 12 - 31)));
        assert!(!table().knows("anthropic", "mystery-1", ON));
        assert!(!table().knows("acme", SONNET, ON));
        // Zero counts of an unknown model are not priced either.
        unknown.tokens = Some(TokenCounts::new());
        assert_eq!(
            table().cost(&unknown).unwrap().label,
            CostLabel::NotPricedYet
        );
        assert_eq!(CostLabel::Priced.as_str(), "priced");
        assert_eq!(CostLabel::CountError.as_str(), "count error");
    }

    #[test]
    fn a_kind_without_a_price_is_listed_and_the_rest_is_priced() {
        let mut table = PriceTable::new(Currency::USD);
        table
            .insert(row(
                TokenKind::INPUT,
                "3",
                date!(2026 - 01 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        let input_only = message(Some(input(1_000_000)), MARCH);
        let cost = table.cost(&input_only).unwrap();
        assert_eq!(cost.amount, Some(usd("3")));
        assert!(cost.unpriced_kinds.is_empty());
        let reasoning = TokenKind::new("reasoning").unwrap();
        let with_more = message(
            Some(
                input(1_000_000)
                    .with(TokenKind::OUTPUT, 1)
                    .with(reasoning.clone(), 5),
            ),
            MARCH,
        );
        let cost = table.cost(&with_more).unwrap();
        assert_eq!(cost.label, CostLabel::Priced);
        assert_eq!(cost.amount, Some(usd("3")));
        assert_eq!(
            cost.unpriced_kinds,
            [TokenKind::OUTPUT, reasoning.clone()].into_iter().collect()
        );
        // Only unpriced kinds: nothing stands, so not priced yet, never zero.
        let only_unknown = message(Some(TokenCounts::new().with(reasoning, 5)), MARCH);
        let cost = table.cost(&only_unknown).unwrap();
        assert_eq!(cost.label, CostLabel::NotPricedYet);
        assert_eq!(cost.amount, None);
        assert!(cost.unpriced_kinds.is_empty());
    }

    #[test]
    fn a_plain_cache_write_is_priced_as_five_minutes_and_finer_counts_win() {
        let mut table = table();
        table
            .insert(row(
                TokenKind::CACHE_WRITE_1H,
                "6",
                date!(2026 - 01 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        // No cache_write_5m row: it takes the plain cache write's price.
        assert_eq!(
            table.price("anthropic", SONNET, &TokenKind::CACHE_WRITE_5M, ON, None),
            Some(usd("3.75"))
        );
        let plain = TokenCounts::new().with(TokenKind::CACHE_WRITE, 1_000_000);
        assert_eq!(
            table
                .cost(&message(Some(plain.clone()), MARCH))
                .unwrap()
                .amount,
            Some(usd("3.75"))
        );
        // The same call told finer: 600k at 3.75 and 400k at 6; the plain
        // count is not added again.
        let finer = plain
            .with(TokenKind::CACHE_WRITE_5M, 600_000)
            .with(TokenKind::CACHE_WRITE_1H, 400_000);
        assert_eq!(
            table.cost(&message(Some(finer), MARCH)).unwrap().amount,
            Some(usd("4.65"))
        );
        // A table that has only a 5-minute row prices a plain write with it.
        let mut five = PriceTable::new(Currency::USD);
        five.insert(row(
            TokenKind::CACHE_WRITE_5M,
            "4",
            date!(2026 - 01 - 01),
            PriceSource::Managed,
        ))
        .unwrap();
        assert_eq!(
            five.price("anthropic", SONNET, &TokenKind::CACHE_WRITE, ON, None),
            Some(usd("4"))
        );
        assert_eq!(
            five.price("anthropic", SONNET, &TokenKind::CACHE_WRITE_1H, ON, None),
            None
        );
    }

    #[test]
    fn without_a_provider_a_message_is_priced_only_when_every_provider_agrees() {
        let mut table = table();
        for (kind, price) in [(TokenKind::INPUT, "3"), (TokenKind::OUTPUT, "16")] {
            let mut bedrock = row(kind, price, date!(2026 - 01 - 01), PriceSource::Managed);
            bedrock.provider = "aws_bedrock".to_owned();
            table.insert(bedrock).unwrap();
        }
        assert_eq!(
            table
                .providers_of(SONNET, ON)
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["anthropic", "aws_bedrock"]
        );
        let mut unreported = message(Some(input(1_000_000)), MARCH);
        unreported.model = ModelRef::unreported(SONNET);
        // Both charge 3 for input.
        assert_eq!(table.cost(&unreported).unwrap().amount, Some(usd("3")));
        // They differ on output: 15 against 16.
        unreported.tokens = Some(input(1_000_000).with(TokenKind::OUTPUT, 1));
        assert_eq!(
            table.cost(&unreported).unwrap().label,
            CostLabel::NotPricedYet
        );
        // Bedrock has no cache read price, Anthropic has one: not the same.
        unreported.tokens = Some(TokenCounts::new().with(TokenKind::CACHE_READ, 1));
        assert_eq!(
            table.cost(&unreported).unwrap().label,
            CostLabel::NotPricedYet
        );
        // No provider in the table has the model.
        unreported.model = ModelRef::unreported("mystery-1");
        assert_eq!(
            table.cost(&unreported).unwrap().label,
            CostLabel::NotPricedYet
        );
        // A contract's own rows never make a provider count here.
        let mut contract_only = row(
            TokenKind::INPUT,
            "1",
            date!(2026 - 01 - 01),
            PriceSource::Organization,
        );
        contract_only.provider = "acme".to_owned();
        contract_only.contract = Some(TagValue::new("acme-deal").unwrap());
        table.insert(contract_only).unwrap();
        assert_eq!(table.providers_of(SONNET, ON).len(), 2);
    }

    #[test]
    fn the_contracts_own_price_wins_for_its_messages_only() {
        let mut table = table();
        let deal = TagValue::new("anthropic-api").unwrap();
        let mut own = row(
            TokenKind::INPUT,
            "2",
            date!(2026 - 02 - 01),
            PriceSource::Managed,
        );
        own.contract = Some(deal.clone());
        table.insert(own).unwrap();
        // An organization row valid later does not beat the contract's row.
        table
            .insert(row(
                TokenKind::INPUT,
                "2.5",
                date!(2026 - 03 - 01),
                PriceSource::Organization,
            ))
            .unwrap();
        assert_eq!(
            table.price("anthropic", SONNET, &TokenKind::INPUT, ON, Some(&deal)),
            Some(usd("2"))
        );
        assert_eq!(
            table.price("anthropic", SONNET, &TokenKind::INPUT, ON, None),
            Some(usd("2.5"))
        );
        let other = TagValue::new("claude-team").unwrap();
        assert_eq!(
            table.price("anthropic", SONNET, &TokenKind::INPUT, ON, Some(&other)),
            Some(usd("2.5"))
        );
        // Kinds the contract has no row for take the table's price.
        let cost = table
            .cost_of(
                Some(&input(1_000_000).with(TokenKind::OUTPUT, 1_000_000)),
                Some("anthropic"),
                SONNET,
                MARCH,
                Some(&deal),
            )
            .unwrap();
        assert_eq!(cost.amount, Some(usd("17")));
    }

    #[test]
    fn the_price_as_of_the_message_time_is_the_latest_valid_row() {
        let mut table = table();
        table
            .insert(row(
                TokenKind::INPUT,
                "6",
                date!(2026 - 03 - 15),
                PriceSource::Managed,
            ))
            .unwrap();
        let price = |on| table.price("anthropic", SONNET, &TokenKind::INPUT, on, None);
        assert_eq!(price(date!(2026 - 03 - 10)), Some(usd("3")));
        assert_eq!(price(date!(2026 - 03 - 15)), Some(usd("6")));
        assert_eq!(price(date!(2025 - 12 - 31)), None);
        let tokens = input(1_000_000);
        let before = table
            .cost(&message(
                Some(tokens.clone()),
                datetime!(2026-03-14 23:59 UTC),
            ))
            .unwrap();
        assert_eq!(before.amount, Some(usd("3")));
        let after = table
            .cost(&message(
                Some(tokens.clone()),
                datetime!(2026-03-15 00:00 UTC),
            ))
            .unwrap();
        assert_eq!(after.amount, Some(usd("6")));
        let too_early = table
            .cost(&message(Some(tokens), datetime!(2025-12-31 23:59 UTC)))
            .unwrap();
        assert_eq!(too_early.label, CostLabel::NotPricedYet);
    }

    #[test]
    fn the_organizations_override_wins_over_the_managed_row() {
        let mut table = table();
        table
            .insert(row(
                TokenKind::INPUT,
                "2",
                date!(2026 - 02 - 01),
                PriceSource::Organization,
            ))
            .unwrap();
        // A newer managed row does not beat an older override.
        table
            .insert(row(
                TokenKind::INPUT,
                "9",
                date!(2026 - 03 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        let price = |on| table.price("anthropic", SONNET, &TokenKind::INPUT, on, None);
        assert_eq!(price(date!(2026 - 03 - 10)), Some(usd("2")));
        // Before the override was valid, the managed row stands.
        assert_eq!(price(date!(2026 - 01 - 15)), Some(usd("3")));
        assert_eq!(table.prices(&sonnet(), ON).len(), 4);
        assert!(table.prices(&ModelRef::unreported(SONNET), ON).is_empty());
        assert_eq!(table.rows().count(), 6);
    }

    #[test]
    fn a_fraction_of_a_nano_unit_rounds_half_away_from_zero() {
        assert_eq!(per_million(500_000, Currency::USD).unwrap().nanos(), 1);
        assert_eq!(per_million(499_999, Currency::USD).unwrap().nanos(), 0);
        assert_eq!(per_million(-500_000, Currency::USD).unwrap().nanos(), -1);
        assert_eq!(per_million(-499_999, Currency::USD).unwrap().nanos(), 0);
        assert_eq!(per_million(2_500_000, Currency::USD).unwrap().nanos(), 3);
        assert_eq!(
            per_million(i128::MAX, Currency::USD),
            Err(MoneyError::Overflow)
        );
        let mut table = PriceTable::new(Currency::USD);
        table
            .insert(PriceRow {
                per_million: Money::from_nanos(i64::MAX, Currency::USD),
                ..row(
                    TokenKind::INPUT,
                    "0",
                    date!(2026 - 01 - 01),
                    PriceSource::Managed,
                )
            })
            .unwrap();
        assert_eq!(
            table.cost(&message(Some(input(u64::MAX)), MARCH)),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn prices_by_kind_list_only_the_kinds_with_a_row() {
        let mut table = PriceTable::new(Currency::USD);
        assert!(table.prices(&sonnet(), ON).is_empty());
        table
            .insert(row(
                TokenKind::OUTPUT,
                "15",
                date!(2026 - 01 - 01),
                PriceSource::Managed,
            ))
            .unwrap();
        let prices = table.prices(&sonnet(), ON);
        assert_eq!(prices.len(), 1);
        assert_eq!(prices.get(&TokenKind::OUTPUT), Some(&usd("15")));
        assert_eq!(prices.get(&TokenKind::INPUT), None);
        assert!(table.prices(&sonnet(), date!(2025 - 12 - 31)).is_empty());
    }

    #[test]
    fn rows_must_be_in_the_tables_currency() {
        let mut table = PriceTable::new(Currency::USD);
        let mut euro = row(
            TokenKind::INPUT,
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
        assert_eq!(row.contract, None);
        assert_eq!(row.model_ref(), sonnet());
        assert_eq!(row.valid_from, date!(2026 - 01 - 01));
        assert!(!serde_json::to_string(&row).unwrap().contains("contract"));
        let json = serde_json::to_string(&table()).unwrap();
        assert_eq!(serde_json::from_str::<PriceTable>(&json).unwrap(), table());
        assert_eq!(
            date_of(datetime!(2026-04-01 01:00 +02:00)),
            date!(2026 - 03 - 31)
        );
    }
}
