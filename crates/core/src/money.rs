//! Money as a whole number of nano-units of a currency (ADR 0004).
//!
//! One nano-unit is one billionth of a currency unit, so one token priced at
//! a few dollars per million tokens is still a whole number. Floats never
//! touch money. Rounding to the currency's minor unit (cents) happens only
//! when a figure is shown or exported, and a split divides with the
//! largest-remainder method so the parts always add up to the whole.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Nano-units in one unit of any currency.
pub const NANOS_PER_UNIT: i64 = 1_000_000_000;

/// Decimal digits in one unit as nano-units.
const NANO_DIGITS: u32 = 9;

/// Everything that can go wrong with money. No operation ever wraps around or
/// silently drops a part.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("{0:?} is not a currency code: use three capital letters, such as EUR")]
    InvalidCurrency(String),
    #[error("cannot combine {left} and {right}: amounts must be in the same currency")]
    CurrencyMismatch { left: Currency, right: Currency },
    #[error("the amount is too large to hold")]
    Overflow,
    #[error("a split needs at least one part with a weight above zero")]
    NoWeights,
    #[error("{0:?} is not an amount: use digits with at most nine after the point, such as 12.34")]
    InvalidAmount(String),
    #[error("{0:?} is not an exchange rate: use a number above zero, such as 0.9234")]
    InvalidRate(String),
}

/// An ISO 4217 currency code, such as `EUR` or `USD`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Currency([u8; 3]);

impl Currency {
    pub const EUR: Self = Self(*b"EUR");
    pub const USD: Self = Self(*b"USD");

    /// Reads a three-letter currency code.
    ///
    /// # Errors
    ///
    /// [`MoneyError::InvalidCurrency`] unless the code is exactly three
    /// capital ASCII letters.
    pub fn new(code: &str) -> Result<Self, MoneyError> {
        let letters = <[u8; 3]>::try_from(code.as_bytes())
            .map_err(|_| MoneyError::InvalidCurrency(code.to_owned()))?;
        if letters.iter().all(u8::is_ascii_uppercase) {
            Ok(Self(letters))
        } else {
            Err(MoneyError::InvalidCurrency(code.to_owned()))
        }
    }

    /// The three-letter code.
    #[must_use]
    pub fn code(self) -> String {
        self.0.iter().copied().map(char::from).collect()
    }

    /// Digits after the decimal point in the currency's minor unit, per
    /// ISO 4217: 2 for EUR and USD, 0 for JPY, 3 for KWD.
    #[must_use]
    pub fn minor_unit_digits(self) -> u32 {
        match &self.0 {
            b"BIF" | b"CLP" | b"DJF" | b"GNF" | b"ISK" | b"JPY" | b"KMF" | b"KRW" | b"PYG"
            | b"RWF" | b"UGX" | b"UYI" | b"VND" | b"VUV" | b"XAF" | b"XOF" | b"XPF" => 0,
            b"BHD" | b"IQD" | b"JOD" | b"KWD" | b"LYD" | b"OMR" | b"TND" => 3,
            b"CLF" | b"UYW" => 4,
            _ => 2,
        }
    }

    /// A whole number of minor units written as a decimal number without the
    /// currency code, such as `12.35` for 1,235 cents: the form CSV exports use.
    #[must_use]
    pub fn format_minor_units(self, minor_units: i64) -> String {
        let digits = self.minor_unit_digits();
        let sign = if minor_units.is_negative() { "-" } else { "" };
        let magnitude = minor_units.unsigned_abs();
        let scale = 10_u64.checked_pow(digits).unwrap_or(1);
        let units = magnitude.checked_div(scale).unwrap_or(0);
        let fraction = magnitude.checked_rem(scale).unwrap_or(0);
        match usize::try_from(digits) {
            Ok(width) if width > 0 => format!("{sign}{units}.{fraction:0width$}"),
            _ => format!("{sign}{units}"),
        }
    }

    /// Nano-units in one minor unit: 10,000,000 for a cent.
    fn nanos_per_minor_unit(self) -> i64 {
        let digits = NANO_DIGITS.saturating_sub(self.minor_unit_digits());
        10_i64.checked_pow(digits).unwrap_or(1)
    }
}

