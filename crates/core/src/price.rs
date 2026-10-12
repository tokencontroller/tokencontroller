//! Price tables and what a token entry is worth.
//!
//! List value is the agent's reported cost where it gives one, otherwise
//! token counts times the price table. A model missing from the table is an
//! unknown model: its cost is labelled, never shown as zero. Price tables
//! are versioned with [`History`](crate::History); a closed period keeps the
//! version it closed with (ADR 0007).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::message::{ModelRef, Usage};
use crate::money::{Currency, Money, MoneyError};
use crate::status::CostSource;
use crate::tokens::{TokenCounts, TokenKind};

/// Tokens a price is quoted for.
const TOKENS_PER_PRICE: i128 = 1_000_000;

/// Published prices of one model, per million tokens of each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: Money,
    pub output: Money,
    pub cache_write: Money,
    pub cache_read: Money,
}

impl ModelPrice {
    /// The price per million tokens of one kind.
    #[must_use]
    pub const fn get(self, kind: TokenKind) -> Money {
        match kind {
            TokenKind::Input => self.input,
            TokenKind::Output => self.output,
            TokenKind::CacheWrite => self.cache_write,
            TokenKind::CacheRead => self.cache_read,
        }
    }

    /// What `tokens` cost at these prices, rounded half away from zero to
    /// the nano-unit once for the whole sum.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the kinds are priced in different
    /// currencies, or [`MoneyError::Overflow`].
    pub fn cost(self, tokens: TokenCounts) -> Result<Money, MoneyError> {
        let currency = self.input.currency();
        let mut scaled = 0_i128;
        for kind in TokenKind::ALL {
            let price = self.get(kind);
            if price.currency() != currency {
                return Err(MoneyError::CurrencyMismatch {
                    left: currency,
                    right: price.currency(),
                });
            }
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
}

/// The published prices of every known model, in one currency (USD for the
/// prices refreshed daily from `LiteLLM` and models.dev).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceTable {
    currency: Currency,
    #[serde(with = "model_prices")]
    models: BTreeMap<ModelRef, ModelPrice>,
}

/// Price tables read and write their models as a list.
mod model_prices {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::ModelPrice;
    use crate::message::ModelRef;

    #[derive(Serialize, Deserialize)]
    struct Row {
        #[serde(flatten)]
        model: ModelRef,
        #[serde(flatten)]
        price: ModelPrice,
    }

