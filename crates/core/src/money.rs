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

    /// The same amount with the opposite sign, as an adjustment books it.
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
        let digits = self.currency.minor_unit_digits();
        let sign = if minor.is_negative() { "-" } else { "" };
        let magnitude = minor.unsigned_abs();
        let scale = 10_u64.checked_pow(digits).unwrap_or(1);
        let units = magnitude.checked_div(scale).unwrap_or(0);
        let fraction = magnitude.checked_rem(scale).unwrap_or(0);
        if digits == 0 {
            write!(f, "{sign}{units} {}", self.currency)
        } else {
            let width = usize::try_from(digits).map_err(|_| fmt::Error)?;
            write!(f, "{sign}{units}.{fraction:0width$} {}", self.currency)
        }
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
        fn rounding_is_never_more_than_half_a_minor_unit_off(nanos in any::<i64>()) {
            let minor = eur(nanos).round_to_minor_units();
            let shown = i128::from(minor).checked_mul(10_000_000).unwrap();
            let difference = i128::from(nanos).checked_sub(shown).unwrap();
            prop_assert!(difference.abs() <= 5_000_000);
        }
    }
}
