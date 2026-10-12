//! The arithmetic behind token forecasts: text embeddings as whole numbers,
//! how alike two of them are, and the median of the matched tickets' cost.
//! Floats are forbidden (ADR 0004), so an embedding is kept in millionths.

use tokencontroller_core::{Currency, Money, MoneyError};

/// Millionths in one: the fixed point an embedding is kept in.
const SCALE_DIGITS: i64 = 6;

/// The longest number text read as one embedding value.
const MAX_NUMBER_LEN: usize = 64;

/// A text embedding, each value in millionths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embedding(Vec<i32>);

/// Why an embedding could not be read or compared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmbeddingError {
    #[error("the embedding value {0:?} is not a number")]
    NotANumber(String),
    #[error("the embedding is empty")]
    Empty,
}

impl Embedding {
    /// An embedding from values already in millionths, as stored.
    ///
    /// # Errors
    ///
    /// [`EmbeddingError::Empty`] for no values.
    pub fn from_millionths(values: Vec<i32>) -> Result<Self, EmbeddingError> {
        if values.is_empty() {
            Err(EmbeddingError::Empty)
        } else {
            Ok(Self(values))
        }
    }

    /// Reads the decimal numbers an embeddings API returns, such as
    /// `-0.0071680755` or `1.25e-05`.
    ///
    /// # Errors
    ///
    /// [`EmbeddingError::NotANumber`] for anything that is not a decimal
    /// number between -2000 and 2000, [`EmbeddingError::Empty`] for no
    /// values.
    pub fn parse<I, S>(values: I) -> Result<Self, EmbeddingError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = values
            .into_iter()
            .map(|text| {
                parse_millionths(text.as_ref())
                    .ok_or_else(|| EmbeddingError::NotANumber(text.as_ref().to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_millionths(values)
    }

    /// The values in millionths, for storing.
    #[must_use]
    pub fn millionths(&self) -> &[i32] {
        &self.0
    }

    /// How alike two embeddings are, from 0 to 100: their cosine similarity
    /// in percent, rounded down, with opposite directions and an all-zero
    /// embedding counted as 0. `None` when they have different lengths.
    #[must_use]
    pub fn similarity(&self, other: &Self) -> Option<u8> {
        if self.0.len() != other.0.len() {
            return None;
        }
        let mut dot: i128 = 0;
        let mut left: i128 = 0;
        let mut right: i128 = 0;
        for (a, b) in self.0.iter().zip(&other.0) {
            let (a, b) = (i128::from(*a), i128::from(*b));
            dot = dot.checked_add(a.checked_mul(b)?)?;
            left = left.checked_add(a.checked_mul(a)?)?;
            right = right.checked_add(b.checked_mul(b)?)?;
        }
        if dot.is_negative() || dot == 0 {
            return Some(0);
        }
        let norms = u128::try_from(left.checked_mul(right)?).ok()?.isqrt();
        if norms == 0 {
            return None;
        }
        let percent = u128::try_from(dot)
            .ok()?
            .checked_mul(100)?
            .checked_div(norms)?
            .min(100);
        u8::try_from(percent).ok()
    }
}

/// A decimal number in millionths, truncated towards zero.
fn parse_millionths(text: &str) -> Option<i32> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_NUMBER_LEN {
        return None;
    }
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.chars().chain(fraction.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits: String = whole.chars().chain(fraction.chars()).collect();
    let fraction_len = i64::try_from(fraction.len()).ok()?;
    let shift = exponent
        .checked_sub(fraction_len)?
        .checked_add(SCALE_DIGITS)?;
    let digits = digits.trim_start_matches('0');
    let magnitude: i64 = if digits.is_empty() {
        0
    } else if shift.is_negative() {
        let keep = i64::try_from(digits.len()).ok()?.checked_add(shift)?;
        if keep <= 0 {
            0
        } else {
            let keep = usize::try_from(keep).ok()?;
            digits.get(..keep)?.parse().ok()?
        }
    } else {
        let base: i64 = digits.parse().ok()?;
        base.checked_mul(10_i64.checked_pow(u32::try_from(shift).ok()?)?)?
    };
    let signed = if negative {
        magnitude.checked_neg()?
    } else {
        magnitude
    };
    // An embedding value is near ±1; anything past ±2000 is not one.
    if signed.unsigned_abs() > 2_000_000_000 {
        return None;
    }
    i32::try_from(signed).ok()
}

/// The median of some amounts: the middle one, or for an even count the
/// exact half of the two middle ones added. `None` for no amounts.
///
/// # Errors
///
/// [`MoneyError`] for amounts in another currency than `currency`, or an
/// overflow.
pub fn median(amounts: &[Money], currency: Currency) -> Result<Option<Money>, MoneyError> {
    if let Some(other) = amounts.iter().find(|amount| amount.currency() != currency) {
        return Err(MoneyError::CurrencyMismatch {
            left: currency,
            right: other.currency(),
        });
    }
    let mut sorted: Vec<i64> = amounts.iter().map(|amount| amount.nanos()).collect();
    sorted.sort_unstable();
    let count = sorted.len();
    let Some(upper_index) = count.checked_div(2) else {
        return Ok(None);
    };
    let Some(upper) = sorted.get(upper_index).copied() else {
        return Ok(None);
    };
    if count.is_multiple_of(2) {
        let lower_index = upper_index.checked_sub(1).ok_or(MoneyError::Overflow)?;
        let lower = sorted.get(lower_index).copied().ok_or(MoneyError::Overflow)?;
        let both = Money::from_nanos(lower, currency)
            .checked_add(Money::from_nanos(upper, currency))?;
        let halves = both.allocate(&[1, 1])?;
        Ok(halves.into_iter().next())
    } else {
        Ok(Some(Money::from_nanos(upper, currency)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedding(values: &[&str]) -> Embedding {
        Embedding::parse(values.iter().copied()).unwrap()
    }

    #[test]
    fn reads_api_numbers_into_millionths() {
        assert_eq!(parse_millionths("-0.0071680755"), Some(-7168));
        assert_eq!(parse_millionths("0.5"), Some(500_000));
        assert_eq!(parse_millionths("1"), Some(1_000_000));
        assert_eq!(parse_millionths("1.25e-05"), Some(12));
        assert_eq!(parse_millionths("2.5E-1"), Some(250_000));
        assert_eq!(parse_millionths("3e2"), Some(300_000_000));
        assert_eq!(parse_millionths("1e-7"), Some(0));
        assert_eq!(parse_millionths("0"), Some(0));
        assert_eq!(parse_millionths(".5"), Some(500_000));
        assert_eq!(parse_millionths("+0.25"), Some(250_000));
        assert_eq!(parse_millionths("-0"), Some(0));
    }

    #[test]
    fn refuses_what_is_not_a_number() {
        for text in ["", "abc", "1.2.3", "--1", "1e", "NaN", "inf", ".", "1e999", "2001", "-2000.5"] {
            assert_eq!(parse_millionths(text), None, "{text}");
        }
        assert_eq!(parse_millionths("2000"), Some(2_000_000_000));
        assert_eq!(
            Embedding::parse(["0.1", "x"]),
            Err(EmbeddingError::NotANumber("x".to_owned()))
        );
        assert_eq!(Embedding::parse(Vec::<String>::new()), Err(EmbeddingError::Empty));
    }

    #[test]
    fn the_same_direction_is_fully_alike() {
        let a = embedding(&["0.6", "0.8"]);
        let b = embedding(&["0.3", "0.4"]);
        assert_eq!(a.similarity(&b), Some(100));
        assert_eq!(a.similarity(&a), Some(100));
    }

    #[test]
    fn right_angles_and_opposites_are_not_alike() {
        let a = embedding(&["1", "0"]);
        assert_eq!(a.similarity(&embedding(&["0", "1"])), Some(0));
        assert_eq!(a.similarity(&embedding(&["-1", "0"])), Some(0));
    }

    #[test]
    fn similarity_is_the_cosine_in_percent() {
        // cos 60° = 0.5
        let a = embedding(&["1", "0"]);
        let b = embedding(&["0.5", "0.8660254"]);
        assert_eq!(a.similarity(&b), Some(50));
        let c = embedding(&["0.7071068", "0.7071068"]);
        assert_eq!(a.similarity(&c), Some(70));
    }

    #[test]
    fn different_lengths_or_zero_cannot_compare() {
        let a = embedding(&["1", "0"]);
        assert_eq!(a.similarity(&embedding(&["1", "0", "0"])), None);
        let zero = embedding(&["0", "0"]);
        assert_eq!(zero.similarity(&zero), Some(0));
        assert_eq!(a.similarity(&zero), Some(0));
    }

    #[test]
    fn median_of_odd_and_even_counts() {
        let eur = |cents: i64| Money::from_minor_units(cents, Currency::EUR).unwrap();
        assert_eq!(median(&[], Currency::EUR).unwrap(), None);
        assert_eq!(median(&[eur(500)], Currency::EUR).unwrap(), Some(eur(500)));
        assert_eq!(
            median(&[eur(900), eur(100), eur(500)], Currency::EUR).unwrap(),
            Some(eur(500))
        );
        assert_eq!(
            median(&[eur(400), eur(100), eur(900), eur(600)], Currency::EUR).unwrap(),
            Some(eur(500))
        );
        let odd_nanos = [
            Money::from_nanos(1, Currency::EUR),
            Money::from_nanos(2, Currency::EUR),
        ];
        assert_eq!(
            median(&odd_nanos, Currency::EUR).unwrap(),
            Some(Money::from_nanos(2, Currency::EUR))
        );
    }

    #[test]
    fn median_refuses_mixed_currencies() {
        let usd = Money::from_minor_units(100, Currency::USD).unwrap();
        assert!(median(&[usd], Currency::EUR).is_err());
    }
}