    pub(super) fn serialize<S: Serializer>(
        models: &BTreeMap<ModelRef, ModelPrice>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        models
            .iter()
            .map(|(model, price)| Row {
                model: model.clone(),
                price: *price,
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<ModelRef, ModelPrice>, D::Error> {
        Ok(Vec::<Row>::deserialize(deserializer)?
            .into_iter()
            .map(|row| (row.model, row.price))
            .collect())
    }
}

impl PriceTable {
    /// An empty table in a currency.
    #[must_use]
    pub fn new(currency: Currency) -> Self {
        Self {
            currency,
            models: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.currency
    }

    /// Sets the prices of a model, replacing any it had.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if a price is not in the table's
    /// currency.
    pub fn insert(&mut self, model: ModelRef, price: ModelPrice) -> Result<(), MoneyError> {
        for kind in TokenKind::ALL {
            let currency = price.get(kind).currency();
            if currency != self.currency {
                return Err(MoneyError::CurrencyMismatch {
                    left: self.currency,
                    right: currency,
                });
            }
        }
        self.models.insert(model, price);
        Ok(())
    }

    /// The prices of a model, if the table knows it.
    #[must_use]
    pub fn get(&self, model: &ModelRef) -> Option<&ModelPrice> {
        self.models.get(model)
    }

    /// The models in the table, in order.
    pub fn models(&self) -> impl Iterator<Item = (&ModelRef, &ModelPrice)> {
        self.models.iter()
    }

    /// What some usage is worth.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if a reported or billed cost is not
    /// in the table's currency, or [`MoneyError::Overflow`].
    pub fn value(&self, usage: &Usage) -> Result<Valuation, MoneyError> {
        let mut valuation = Valuation::zero(self.currency);
        let list = match (usage.reported_cost, self.get(&usage.model)) {
            (Some(reported), _) => Some(reported),
            (None, Some(price)) => Some(price.cost(usage.tokens)?),
            (None, None) => None,
        };
        match list {
            Some(list) => valuation.list = valuation.list.checked_add(list)?,
            None => {
                valuation.unknown_models.insert(usage.model.clone());
            }
        }
        valuation.billed = match usage.billed_cost {
            Some(billed) => Some(Money::zero(self.currency).checked_add(billed)?),
            None => None,
        };
        Ok(valuation)
    }

    /// What a list of usage is worth together.
    ///
    /// # Errors
    ///
    /// As [`PriceTable::value`].
    pub fn value_all<'a, I>(&self, usage: I) -> Result<Valuation, MoneyError>
    where
        I: IntoIterator<Item = &'a Usage>,
    {
        usage
            .into_iter()
            .try_fold(Valuation::zero(self.currency), |sum, usage| {
                sum.checked_add(&self.value(usage)?)
            })
    }
}

/// What a token entry or a split is worth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Valuation {
    /// The list value of every part whose model is known.
    pub list: Money,
    /// What the provider actually charged, where billed telemetry says so
    /// for every part.
    pub billed: Option<Money>,
    /// Models missing from the price table. Their cost is not in `list`.
    pub unknown_models: BTreeSet<ModelRef>,
}

impl Valuation {
    /// Nothing, in a currency.
    #[must_use]
    pub fn zero(currency: Currency) -> Self {
        Self {
            list: Money::zero(currency),
            billed: Some(Money::zero(currency)),
            unknown_models: BTreeSet::new(),
        }
    }

    /// Two valuations together. A billed value remains only if both have
    /// one.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] or [`MoneyError::Overflow`].
    pub fn checked_add(&self, other: &Self) -> Result<Self, MoneyError> {
        let billed = match (self.billed, other.billed) {
            (Some(left), Some(right)) => Some(left.checked_add(right)?),
            _ => None,
        };
        Ok(Self {
            list: self.list.checked_add(other.list)?,
            billed,
            unknown_models: self
                .unknown_models
                .union(&other.unknown_models)
                .cloned()
                .collect(),
        })
    }

    /// Whether every part's model is known.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unknown_models.is_empty()
    }

    /// Divides the valuation into shares in proportion to `weights`, with
    /// the largest-remainder method, so the shares add up to the whole.
    /// Every share keeps the unknown models.
    ///
    /// # Errors
    ///
    /// As [`Money::allocate`].
    pub fn allocate(&self, weights: &[u64]) -> Result<Vec<Self>, MoneyError> {
        let lists = self.list.allocate(weights)?;
        let billed = match self.billed {
            Some(billed) => billed.allocate(weights)?.into_iter().map(Some).collect(),
            None => vec![None; weights.len()],
        };
        Ok(lists
            .into_iter()
            .zip(billed)
            .map(|(list, billed)| Self {
                list,
                billed,
                unknown_models: self.unknown_models.clone(),
            })
            .collect())
    }

    /// How a figure is labelled: billed, list price, an estimate, or unknown
    /// model.
    #[must_use]
    pub fn label(&self, cost_source: CostSource) -> Figure {
        if !self.is_complete() {
            Figure::UnknownModel
        } else if !cost_source.is_metered() {
            Figure::Estimate
        } else if self.billed.is_some() {
            Figure::Billed
        } else {
            Figure::ListPrice
        }
    }
}

/// What a figure is. Every figure says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Figure {
    /// What the provider actually charged.
    Billed,
    /// The cost at the provider's published prices.
    ListPrice,
    /// What usage inside a seat or subscription would have cost at API
    /// prices.
    Estimate,
    /// Part of the usage is on a model missing from the price table.
    UnknownModel,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn sonnet() -> ModelRef {
        ModelRef::new("anthropic", "claude-sonnet-4-5")
    }

    fn sonnet_price() -> ModelPrice {
        ModelPrice {
            input: usd("3"),
            output: usd("15"),
            cache_write: usd("3.75"),
            cache_read: usd("0.30"),
        }
    }

    fn table() -> PriceTable {
        let mut table = PriceTable::new(Currency::USD);
        table.insert(sonnet(), sonnet_price()).unwrap();
        table
    }

    fn usage(tokens: TokenCounts) -> Usage {
        Usage {
            model: sonnet(),
            tokens,
            reported_cost: None,
            billed_cost: None,
        }
    }

    #[test]
    fn list_value_is_tokens_times_the_price_per_million() {
        let tokens = TokenCounts {
            input: 1_000_000,
            output: 100_000,
            cache_write: 10_000,
            cache_read: 1_000,
        };
        // 3 + 1.5 + 0.0375 + 0.0003
        assert_eq!(sonnet_price().cost(tokens).unwrap(), usd("4.5378"));
        assert_eq!(sonnet_price().get(TokenKind::CacheRead), usd("0.30"));
        let valuation = table().value(&usage(tokens)).unwrap();
        assert_eq!(valuation.list, usd("4.5378"));
        assert_eq!(valuation.billed, None);
        assert!(valuation.is_complete());
    }

    #[test]
    fn a_fraction_of_a_nano_unit_rounds_half_away_from_zero() {
        let price = ModelPrice {
            input: Money::from_nanos(1, Currency::USD),
            output: Money::from_nanos(-1, Currency::USD),
            cache_write: Money::zero(Currency::USD),
            cache_read: Money::zero(Currency::USD),
        };
        let half = TokenCounts {
            input: 500_000,
            ..TokenCounts::default()
        };
        assert_eq!(price.cost(half).unwrap().nanos(), 1);
        let less = TokenCounts {
            input: 499_999,
            ..TokenCounts::default()
        };
        assert_eq!(price.cost(less).unwrap().nanos(), 0);
        let negative = TokenCounts {
            output: 500_000,
            ..TokenCounts::default()
        };
        assert_eq!(price.cost(negative).unwrap().nanos(), -1);
        let small_negative = TokenCounts {
            output: 499_999,
            ..TokenCounts::default()
        };
        assert_eq!(price.cost(small_negative).unwrap().nanos(), 0);
    }

    #[test]
    fn prices_are_in_one_currency() {
        let mut mixed = sonnet_price();
        mixed.cache_read = Money::zero(Currency::EUR);
        assert!(matches!(
            mixed.cost(TokenCounts::default()),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        assert!(matches!(
            table().insert(sonnet(), mixed),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        let huge = ModelPrice {
            input: Money::from_nanos(i64::MAX, Currency::USD),
            ..sonnet_price()
        };
        let many = TokenCounts {
            input: u64::MAX,
            ..TokenCounts::default()
        };
        assert_eq!(huge.cost(many), Err(MoneyError::Overflow));
    }

    #[test]
    fn a_reported_cost_wins_over_the_price_table() {
        let mut reported = usage(TokenCounts {
            input: 1_000_000,
            ..TokenCounts::default()
        });
        reported.reported_cost = Some(usd("2.50"));
        assert_eq!(table().value(&reported).unwrap().list, usd("2.50"));
    }

    #[test]
    fn an_unknown_model_is_labelled_not_zero() {
        let mut unknown = usage(TokenCounts {
            input: 1_000,
            ..TokenCounts::default()
        });
        unknown.model = ModelRef::new("acme", "mystery-1");
        let valuation = table().value(&unknown).unwrap();
        assert!(!valuation.is_complete());
        assert_eq!(valuation.list, usd("0"));
        assert_eq!(valuation.label(CostSource::ApiKey), Figure::UnknownModel);
        assert_eq!(valuation.label(CostSource::Seat), Figure::UnknownModel);
        // An unknown model with a reported cost is known enough.
        unknown.reported_cost = Some(usd("1"));
        assert!(table().value(&unknown).unwrap().is_complete());
    }

    #[test]
    fn billed_value_stays_only_while_every_part_is_billed() {
        let mut billed = usage(TokenCounts::default());
        billed.billed_cost = Some(usd("1.10"));
        let unbilled = usage(TokenCounts {
            input: 1_000_000,
            ..TokenCounts::default()
        });
        let table = table();
        let both = table.value_all([&billed, &billed]).unwrap();
        assert_eq!(both.billed, Some(usd("2.20")));
        assert_eq!(both.label(CostSource::ApiKey), Figure::Billed);
        let mixed = table.value_all([&billed, &unbilled]).unwrap();
        assert_eq!(mixed.billed, None);
        assert_eq!(mixed.list, usd("3"));
        assert_eq!(mixed.label(CostSource::UsageAtApiRates), Figure::ListPrice);
        assert_eq!(mixed.label(CostSource::Seat), Figure::Estimate);
        assert_eq!(both.label(CostSource::Subscription), Figure::Estimate);
        let empty = table.value_all([]).unwrap();
        assert_eq!(empty, Valuation::zero(Currency::USD));
    }

    #[test]
    fn costs_in_another_currency_than_the_table_are_refused() {
        let mut euro = usage(TokenCounts::default());
        euro.reported_cost = Some(Money::zero(Currency::EUR));
        assert!(
            table()
                .value_all([&usage(TokenCounts::default()), &euro])
                .is_err()
        );
        let mut billed_euro = usage(TokenCounts::default());
        billed_euro.billed_cost = Some(Money::zero(Currency::EUR));
        assert!(table().value(&billed_euro).is_err());
    }

    #[test]
    fn valuations_split_into_shares_that_add_up() {
        let mut billed = usage(TokenCounts::default());
        billed.billed_cost = Some(Money::from_nanos(10, Currency::USD));
        billed.reported_cost = Some(Money::from_nanos(10, Currency::USD));
        let mut unknown = usage(TokenCounts::default());
        unknown.model = ModelRef::new("acme", "mystery-1");
        let table = table();
        let whole = table.value(&billed).unwrap();
        let shares = whole.allocate(&[60, 40]).unwrap();
        let lists: Vec<i64> = shares.iter().map(|share| share.list.nanos()).collect();
        assert_eq!(lists, vec![6, 4]);
        let billed_shares: Vec<Option<i64>> = shares
            .iter()
            .map(|share| share.billed.map(Money::nanos))
            .collect();
        assert_eq!(billed_shares, vec![Some(6), Some(4)]);

        let partly_unknown = table.value_all([&billed, &unknown]).unwrap();
        let shares = partly_unknown.allocate(&[1, 1]).unwrap();
        assert!(shares.iter().all(|share| !share.is_complete()));
        assert!(shares.iter().all(|share| share.billed.is_none()));
        assert!(whole.allocate(&[]).is_err());
    }

    #[test]
    fn price_tables_read_and_write_as_json() {
        let json = serde_json::to_string(&table()).unwrap();
        let read: PriceTable = serde_json::from_str(&json).unwrap();
        assert_eq!(read, table());
        assert_eq!(read.currency(), Currency::USD);
        assert_eq!(read.models().count(), 1);
        assert_eq!(read.get(&sonnet()), Some(&sonnet_price()));
    }
}