impl TryFrom<String> for Currency {
    type Error = MoneyError;

    fn try_from(code: String) -> Result<Self, Self::Error> {
        Self::new(&code)
    }
}

impl From<Currency> for String {
    fn from(currency: Currency) -> Self {
        currency.code()
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code())
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code())
    }
}

/// An amount of money: a whole number of nano-units with its currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    nanos: i64,
    currency: Currency,
}

impl Money {
    /// Nothing, in the given currency.
    #[must_use]
    pub const fn zero(currency: Currency) -> Self {
        Self { nanos: 0, currency }
    }

    /// An amount given in nano-units.
    #[must_use]
    pub const fn from_nanos(nanos: i64, currency: Currency) -> Self {
        Self { nanos, currency }
    }

    /// An amount given in the currency's minor unit, such as cents.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the amount does not fit.
    pub fn from_minor_units(minor_units: i64, currency: Currency) -> Result<Self, MoneyError> {
        minor_units
            .checked_mul(currency.nanos_per_minor_unit())
            .map(|nanos| Self { nanos, currency })
            .ok_or(MoneyError::Overflow)
    }

    /// Reads an amount written as a decimal number in whole units, such as
    /// `"12.34"` or `"-0.000000001"`, as CSV files and the API carry it.
    ///
    /// # Errors
    ///
    /// [`MoneyError::InvalidAmount`] unless the text is digits with an
    /// optional sign and at most nine digits after the point.
    pub fn parse(amount: &str, currency: Currency) -> Result<Self, MoneyError> {
        parse_billionths(amount)
            .map(|nanos| Self { nanos, currency })
            .ok_or_else(|| MoneyError::InvalidAmount(amount.to_owned()))
    }

    /// The amount in nano-units.
    #[must_use]
    pub const fn nanos(self) -> i64 {
        self.nanos
    }

