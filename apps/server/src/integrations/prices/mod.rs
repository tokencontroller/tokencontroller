//! The price table: published prices per model from `LiteLLM` and `models.dev`,
//! refreshed daily, every change a new version.
//!
//! [`fetch`] reads both sources and merges them into one price table,
//! [`table_digest`] tells whether a refresh changed it, [`lookup`] finds a
//! model's row and [`list_value`] turns token counts into list value. Storing
//! versions and scheduling the refresh happen elsewhere.

mod fetch;
mod litellm;
mod models_dev;
mod names;
mod table;
mod value;

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr as _;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokencontroller_core::{Currency, Money};

use crate::integrations::decimal::money_from_decimal;

pub use fetch::{Fetched, LITELLM_URL, MODELS_DEV_URL, fetch};
pub use litellm::parse_litellm;
pub use models_dev::parse_models_dev;
pub use names::lookup;
pub use table::{merge, table_digest};
pub use value::list_value;

/// One row of the price table: a model's published prices in USD per one
/// million tokens, by kind. A kind without a price is `None`, never zero.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelPrice {
    /// The model name, lowercase and without a provider path such as
    /// `anthropic/` or `bedrock/us-east-1/`.
    pub model: String,
    /// Whose price this is, as the source names it, such as `anthropic` or
    /// `vertex_ai-language-models`.
    pub provider: Option<String>,
    pub input: Option<Money>,
    pub output: Option<Money>,
    pub cache_write: Option<Money>,
    pub cache_read: Option<Money>,
}

/// A price as it sorts: by currency, then amount.
type PriceKey = Option<(Currency, i64)>;

impl ModelPrice {
    /// Model name first, then provider, then the prices.
    fn sort_key(&self) -> (&str, Option<&str>, [PriceKey; 4]) {
        let price = |money: Option<Money>| money.map(|money| (money.currency(), money.nanos()));
        (
            &self.model,
            self.provider.as_deref(),
            [
                price(self.input),
                price(self.output),
                price(self.cache_write),
                price(self.cache_read),
            ],
        )
    }
}

impl Ord for ModelPrice {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

impl PartialOrd for ModelPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Where published prices come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceSource {
    /// `LiteLLM`'s `model_prices_and_context_window.json`.
    LiteLlm,
    /// The `models.dev` catalogue, `api.json`.
    ModelsDev,
}

impl fmt::Display for PriceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LiteLlm => "LiteLLM",
            Self::ModelsDev => "models.dev",
        })
    }
}

