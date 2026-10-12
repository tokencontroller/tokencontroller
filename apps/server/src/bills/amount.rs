//! Amounts and exchange rates as the API and CSV files write them: decimal
//! strings such as `1234.56` and `0.9214`, never floats (ADR 0004).

use std::fmt;

use tokencontroller_core::{Currency, ExchangeRate, Money};

/// Decimal digits a nano-unit has.
const NANO_DIGITS: usize = 9;

/// Nano-units in one: the fixed point of amounts and exchange rates.
const NANOS_PER_UNIT: i128 = 1_000_000_000;

/// Why an amount or a rate could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    #[error("{0:?} is not an amount: write it as a decimal number, such as 1234.56")]
    Invalid(String),
    #[error(
        "{0:?} is not an exchange rate: write it as a decimal number above zero, such as 0.9214"
    )]
    InvalidRate(String),
    #[error("the amount is too large to hold")]
    Overflow,
}

/// Reads a decimal number with up to nine digits after the point into
/// nano-units.
fn parse_nanos(text: &str) -> Option<i64> {
    let text = text.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || fraction.len() > NANO_DIGITS
        || (unsigned.contains('.') && fraction.is_empty())
    {
        return None;
    }
    if !whole
        .chars()
        .chain(fraction.chars())
        .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let mut padded = fraction.to_owned();
    while padded.len() < NANO_DIGITS {
        padded.push('0');
    }
    let fraction: i64 = padded.parse().ok()?;
    let magnitude = whole.checked_mul(1_000_000_000)?.checked_add(fraction)?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// Reads an amount such as `1234.56` or `-12` in `currency`, with spaces
/// around it allowed.
///
/// # Errors
///
/// [`AmountError::Invalid`] for anything but a decimal number with at most
/// nine digits after the point.
pub fn parse_amount(text: &str, currency: Currency) -> Result<Money, AmountError> {
    Money::parse(text.trim(), currency).map_err(|_| AmountError::Invalid(text.to_owned()))
}

/// An amount as the API writes it: rounded to the currency's minor unit,
/// such as `1234.56`, without the currency code.
#[must_use]
pub fn format_amount(amount: Money) -> String {
    let shown = amount.to_string();
    shown
        .rsplit_once(' ')
        .map_or_else(|| shown.clone(), |(number, _code)| number.to_owned())
}

/// A rate as a plain number in nano-units, before it is tied to two
/// currencies: `0.9214` is 921,400,000. The reference rates are numbers per
/// euro; [`exchange_rate`] turns one into the core's rate between two
/// currencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RateNumber(i64);

impl RateNumber {
    /// One: a currency to itself.
    pub const ONE: Self = Self(1_000_000_000);

    /// A rate kept in nano-units.
    ///
    /// # Errors
    ///
    /// [`AmountError::InvalidRate`] for a rate of zero or below.
    pub fn from_nanos(nanos: i64) -> Result<Self, AmountError> {
        if nanos > 0 {
            Ok(Self(nanos))
        } else {
            Err(AmountError::InvalidRate(nanos.to_string()))
        }
    }

    /// Reads a rate such as `0.9214`.
    ///
    /// # Errors
    ///
    /// [`AmountError::InvalidRate`] for anything but a decimal number above
    /// zero with at most nine digits after the point.
    pub fn parse(text: &str) -> Result<Self, AmountError> {
        parse_nanos(text)
            .filter(|nanos| *nanos > 0)
            .map(Self)
            .ok_or_else(|| AmountError::InvalidRate(text.to_owned()))
    }

    /// The rate in nano-units, as stored.
    #[must_use]
    pub const fn nanos(self) -> i64 {
        self.0
    }

    /// The rate the other way round, rounded to the nano.
    ///
    /// # Errors
    ///
    /// [`AmountError::Overflow`] for a rate too small to turn round.
    pub fn inverse(self) -> Result<Self, AmountError> {
        let squared = NANOS_PER_UNIT
            .checked_mul(NANOS_PER_UNIT)
            .ok_or(AmountError::Overflow)?;
        let nanos = divide_rounded(squared, i128::from(self.0)).ok_or(AmountError::Overflow)?;
        i64::try_from(nanos)
            .map_err(|_| AmountError::Overflow)
            .and_then(Self::from_nanos)
    }

    /// The rate from `a` to `c` through `b`: this rate (a to b) times
    /// `then` (b to c), rounded to the nano.
    ///
    /// # Errors
    ///
    /// [`AmountError::Overflow`] when the result does not fit.
    pub fn then(self, then: Self) -> Result<Self, AmountError> {
        let product = i128::from(self.0)
            .checked_mul(i128::from(then.0))
            .ok_or(AmountError::Overflow)?;
        let nanos = divide_rounded(product, NANOS_PER_UNIT).ok_or(AmountError::Overflow)?;
        i64::try_from(nanos)
            .map_err(|_| AmountError::Overflow)
            .and_then(Self::from_nanos)
    }
}

impl fmt::Display for RateNumber {
    /// The rate as a decimal number without trailing zeros, such as `0.9214`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0.checked_div(1_000_000_000).unwrap_or(0);
        let fraction = self.0.checked_rem(1_000_000_000).unwrap_or(0);
        if fraction == 0 {
            return write!(f, "{whole}");
        }
        let digits = format!("{fraction:09}");
        write!(f, "{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// The core's rate from `from` to `to` at `number`.
///
/// # Errors
///
/// [`AmountError::InvalidRate`], which a [`RateNumber`] never is.
pub fn exchange_rate(
    from: Currency,
    to: Currency,
    number: RateNumber,
) -> Result<ExchangeRate, AmountError> {
    if from == to {
        return Ok(ExchangeRate::identity(from));
    }
    ExchangeRate::new(from, to, &number.to_string())
        .map_err(|_| AmountError::InvalidRate(number.to_string()))
}

/// The number of a core rate, as stored and shown.
#[must_use]
pub fn rate_number(rate: ExchangeRate) -> RateNumber {
    RateNumber(rate.nanos())
}

/// `numerator / denominator`, rounded half away from zero, for a positive
/// denominator.
fn divide_rounded(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator <= 0 {
        return None;
    }
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    let doubled = remainder.unsigned_abs().checked_mul(2)?;
    if doubled < denominator.unsigned_abs() {
        Some(quotient)
    } else if numerator.is_negative() {
        quotient.checked_sub(1)
    } else {
        quotient.checked_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_decimal_amounts() {
        let eur = Currency::EUR;
        assert_eq!(
            parse_amount("1234.56", eur).unwrap().nanos(),
            1_234_560_000_000
        );
        assert_eq!(parse_amount("-12", eur).unwrap().nanos(), -12_000_000_000);
        assert_eq!(parse_amount("0.000000001", eur).unwrap().nanos(), 1);
        assert_eq!(parse_amount(" 7.5 ", eur).unwrap().nanos(), 7_500_000_000);
        assert_eq!(parse_amount("0", eur).unwrap().nanos(), 0);
    }

    #[test]
    fn refuses_what_is_not_an_amount() {
        for text in [
            "",
            "abc",
            "1,5",
            "1.",
            ".5",
            "1.0000000001",
            "--1",
            "1e3",
            "99999999999",
        ] {
            assert!(parse_amount(text, Currency::EUR).is_err(), "{text}");
        }
    }

    #[test]
    fn writes_amounts_in_the_minor_unit() {
        let amount = parse_amount("1234.565", Currency::EUR).unwrap();
        assert_eq!(format_amount(amount), "1234.57");
        let negative = parse_amount("-0.004", Currency::USD).unwrap();
        assert_eq!(format_amount(negative), "0.00");
    }

    #[test]
    fn reads_and_writes_rates() {
        let rate = RateNumber::parse("0.9214").unwrap();
        assert_eq!(rate.nanos(), 921_400_000);
        assert_eq!(rate.to_string(), "0.9214");
        assert_eq!(RateNumber::parse("1").unwrap(), RateNumber::ONE);
        assert_eq!(RateNumber::ONE.to_string(), "1");
        assert_eq!(RateNumber::parse("156.25").unwrap().to_string(), "156.25");
        for text in ["0", "-1", "", "x", "0.0000000001"] {
            assert!(RateNumber::parse(text).is_err(), "{text}");
        }
        assert!(RateNumber::from_nanos(0).is_err());
        assert!(RateNumber::from_nanos(1).is_ok());
    }

    #[test]
    fn ties_numbers_to_two_currencies() {
        let rate = exchange_rate(
            Currency::USD,
            Currency::EUR,
            RateNumber::parse("0.9214").unwrap(),
        )
        .unwrap();
        assert_eq!((rate.from(), rate.to()), (Currency::USD, Currency::EUR));
        assert_eq!(rate_number(rate).to_string(), "0.9214");
        let usd = parse_amount("100", Currency::USD).unwrap();
        assert_eq!(
            rate.convert(usd).unwrap(),
            parse_amount("92.14", Currency::EUR).unwrap()
        );
        let same = exchange_rate(
            Currency::EUR,
            Currency::EUR,
            RateNumber::parse("2").unwrap(),
        )
        .unwrap();
        assert_eq!(same, ExchangeRate::identity(Currency::EUR));
    }

    #[test]
    fn turns_rates_round_and_chains_them() {
        let usd_to_eur = RateNumber::parse("0.8").unwrap();
        assert_eq!(
            usd_to_eur.inverse().unwrap(),
            RateNumber::parse("1.25").unwrap()
        );
        let eur_to_gbp = RateNumber::parse("0.85").unwrap();
        assert_eq!(
            usd_to_eur.then(eur_to_gbp).unwrap(),
            RateNumber::parse("0.68").unwrap()
        );
        let third = RateNumber::parse("3").unwrap();
        assert_eq!(third.inverse().unwrap().nanos(), 333_333_333);
        let two_thirds = RateNumber::parse("1.5").unwrap();
        assert_eq!(two_thirds.inverse().unwrap().nanos(), 666_666_667);
    }

    #[test]
    fn rounding_division_handles_signs() {
        assert_eq!(divide_rounded(5, 2), Some(3));
        assert_eq!(divide_rounded(-5, 2), Some(-3));
        assert_eq!(divide_rounded(4, 3), Some(1));
        assert_eq!(divide_rounded(-4, 3), Some(-1));
        assert_eq!(divide_rounded(1, 0), None);
        assert_eq!(divide_rounded(1, -2), None);
    }
}
