//! List value: token counts times the price table.

use rust_decimal::{Decimal, RoundingStrategy};
use tokencontroller_core::{Currency, Money};

use super::{ModelPrice, PriceError};
use crate::ingest::model::TokenCounts;

/// Digits of the one million tokens prices are given for.
const PER_MILLION_DIGITS: u32 = 6;

/// The list value of token counts at a model's prices: tokens times price
/// per million, added up over the four kinds and rounded half to even to a
/// nano-unit once, at the end. `None` if tokens of some kind have no price:
/// the cost is unknown, never shown as zero. Zero tokens of a kind need no
/// price.
///
/// # Errors
///
/// [`PriceError::Overflow`] if the amount does not fit, and
/// [`PriceError::MixedCurrencies`] if the model's prices are in more than one
/// currency.
pub fn list_value(tokens: TokenCounts, price: &ModelPrice) -> Result<Option<Money>, PriceError> {
    let kinds = [
        (tokens.input, price.input),
        (tokens.output, price.output),
        (tokens.cache_write, price.cache_write),
        (tokens.cache_read, price.cache_read),
    ];
    let mut currency = None;
    let mut total = 0_i128;
    for (count, per_million) in kinds {
        if count == 0 {
            continue;
        }
        let Some(per_million) = per_million else {
            return Ok(None);
        };
        if currency.is_some_and(|currency| currency != per_million.currency()) {
            return Err(PriceError::MixedCurrencies);
        }
        currency = Some(per_million.currency());
        total = i128::from(count)
            .checked_mul(i128::from(per_million.nanos()))
            .and_then(|amount| total.checked_add(amount))
            .ok_or(PriceError::Overflow)?;
    }
    let nanos = Decimal::try_from_i128_with_scale(total, PER_MILLION_DIGITS)
        .map_err(|_| PriceError::Overflow)?
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven);
    let nanos = i64::try_from(nanos).map_err(|_| PriceError::Overflow)?;
    Ok(Some(Money::from_nanos(
        nanos,
        currency.unwrap_or(Currency::USD),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn price(prices: [Option<Money>; 4]) -> ModelPrice {
        let [input, output, cache_write, cache_read] = prices;
        ModelPrice {
            model: "m".to_owned(),
            provider: None,
            input,
            output,
            cache_write,
            cache_read,
        }
    }

    fn tokens(counts: [u64; 4]) -> TokenCounts {
        let [input, output, cache_write, cache_read] = counts;
        TokenCounts {
            input,
            output,
            cache_write,
            cache_read,
        }
    }

    fn sonnet() -> ModelPrice {
        price([
            Some(usd(3_000_000_000)),
            Some(usd(15_000_000_000)),
            Some(usd(3_750_000_000)),
            Some(usd(300_000_000)),
        ])
    }

    #[test]
    fn tokens_times_price_per_million_add_up() {
        // 1,234 × 3 + 567 × 15 + 89 × 3.75 + 10,001 × 0.3 USD per million.
        assert_eq!(
            list_value(tokens([1_234, 567, 89, 10_001]), &sonnet()).unwrap(),
            Some(usd(15_541_050))
        );
        assert_eq!(
            list_value(tokens([1_000_000, 0, 0, 0]), &sonnet()).unwrap(),
            Some(usd(3_000_000_000))
        );
        assert_eq!(
            list_value(tokens([1, 0, 0, 0]), &sonnet()).unwrap(),
            Some(usd(3_000))
        );
        assert_eq!(list_value(tokens([0; 4]), &sonnet()).unwrap(), Some(usd(0)));
    }

    #[test]
    fn rounding_is_half_to_even_once_at_the_end() {
        // 62.5 nanos per token, as gpt-5 flex cache reads at 0.0625 USD per million.
        let flex = price([
            Some(usd(62_500_000)),
            Some(usd(62_500_000)),
            None,
            Some(usd(62_500_000)),
        ]);
        assert_eq!(
            list_value(tokens([1, 0, 0, 0]), &flex).unwrap(),
            Some(usd(62))
        );
        assert_eq!(
            list_value(tokens([3, 0, 0, 0]), &flex).unwrap(),
            Some(usd(188))
        );
        // Rounded per kind this would be 62 + 62.
        assert_eq!(
            list_value(tokens([1, 1, 0, 0]), &flex).unwrap(),
            Some(usd(125))
        );
        assert_eq!(
            list_value(tokens([1, 1, 0, 1]), &flex).unwrap(),
            Some(usd(188))
        );

        let one_nano = price([Some(usd(1)), None, None, None]);
        assert_eq!(
            list_value(tokens([499_999, 0, 0, 0]), &one_nano).unwrap(),
            Some(usd(0))
        );
        assert_eq!(
            list_value(tokens([500_000, 0, 0, 0]), &one_nano).unwrap(),
            Some(usd(0))
        );
        assert_eq!(
            list_value(tokens([500_001, 0, 0, 0]), &one_nano).unwrap(),
            Some(usd(1))
        );
        assert_eq!(
            list_value(tokens([1_500_000, 0, 0, 0]), &one_nano).unwrap(),
            Some(usd(2))
        );
        assert_eq!(
            list_value(tokens([2_500_000, 0, 0, 0]), &one_nano).unwrap(),
            Some(usd(2))
        );

        let refund = price([Some(usd(-62_500_000)), None, None, None]);
        assert_eq!(
            list_value(tokens([1, 0, 0, 0]), &refund).unwrap(),
            Some(usd(-62))
        );
        assert_eq!(
            list_value(tokens([3, 0, 0, 0]), &refund).unwrap(),
            Some(usd(-188))
        );
    }

    #[test]
    fn tokens_of_an_unpriced_kind_make_the_value_unknown() {
        let no_cache_write = price([
            Some(usd(1_250_000_000)),
            Some(usd(10_000_000_000)),
            None,
            Some(usd(125_000_000)),
        ]);
        assert_eq!(
            list_value(tokens([10, 10, 1, 10]), &no_cache_write).unwrap(),
            None
        );
        assert_eq!(
            list_value(tokens([10, 10, 0, 10]), &no_cache_write).unwrap(),
            Some(usd(113_750))
        );
        assert_eq!(
            list_value(tokens([0; 4]), &price([None; 4])).unwrap(),
            Some(usd(0))
        );
        assert_eq!(
            list_value(tokens([1, 0, 0, 0]), &price([None; 4])).unwrap(),
            None
        );
    }

    #[test]
    fn a_value_too_large_is_an_error() {
        let just_fits = u64::try_from(i64::MAX).unwrap();
        let thousandth = price([Some(usd(1_000_000)), None, None, None]);
        assert_eq!(
            list_value(tokens([just_fits, 0, 0, 0]), &thousandth).unwrap(),
            Some(usd(i64::MAX))
        );
        let too_many = just_fits.checked_add(1).unwrap();
        assert!(matches!(
            list_value(tokens([too_many, 0, 0, 0]), &thousandth),
            Err(PriceError::Overflow)
        ));
        assert!(matches!(
            list_value(tokens([u64::MAX, 0, 0, 0]), &sonnet()),
            Err(PriceError::Overflow)
        ));
        let dearest = price([Some(usd(i64::MAX)); 4]);
        assert!(matches!(
            list_value(tokens([u64::MAX; 4]), &dearest),
            Err(PriceError::Overflow)
        ));
        assert!(matches!(
            list_value(tokens([u64::MAX, 0, 0, 0]), &dearest),
            Err(PriceError::Overflow)
        ));
    }

    #[test]
    fn the_value_keeps_the_prices_currency() {
        let euros = Some(Money::from_nanos(1_000_000, Currency::EUR));
        let value = list_value(tokens([1, 0, 0, 0]), &price([euros, euros, None, None])).unwrap();
        assert_eq!(value, Some(Money::from_nanos(1, Currency::EUR)));
        assert!(matches!(
            list_value(
                tokens([1, 1, 0, 0]),
                &price([euros, Some(usd(1)), None, None])
            ),
            Err(PriceError::MixedCurrencies)
        ));
        assert_eq!(
            list_value(
                tokens([1, 0, 0, 0]),
                &price([euros, Some(usd(1)), None, None])
            )
            .unwrap(),
            Some(Money::from_nanos(1, Currency::EUR))
        );
    }
}