    /// The currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.nanos == 0
    }

    /// Adds two amounts in the same currency.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] or [`MoneyError::Overflow`].
    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        self.nanos
            .checked_add(other.nanos)
            .map(|nanos| Self::from_nanos(nanos, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    /// Subtracts an amount in the same currency.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] or [`MoneyError::Overflow`].
    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        self.nanos
            .checked_sub(other.nanos)
            .map(|nanos| Self::from_nanos(nanos, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    /// The same amount with the opposite sign.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] for the one amount that has no opposite.
    pub fn checked_neg(self) -> Result<Self, MoneyError> {
        self.nanos
            .checked_neg()
            .map(|nanos| Self::from_nanos(nanos, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    /// The amount times `numerator / denominator`, rounded half away from
    /// zero to the nano-unit: a share of a plan, or a markup on top of it.
    ///
    /// # Errors
    ///
    /// [`MoneyError::Overflow`] if the result does not fit, or
    /// [`MoneyError::NoWeights`] if the denominator is zero.
    pub fn checked_mul_ratio(self, numerator: u64, denominator: u64) -> Result<Self, MoneyError> {
        if denominator == 0 {
            return Err(MoneyError::NoWeights);
        }
        let scaled = i128::from(self.nanos)
            .checked_mul(i128::from(numerator))
            .ok_or(MoneyError::Overflow)?;
        let rounded =
            divide_rounded(scaled, i128::from(denominator)).ok_or(MoneyError::Overflow)?;
        i64::try_from(rounded)
            .map(|nanos| Self::from_nanos(nanos, self.currency))
            .map_err(|_| MoneyError::Overflow)
    }

    /// Adds up amounts. An empty list is zero in `currency`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if any amount is not in `currency`,
    /// or [`MoneyError::Overflow`].
    pub fn sum<I>(amounts: I, currency: Currency) -> Result<Self, MoneyError>
    where
        I: IntoIterator<Item = Self>,
    {
        amounts
            .into_iter()
            .try_fold(Self::zero(currency), Self::checked_add)
    }

    /// The amount in the currency's minor unit, rounded half away from zero:
    /// 0.005 EUR shows as 0.01 EUR and -0.005 EUR as -0.01 EUR. Use it only
    /// to show or export a figure, never to keep computing with it.
    #[must_use]
    pub fn round_to_minor_units(self) -> i64 {
        let step = self.currency.nanos_per_minor_unit();
        let whole = self.nanos.checked_div(step).unwrap_or(0);
        let rest = self.nanos.checked_rem(step).unwrap_or(0);
        let doubled = rest.unsigned_abs().saturating_mul(2);
        if doubled < step.unsigned_abs() {
            whole
        } else if self.nanos.is_negative() {
            whole.saturating_sub(1)
        } else {
            whole.saturating_add(1)
        }
    }

    /// Rounds a list of amounts to the minor unit so that the rounded amounts
    /// add up to the rounded total, as a report shows them. Each is at most
    /// one minor unit from rounding it alone.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the amounts are not all in
    /// `currency`, or [`MoneyError::Overflow`].
    pub fn round_all_to_minor_units(
        amounts: &[Self],
        currency: Currency,
    ) -> Result<Vec<i64>, MoneyError> {
        if let Some(other) = amounts.iter().find(|amount| amount.currency != currency) {
            return Err(MoneyError::CurrencyMismatch {
                left: currency,
                right: other.currency,
            });
        }
        let step = i128::from(currency.nanos_per_minor_unit());
        running_round(
            amounts.iter().map(|amount| i128::from(amount.nanos)),
            |exact| divide_rounded(exact, step),
        )?
        .into_iter()
        .map(|minor| i64::try_from(minor).map_err(|_| MoneyError::Overflow))
        .collect()
    }

    /// Divides the amount into parts in proportion to `weights`, with the
    /// largest-remainder method: each part gets its share rounded towards
    /// zero, then the nano-units left over go one each to the parts with the
    /// largest remainders, ties to the earlier part. The parts always add up
    /// to the amount exactly, and no part is more than one nano-unit from its
    /// exact share.
    ///
    /// # Errors
    ///
    /// [`MoneyError::NoWeights`] if there are no weights or they are all
    /// zero, or [`MoneyError::Overflow`] if the weights add up to more than
    /// `u64` holds.
    pub fn allocate(self, weights: &[u64]) -> Result<Vec<Self>, MoneyError> {
        let total_weight = weights
            .iter()
            .try_fold(0_u64, |sum, weight| sum.checked_add(*weight))
            .ok_or(MoneyError::Overflow)?;
        if total_weight == 0 {
            return Err(MoneyError::NoWeights);
        }
        let total_weight = i128::from(total_weight);
        let magnitude = i128::from(self.nanos.unsigned_abs());

        // (position, share rounded towards zero, remainder)
        let mut parts = weights
            .iter()
            .enumerate()
            .map(|(position, weight)| {
                let scaled = magnitude
                    .checked_mul(i128::from(*weight))
                    .ok_or(MoneyError::Overflow)?;
                let share = scaled
                    .checked_div(total_weight)
                    .ok_or(MoneyError::Overflow)?;
                let remainder = scaled
                    .checked_rem(total_weight)
                    .ok_or(MoneyError::Overflow)?;
                Ok((position, share, remainder))
            })
            .collect::<Result<Vec<_>, MoneyError>>()?;

        let handed_out = parts
            .iter()
            .try_fold(0_i128, |sum, (_, share, _)| sum.checked_add(*share))
            .ok_or(MoneyError::Overflow)?;
        let left_over = magnitude
            .checked_sub(handed_out)
            .and_then(|left| usize::try_from(left).ok())
            .ok_or(MoneyError::Overflow)?;

        parts.sort_by(|a, b| match b.2.cmp(&a.2) {
            Ordering::Equal => a.0.cmp(&b.0),
            other => other,
        });
        for part in parts.iter_mut().take(left_over) {
            part.1 = part.1.checked_add(1).ok_or(MoneyError::Overflow)?;
        }
        parts.sort_by_key(|part| part.0);

        parts
            .into_iter()
            .map(|(_, share, _)| {
                let signed = if self.nanos.is_negative() {
                    share.checked_neg().ok_or(MoneyError::Overflow)?
                } else {
                    share
                };
                i64::try_from(signed)
                    .map(|nanos| Self::from_nanos(nanos, self.currency))
                    .map_err(|_| MoneyError::Overflow)
            })
            .collect()
    }

    fn same_currency(self, other: Self) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

impl fmt::Display for Money {
    /// Shows the amount rounded to the minor unit, such as `12.35 EUR`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minor = self.round_to_minor_units();
        write!(
            f,
            "{} {}",
            self.currency.format_minor_units(minor),
            self.currency
        )
    }
}

/// Turns exact running totals into parts that add up to the rounded total.
///
/// Each part is the rounded running total after it minus the rounded running
/// total before it. So the parts always add up to the rounded total, and no
/// part is more than one rounding step away from its own rounded value. It
/// is used to convert lines into another currency and to show them in cents.
fn running_round(
    values: impl IntoIterator<Item = i128>,
    round: impl Fn(i128) -> Option<i128>,
) -> Result<Vec<i128>, MoneyError> {
    let mut exact = 0_i128;
    let mut rounded_before = 0_i128;
    values
        .into_iter()
        .map(|value| {
            exact = exact.checked_add(value).ok_or(MoneyError::Overflow)?;
            let rounded = round(exact).ok_or(MoneyError::Overflow)?;
            let part = rounded
                .checked_sub(rounded_before)
                .ok_or(MoneyError::Overflow)?;
            rounded_before = rounded;
            Ok(part)
        })
        .collect()
}

/// `numerator / denominator` for a denominator above zero, rounded half away
/// from zero. `None` if the denominator is zero.
fn divide_rounded(numerator: i128, denominator: i128) -> Option<i128> {
    let whole = numerator.checked_div(denominator)?;
    let rest = numerator.checked_rem(denominator)?;
    if rest.unsigned_abs().checked_mul(2)? < denominator.unsigned_abs() {
        Some(whole)
    } else if numerator.is_negative() {
        whole.checked_sub(1)
    } else {
        whole.checked_add(1)
    }
}

/// Reads a decimal number such as `-12.345` as billionths: the format of
/// amounts and exchange rates in CSV files and the API.
fn parse_billionths(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (unsigned, ""),
    };
    let fraction_digits = u32::try_from(fraction.len()).ok()?;
    if whole.is_empty() {
        return None;
    }
    let digits = whole
        .chars()
        .chain(fraction.chars())
        .try_fold(0_i64, |sum, digit| {
            let digit = i64::from(digit.to_digit(10)?);
            sum.checked_mul(10)?.checked_add(digit)
        })?;
    let scale = 10_i64.checked_pow(NANO_DIGITS.checked_sub(fraction_digits)?)?;
    let magnitude = digits.checked_mul(scale)?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// Units of one currency per unit of another, such as 0.92 EUR per USD, to
/// nine decimal places.
///
/// A bill in another currency than the reporting currency keeps its own
/// amounts and the exchange rate of its date. Converting rounds each amount
/// half away from zero to the nano-unit; [`ExchangeRate::convert_all`]
/// converts a list so that the converted parts add up to the converted total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExchangeRate {
    from: Currency,
    to: Currency,
    /// Billionths of a unit of `to` per unit of `from`.
    nanos: i64,
}

impl ExchangeRate {
    /// A rate written as a decimal number, such as `"0.9234"` EUR per USD.
    ///
    /// # Errors
    ///
    /// [`MoneyError::InvalidRate`] unless the rate is a decimal number above
    /// zero with at most nine digits after the point.
    pub fn new(from: Currency, to: Currency, rate: &str) -> Result<Self, MoneyError> {
        match parse_billionths(rate) {
            Some(nanos) if nanos > 0 => Ok(Self { from, to, nanos }),
            _ => Err(MoneyError::InvalidRate(rate.to_owned())),
        }
    }

    /// The rate of a currency to itself: converting changes nothing.
    #[must_use]
    pub const fn identity(currency: Currency) -> Self {
        Self {
            from: currency,
            to: currency,
            nanos: NANOS_PER_UNIT,
        }
    }

    /// The currency amounts are converted from.
    #[must_use]
    pub const fn from(self) -> Currency {
        self.from
    }

    /// The currency amounts are converted to.
    #[must_use]
    pub const fn to(self) -> Currency {
        self.to
    }

    /// Billionths of a unit of [`to`](Self::to) per unit of
    /// [`from`](Self::from).
    #[must_use]
    pub const fn nanos(self) -> i64 {
        self.nanos
    }

    /// Converts one amount, rounding half away from zero to the nano-unit.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the amount is not in the rate's
    /// `from` currency, or [`MoneyError::Overflow`].
    pub fn convert(self, amount: Money) -> Result<Money, MoneyError> {
        self.convert_all(&[amount])?
            .into_iter()
            .next()
            .ok_or(MoneyError::Overflow)
    }

    /// Converts a list of amounts so that the converted amounts add up to the
    /// converted total exactly. Each is within one nano-unit of converting it
    /// alone.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if an amount is not in the rate's
    /// `from` currency, or [`MoneyError::Overflow`].
    pub fn convert_all(self, amounts: &[Money]) -> Result<Vec<Money>, MoneyError> {
        if let Some(other) = amounts.iter().find(|amount| amount.currency != self.from) {
            return Err(MoneyError::CurrencyMismatch {
                left: self.from,
                right: other.currency,
            });
        }
        let rate = i128::from(self.nanos);
        let unit = i128::from(NANOS_PER_UNIT);
        running_round(
            amounts.iter().map(|amount| i128::from(amount.nanos)),
            |exact| divide_rounded(exact.checked_mul(rate)?, unit),
        )?
        .into_iter()
        .map(|nanos| {
            i64::try_from(nanos)
                .map(|nanos| Money::from_nanos(nanos, self.to))
                .map_err(|_| MoneyError::Overflow)
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn eur(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::EUR)
    }

    #[test]
    fn currency_codes_are_three_capital_letters() {
        assert_eq!(Currency::new("EUR").unwrap(), Currency::EUR);
        assert!(Currency::new("eur").is_err());
        assert!(Currency::new("EURO").is_err());
        assert!(Currency::new("€").is_err());
        assert_eq!(Currency::new("JPY").unwrap().minor_unit_digits(), 0);
        assert_eq!(Currency::new("KWD").unwrap().minor_unit_digits(), 3);
        assert_eq!(Currency::new("CLF").unwrap().minor_unit_digits(), 4);
        assert_eq!(Currency::EUR.minor_unit_digits(), 2);
        assert_eq!(format!("{:?}", Currency::EUR), "EUR");
        assert_eq!(Currency::USD.to_string(), "USD");
    }

    #[test]
    fn currency_reads_and_writes_as_its_code() {
        let json = serde_json::to_string(&Currency::USD).unwrap();
        assert_eq!(json, "\"USD\"");
        assert_eq!(
            serde_json::from_str::<Currency>(&json).unwrap(),
            Currency::USD
        );
        assert!(serde_json::from_str::<Currency>("\"usd\"").is_err());
    }

    #[test]
    fn minor_units_convert_to_nano_units() {
        assert_eq!(
            Money::from_minor_units(1234, Currency::EUR)
                .unwrap()
                .nanos(),
            12_340_000_000
        );
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(
            Money::from_minor_units(5, yen).unwrap().nanos(),
            5_000_000_000
        );
        assert_eq!(
            Money::from_minor_units(i64::MAX, Currency::EUR),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn adding_needs_one_currency_and_never_wraps() {
        assert_eq!(eur(2).checked_add(eur(3)).unwrap(), eur(5));
        assert_eq!(eur(2).checked_sub(eur(3)).unwrap(), eur(-1));
        assert!(matches!(
            eur(1).checked_add(Money::from_nanos(1, Currency::USD)),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        assert_eq!(eur(i64::MAX).checked_add(eur(1)), Err(MoneyError::Overflow));
        assert_eq!(eur(i64::MIN).checked_neg(), Err(MoneyError::Overflow));
        assert_eq!(
            Money::sum([eur(1), eur(2), eur(3)], Currency::EUR).unwrap(),
            eur(6)
        );
        assert_eq!(Money::sum([], Currency::EUR).unwrap(), eur(0));
        assert!(eur(0).is_zero());
        assert!(!eur(-1).is_zero());
        assert!(!eur(1).is_zero());
    }

    #[test]
    fn a_ratio_of_an_amount_rounds_half_away_from_zero() {
        assert_eq!(
            eur(750_000_000_000).checked_mul_ratio(31, 100).unwrap(),
            eur(232_500_000_000)
        );
        assert_eq!(eur(3).checked_mul_ratio(1, 2).unwrap(), eur(2));
        assert_eq!(eur(-3).checked_mul_ratio(1, 2).unwrap(), eur(-2));
        assert_eq!(eur(1).checked_mul_ratio(1, 3).unwrap(), eur(0));
        assert_eq!(eur(10).checked_mul_ratio(12_000, 10_000).unwrap(), eur(12));
        assert_eq!(eur(10).checked_mul_ratio(0, 7).unwrap(), eur(0));
        assert_eq!(eur(10).checked_mul_ratio(1, 0), Err(MoneyError::NoWeights));
        assert_eq!(
            eur(i64::MAX).checked_mul_ratio(2, 1),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            eur(i64::MAX).checked_mul_ratio(1, 1).unwrap(),
            eur(i64::MAX)
        );
    }

    #[test]
    fn rounding_to_cents_goes_half_away_from_zero() {
        assert_eq!(eur(4_999_999).round_to_minor_units(), 0);
        assert_eq!(eur(5_000_000).round_to_minor_units(), 1);
        assert_eq!(eur(-5_000_000).round_to_minor_units(), -1);
        assert_eq!(eur(-4_999_999).round_to_minor_units(), 0);
        assert_eq!(eur(12_345_000_000).round_to_minor_units(), 1235);
    }

    #[test]
    fn display_shows_the_minor_unit() {
        assert_eq!(eur(12_345_000_000).to_string(), "12.35 EUR");
        assert_eq!(eur(-50_000_000).to_string(), "-0.05 EUR");
        assert_eq!(eur(0).to_string(), "0.00 EUR");
        assert_eq!(eur(-4_000_000).to_string(), "0.00 EUR");
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(Money::from_nanos(1_500_000_000, yen).to_string(), "2 JPY");
    }

    #[test]
    fn a_split_gives_left_over_units_to_the_largest_remainders() {
        let parts = eur(100).allocate(&[1, 1, 1]).unwrap();
        assert_eq!(parts, vec![eur(34), eur(33), eur(33)]);

        let parts = eur(-100).allocate(&[1, 1, 1]).unwrap();
        assert_eq!(parts, vec![eur(-34), eur(-33), eur(-33)]);

        let parts = eur(10).allocate(&[2, 5, 3]).unwrap();
        assert_eq!(parts, vec![eur(2), eur(5), eur(3)]);

        let parts = eur(7).allocate(&[1, 0, 3]).unwrap();
        assert_eq!(parts, vec![eur(2), eur(0), eur(5)]);
    }

    #[test]
    fn a_split_needs_a_weight() {
        assert_eq!(eur(1).allocate(&[]), Err(MoneyError::NoWeights));
        assert_eq!(eur(1).allocate(&[0, 0]), Err(MoneyError::NoWeights));
        assert_eq!(eur(1).allocate(&[u64::MAX, 1]), Err(MoneyError::Overflow));
    }

    #[test]
    fn extreme_amounts_split_without_overflow() {
        let parts = eur(i64::MIN).allocate(&[1]).unwrap();
        assert_eq!(parts, vec![eur(i64::MIN)]);
        let parts = eur(i64::MAX).allocate(&[1 << 62, 1 << 62]).unwrap();
        assert_eq!(Money::sum(parts, Currency::EUR).unwrap(), eur(i64::MAX));
    }

    #[test]
    fn amounts_read_from_decimal_text() {
        assert_eq!(
            Money::parse("12.34", Currency::EUR).unwrap(),
            eur(12_340_000_000)
        );
        assert_eq!(
            Money::parse(" -0.000000001 ", Currency::EUR).unwrap(),
            eur(-1)
        );
        assert_eq!(
            Money::parse("+7", Currency::EUR).unwrap(),
            eur(7_000_000_000)
        );
        assert_eq!(Money::parse("0", Currency::EUR).unwrap(), eur(0));
        assert_eq!(
            Money::parse("9223372036.854775807", Currency::EUR).unwrap(),
            eur(i64::MAX)
        );
        for bad in [
            "",
            "-",
            ".5",
            "12.",
            "1.2.3",
            "1,5",
            "12 EUR",
            "0.0000000001",
            "1e3",
            "--1",
            "9223372036.854775808",
            "92233720368",
        ] {
            assert_eq!(
                Money::parse(bad, Currency::EUR),
                Err(MoneyError::InvalidAmount(bad.to_owned())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn minor_units_are_written_with_the_currency_digits() {
        assert_eq!(Currency::EUR.format_minor_units(1235), "12.35");
        assert_eq!(Currency::EUR.format_minor_units(-5), "-0.05");
        assert_eq!(Currency::EUR.format_minor_units(0), "0.00");
        assert_eq!(Currency::new("JPY").unwrap().format_minor_units(-12), "-12");
        assert_eq!(
            Currency::new("KWD").unwrap().format_minor_units(1005),
            "1.005"
        );
    }

    #[test]
    fn rounded_lines_add_up_to_the_rounded_total() {
        // Three thirds of a cent each round to zero alone, but together they
        // are one cent.
        let third = eur(3_333_334);
        let rounded =
            Money::round_all_to_minor_units(&[third, third, third], Currency::EUR).unwrap();
        assert_eq!(rounded, vec![0, 1, 0]);
        assert_eq!(
            Money::round_all_to_minor_units(&[eur(-5_000_000), eur(-5_000_000)], Currency::EUR)
                .unwrap(),
            vec![-1, 0]
        );
        assert_eq!(
            Money::round_all_to_minor_units(&[eur(4_999_999)], Currency::EUR).unwrap(),
            vec![0]
        );
        assert_eq!(
            Money::round_all_to_minor_units(&[], Currency::EUR).unwrap(),
            Vec::<i64>::new()
        );
        assert!(matches!(
            Money::round_all_to_minor_units(&[Money::zero(Currency::USD)], Currency::EUR),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn exchange_rates_are_decimal_numbers_above_zero() {
        let rate = ExchangeRate::new(Currency::USD, Currency::EUR, "0.92").unwrap();
        assert_eq!(rate.from(), Currency::USD);
        assert_eq!(rate.to(), Currency::EUR);
        assert_eq!(rate.nanos(), 920_000_000);
        assert!(ExchangeRate::new(Currency::USD, Currency::EUR, "0.000000001").is_ok());
        for bad in ["0", "-1", "0.0", "", "abc"] {
            assert_eq!(
                ExchangeRate::new(Currency::USD, Currency::EUR, bad),
                Err(MoneyError::InvalidRate(bad.to_owned()))
            );
        }
        assert_eq!(
            ExchangeRate::identity(Currency::EUR).nanos(),
            NANOS_PER_UNIT
        );
    }

    #[test]
    fn converting_rounds_half_away_from_zero() {
        let usd = |nanos| Money::from_nanos(nanos, Currency::USD);
        let half = ExchangeRate::new(Currency::USD, Currency::EUR, "0.5").unwrap();
        assert_eq!(half.convert(usd(3)).unwrap(), eur(2));
        assert_eq!(half.convert(usd(-3)).unwrap(), eur(-2));
        assert_eq!(half.convert(usd(2)).unwrap(), eur(1));
        let rate = ExchangeRate::new(Currency::USD, Currency::EUR, "0.92").unwrap();
        assert_eq!(
            rate.convert(usd(100_000_000_000)).unwrap(),
            eur(92_000_000_000)
        );
        assert!(matches!(
            rate.convert(eur(1)),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        let large = ExchangeRate::new(Currency::USD, Currency::EUR, "2").unwrap();
        assert_eq!(large.convert(usd(i64::MAX)), Err(MoneyError::Overflow));
        let same = ExchangeRate::identity(Currency::EUR);
        assert_eq!(same.convert(eur(-7)).unwrap(), eur(-7));
    }

    #[test]
    fn converted_parts_add_up_to_the_converted_total() {
        let usd = |nanos| Money::from_nanos(nanos, Currency::USD);
        let half = ExchangeRate::new(Currency::USD, Currency::EUR, "0.5").unwrap();
        // Alone each 1 nano-unit converts to 1 (0.5 rounds up), which would
        // add up to 3 instead of 2.
        let parts = half.convert_all(&[usd(1), usd(1), usd(1), usd(1)]).unwrap();
        assert_eq!(parts, vec![eur(1), eur(0), eur(1), eur(0)]);
        assert!(matches!(
            half.convert_all(&[usd(1), eur(1)]),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
    }

    proptest! {
        #[test]
        fn split_parts_add_up_to_the_whole(
            nanos in any::<i64>(),
            weights in prop::collection::vec(0_u64..1_000_000, 1..20),
        ) {
            prop_assume!(weights.iter().any(|weight| *weight > 0));
            let parts = eur(nanos).allocate(&weights).unwrap();
            prop_assert_eq!(parts.len(), weights.len());
            prop_assert_eq!(Money::sum(parts.iter().copied(), Currency::EUR).unwrap(), eur(nanos));
        }

        #[test]
        fn split_parts_are_within_one_nano_unit_of_their_share(
            nanos in any::<i64>(),
            weights in prop::collection::vec(0_u64..1_000_000, 1..20),
        ) {
            prop_assume!(weights.iter().any(|weight| *weight > 0));
            let total: u64 = weights.iter().sum();
            let parts = eur(nanos).allocate(&weights).unwrap();
            for (part, weight) in parts.iter().zip(&weights) {
                let exact_times_total = i128::from(nanos).checked_mul(i128::from(*weight)).unwrap();
                let part_times_total = i128::from(part.nanos()).checked_mul(i128::from(total)).unwrap();
                let off = exact_times_total.checked_sub(part_times_total).unwrap();
                prop_assert!(off.abs() < i128::from(total));
            }
        }

        #[test]
        fn converted_parts_add_up_and_stay_close(
            amounts in prop::collection::vec(-1_000_000_000_000_i64..1_000_000_000_000, 0..20),
            rate in 1_i64..5_000_000_000,
        ) {
            let usd: Vec<Money> = amounts.iter().map(|nanos| Money::from_nanos(*nanos, Currency::USD)).collect();
            let rate = ExchangeRate { from: Currency::USD, to: Currency::EUR, nanos: rate };
            let parts = rate.convert_all(&usd).unwrap();
            let total = Money::sum(usd.iter().copied(), Currency::USD).unwrap();
            prop_assert_eq!(Money::sum(parts.iter().copied(), Currency::EUR).unwrap(), rate.convert(total).unwrap());
            for (part, amount) in parts.iter().zip(&usd) {
                let alone = rate.convert(*amount).unwrap();
                prop_assert!(part.nanos().checked_sub(alone.nanos()).unwrap().abs() <= 1);
            }
        }

        #[test]
        fn rounded_lines_add_up_and_stay_close(
            amounts in prop::collection::vec(-1_000_000_000_000_i64..1_000_000_000_000, 0..20),
        ) {
            let lines: Vec<Money> = amounts.iter().map(|nanos| eur(*nanos)).collect();
            let rounded = Money::round_all_to_minor_units(&lines, Currency::EUR).unwrap();
            let total = Money::sum(lines.iter().copied(), Currency::EUR).unwrap();
            prop_assert_eq!(rounded.iter().sum::<i64>(), total.round_to_minor_units());
            for (minor, line) in rounded.iter().zip(&lines) {
                prop_assert!(minor.checked_sub(line.round_to_minor_units()).unwrap().abs() <= 1);
            }
        }

        #[test]
        fn amounts_read_back_what_display_writes(nanos in any::<i64>()) {
            let minor = eur(nanos).round_to_minor_units();
            let text = Currency::EUR.format_minor_units(minor);
            prop_assert_eq!(Money::parse(&text, Currency::EUR).map(Money::round_to_minor_units), Ok(minor));
        }

        #[test]
        fn rounding_is_never_more_than_half_a_minor_unit_off(nanos in any::<i64>()) {
            let minor = eur(nanos).round_to_minor_units();
            let shown = i128::from(minor).checked_mul(10_000_000).unwrap();
            let difference = i128::from(nanos).checked_sub(shown).unwrap();
            prop_assert!(difference.abs() <= 5_000_000);
        }
    }
}