/// Why prices could not be read or a list value not computed.
#[derive(Debug, thiserror::Error)]
pub enum PriceError {
    #[error("could not fetch the {0} price file: {1}")]
    Fetch(PriceSource, #[source] reqwest::Error),
    #[error("the {0} price file is not in the expected shape: {1}")]
    BadFile(PriceSource, String),
    #[error("the {0} price file has no model with an input and an output price")]
    NoPrices(PriceSource),
    #[error("no price source could be read: {}", join_errors(.0))]
    AllSourcesFailed(Vec<PriceError>),
    #[error("the list value is too large to hold")]
    Overflow,
    #[error("the prices of one model are in more than one currency")]
    MixedCurrencies,
}

fn join_errors(errors: &[PriceError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Reads one price from a source file as USD per million tokens. Absent or
/// `null` is no price. `scale` turns the source's unit into per million
/// tokens: 1,000,000 for a price per token. The number is read through its
/// decimal text, never as a float (ADR 0004).
fn read_price(value: Option<&Value>, scale: Decimal) -> Result<Option<Money>, String> {
    let number = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(number)) => number,
        Some(other) => return Err(format!("{other} is not a number")),
    };
    let text = number.to_string();
    let decimal = Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .map_err(|_| format!("{text} is not a decimal number"))?;
    if decimal < Decimal::ZERO {
        return Err(format!("{text} is below zero"));
    }
    let per_million = decimal
        .checked_mul(scale)
        .ok_or_else(|| format!("{text} is too large"))?;
    money_from_decimal(&per_million.to_string(), Currency::USD)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn row(model: &str, provider: Option<&str>, input: Option<Money>) -> ModelPrice {
        ModelPrice {
            model: model.to_owned(),
            provider: provider.map(str::to_owned),
            input,
            output: None,
            cache_write: None,
            cache_read: None,
        }
    }

    #[test]
    fn rows_sort_by_model_then_provider_then_price() {
        let mut rows = vec![
            row("gpt-5", Some("openai"), Some(usd(2))),
            row("claude-sonnet-4-5", Some("anthropic"), Some(usd(1))),
            row("gpt-5", None, Some(usd(9))),
            row("gpt-5", Some("openai"), Some(usd(1))),
            row("gpt-5", Some("openai"), None),
        ];
        rows.sort();
        assert_eq!(
            rows,
            vec![
                row("claude-sonnet-4-5", Some("anthropic"), Some(usd(1))),
                row("gpt-5", None, Some(usd(9))),
                row("gpt-5", Some("openai"), None),
                row("gpt-5", Some("openai"), Some(usd(1))),
                row("gpt-5", Some("openai"), Some(usd(2))),
            ]
        );
        assert_eq!(
            rows[0].partial_cmp(&rows[1]),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn a_row_reads_and_writes_as_stable_json() {
        let price = ModelPrice {
            model: "claude-sonnet-4-5".to_owned(),
            provider: Some("anthropic".to_owned()),
            input: Some(usd(3_000_000_000)),
            output: Some(usd(15_000_000_000)),
            cache_write: None,
            cache_read: Some(usd(300_000_000)),
        };
        let json = serde_json::to_string(&price).unwrap();
        assert_eq!(
            json,
            r#"{"model":"claude-sonnet-4-5","provider":"anthropic","input":{"nanos":3000000000,"currency":"USD"},"output":{"nanos":15000000000,"currency":"USD"},"cache_write":null,"cache_read":{"nanos":300000000,"currency":"USD"}}"#
        );
        assert_eq!(serde_json::from_str::<ModelPrice>(&json).unwrap(), price);
    }

    #[test]
    fn sources_and_errors_say_what_failed() {
        assert_eq!(PriceSource::LiteLlm.to_string(), "LiteLLM");
        assert_eq!(PriceSource::ModelsDev.to_string(), "models.dev");
        let both = PriceError::AllSourcesFailed(vec![
            PriceError::NoPrices(PriceSource::LiteLlm),
            PriceError::BadFile(PriceSource::ModelsDev, "not JSON".to_owned()),
        ]);
        assert_eq!(
            both.to_string(),
            "no price source could be read: the LiteLLM price file has no model with an input \
             and an output price; the models.dev price file is not in the expected shape: not JSON"
        );
    }

    #[test]
    fn prices_are_read_through_their_decimal_text() {
        let per_token = Decimal::from(1_000_000_u32);
        let read = |json: &str, scale: Decimal| {
            let value: Value = serde_json::from_str(json).unwrap();
            read_price(Some(&value), scale)
        };
        assert_eq!(read("3e-06", per_token), Ok(Some(usd(3_000_000_000))));
        assert_eq!(read("3.75e-06", per_token), Ok(Some(usd(3_750_000_000))));
        assert_eq!(read("3e-07", per_token), Ok(Some(usd(300_000_000))));
        assert_eq!(read("6.25e-08", per_token), Ok(Some(usd(62_500_000))));
        assert_eq!(read("0.0000075", per_token), Ok(Some(usd(7_500_000_000))));
        // Digits below a nano-unit per million round half to even.
        assert_eq!(read("1.2345678915e-9", per_token), Ok(Some(usd(1_234_568))));
        assert_eq!(read("1.2345675e-9", per_token), Ok(Some(usd(1_234_568))));
        assert_eq!(read("1.2345665e-9", per_token), Ok(Some(usd(1_234_566))));
        assert_eq!(read("0", per_token), Ok(Some(usd(0))));
        assert_eq!(read("-0.0", per_token), Ok(Some(usd(0))));

        assert_eq!(read("3.75", Decimal::ONE), Ok(Some(usd(3_750_000_000))));
        assert_eq!(
            read("0.30000000000000004", Decimal::ONE),
            Ok(Some(usd(300_000_000)))
        );
        assert_eq!(read("15", Decimal::ONE), Ok(Some(usd(15_000_000_000))));

        assert_eq!(read_price(None, per_token), Ok(None));
        assert_eq!(read("null", per_token), Ok(None));
        assert!(read("\"3e-06\"", per_token).is_err());
        assert!(read("-1e-06", per_token).is_err());
        assert!(read("1e300", per_token).is_err());
        assert!(read("1e10", per_token).is_err());
    }
}
