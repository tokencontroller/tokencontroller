//! Decimal text to and from nano-units, so amounts never pass through a float
//! (ADR 0004). Session files write costs as JSON numbers such as `0.0312`;
//! they are read here digit by digit.

use tokencontroller_core::{Currency, Money};

/// Nano-units in one unit, as in the core crate.
const NANO_DIGITS: usize = 9;

/// Reads a non-negative or negative decimal number such as `12`, `0.031` or
/// `1.5e-3` as nano-units. Digits past the ninth decimal are rounded half up.
/// Returns `None` for anything that is not a plain number or does not fit.
pub fn parse_nanos(text: &str) -> Option<i64> {
    let text = text.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole
        .bytes()
        .chain(fraction.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    // All digits, and where the decimal point sits after applying the exponent.
    let digits: Vec<u8> = whole
        .bytes()
        .chain(fraction.bytes())
        .map(|b| b.saturating_sub(b'0'))
        .collect();
    let point = i32::try_from(whole.len()).ok()?.checked_add(exponent)?;
    let nano_digits = i32::try_from(NANO_DIGITS).ok()?;
    let keep = point.checked_add(nano_digits)?;
    let mut nanos: i128 = 0;
    let mut position: i32 = 0;
    let mut round_up = false;
    for digit in &digits {
        if position < keep {
            nanos = nanos.checked_mul(10)?.checked_add(i128::from(*digit))?;
        } else if position == keep {
            round_up = *digit >= 5;
        }
        position = position.checked_add(1)?;
    }
    while position < keep {
        nanos = nanos.checked_mul(10)?;
        position = position.checked_add(1)?;
    }
    if round_up {
        nanos = nanos.checked_add(1)?;
    }
    let nanos = i64::try_from(nanos).ok()?;
    if negative {
        nanos.checked_neg()
    } else {
        Some(nanos)
    }
}

/// An amount in nano-units rounded to the currency's minor unit, as a decimal
/// string without the currency, such as `41.87`. This is how money goes into
/// JSON and CSV.
pub fn decimal(nanos: i64, currency: Currency) -> String {
    let text = Money::from_nanos(nanos, currency).to_string();
    // `Money` shows `41.87 USD`; keep the number.
    text.split_once(' ')
        .map_or(text.clone(), |(number, _)| number.to_owned())
}

/// An amount for people to read, such as `$41.87` or `€41.87`. Other
/// currencies show their code after the number.
pub fn display(nanos: i64, currency: Currency) -> String {
    let number = decimal(nanos, currency);
    let (sign, digits) = match number.strip_prefix('-') {
        Some(rest) => ("-", rest.to_owned()),
        None => ("", number.clone()),
    };
    if currency == Currency::USD {
        format!("{sign}${digits}")
    } else if currency == Currency::EUR {
        format!("{sign}€{digits}")
    } else {
        format!("{number} {currency}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_become_nanos_without_floats() {
        assert_eq!(parse_nanos("0"), Some(0));
        assert_eq!(parse_nanos("12"), Some(12_000_000_000));
        assert_eq!(parse_nanos("0.031"), Some(31_000_000));
        assert_eq!(parse_nanos("-1.5"), Some(-1_500_000_000));
        assert_eq!(parse_nanos("+2"), Some(2_000_000_000));
        assert_eq!(parse_nanos(".5"), Some(500_000_000));
        assert_eq!(parse_nanos("5."), Some(5_000_000_000));
        assert_eq!(parse_nanos("1.5e-3"), Some(1_500_000));
        assert_eq!(parse_nanos("3E2"), Some(300_000_000_000));
        assert_eq!(parse_nanos("2.5e-06"), Some(2_500));
    }

    #[test]
    fn digits_past_the_nano_round_half_up() {
        assert_eq!(parse_nanos("0.0000000014"), Some(1));
        assert_eq!(parse_nanos("0.0000000015"), Some(2));
        assert_eq!(parse_nanos("0.00000000049"), Some(0));
    }

    #[test]
    fn anything_else_is_not_a_number() {
        assert_eq!(parse_nanos(""), None);
        assert_eq!(parse_nanos("."), None);
        assert_eq!(parse_nanos("abc"), None);
        assert_eq!(parse_nanos("1.2.3"), None);
        assert_eq!(parse_nanos("1e"), None);
        assert_eq!(parse_nanos("99999999999"), None);
    }

    #[test]
    fn amounts_show_with_their_symbol() {
        assert_eq!(decimal(41_870_000_000, Currency::USD), "41.87");
        assert_eq!(display(41_870_000_000, Currency::USD), "$41.87");
        assert_eq!(display(-5_000_000_000, Currency::USD), "-$5.00");
        assert_eq!(display(9_750_000_000, Currency::EUR), "€9.75");
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(display(1_000_000_000_000, yen), "1000 JPY");
    }
}
