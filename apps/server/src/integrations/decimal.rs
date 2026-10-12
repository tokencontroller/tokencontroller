//! Decimal amounts from providers, such as `"12.345"` or `3e-6`, read into
//! [`Money`] without floats (ADR 0004).

use std::str::FromStr as _;

use rust_decimal::{Decimal, RoundingStrategy};
use tokencontroller_core::money::NANOS_PER_UNIT;
use tokencontroller_core::{Currency, Money};

/// Why a decimal amount cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    #[error("{0:?} is not a decimal number")]
    NotANumber(String),
    #[error("{0:?} is too large to hold as money")]
    TooLarge(String),
}

/// Reads a decimal amount in currency units, such as `"12.345"`, `"-0.5"` or
/// `"3e-6"`. Digits below one nano-unit are rounded half to even.
///
/// # Errors
///
/// [`DecimalError`] if the text is no number or the amount does not fit.
pub fn money_from_decimal(text: &str, currency: Currency) -> Result<Money, DecimalError> {
    let trimmed = text.trim();
    let value = Decimal::from_str(trimmed)
        .or_else(|_| Decimal::from_scientific(trimmed))
        .map_err(|_| DecimalError::NotANumber(text.to_owned()))?;
    let nanos = value
        .checked_mul(Decimal::from(NANOS_PER_UNIT))
        .ok_or_else(|| DecimalError::TooLarge(text.to_owned()))?
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven);
    let nanos = i64::try_from(nanos).map_err(|_| DecimalError::TooLarge(text.to_owned()))?;
    Ok(Money::from_nanos(nanos, currency))
}

/// Reads a JSON number in currency units, such as the `12.5` in
/// `{"amount": 12.5}`, through its shortest decimal text.
///
/// # Errors
///
/// [`DecimalError`] if the amount does not fit.
pub fn money_from_json_number(
    number: &serde_json::Number,
    currency: Currency,
) -> Result<Money, DecimalError> {
    money_from_decimal(&number.to_string(), currency)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nanos(text: &str) -> i64 {
        money_from_decimal(text, Currency::USD).unwrap().nanos()
    }

    #[test]
    fn plain_and_scientific_decimals_become_nanos() {
        assert_eq!(nanos("1"), 1_000_000_000);
        assert_eq!(nanos("12.345"), 12_345_000_000);
        assert_eq!(nanos("-0.5"), -500_000_000);
        assert_eq!(nanos("3e-6"), 3_000);
        assert_eq!(nanos("3E-7"), 300);
        assert_eq!(nanos(" 0 "), 0);
    }

    #[test]
    fn digits_below_a_nano_round_half_to_even() {
        assert_eq!(nanos("0.0000000005"), 0);
        assert_eq!(nanos("0.0000000015"), 2);
        assert_eq!(nanos("0.0000000016"), 2);
        assert_eq!(nanos("0.0000000014"), 1);
    }

    #[test]
    fn json_numbers_are_read_through_their_text() {
        let number: serde_json::Number = serde_json::from_str("0.000015").unwrap();
        let money = money_from_json_number(&number, Currency::EUR).unwrap();
        assert_eq!(money.nanos(), 15_000);
        assert_eq!(money.currency(), Currency::EUR);
    }

    #[test]
    fn bad_input_is_named() {
        assert_eq!(
            money_from_decimal("twelve", Currency::USD),
            Err(DecimalError::NotANumber("twelve".to_owned()))
        );
        for too_large in ["1e20", "1e15", "9223372037"] {
            assert_eq!(
                money_from_decimal(too_large, Currency::USD),
                Err(DecimalError::TooLarge(too_large.to_owned()))
            );
        }
        assert_eq!(nanos("9223372036"), 9_223_372_036_000_000_000);
    }
}
