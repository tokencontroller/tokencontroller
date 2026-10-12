//! Exchange rates. An organization computes in one reporting currency. A bill
//! in another currency keeps its own amounts and the rate of its date, and a
//! report can be shown in another currency at the rate of the period's last
//! day (docs: Account > Bills, glossary: Reporting currency).
//!
//! Rates are reference rates per euro, as the European Central Bank
//! publishes them, kept per day in `exchange_rates` (ADR 0019). A rate
//! between two other currencies goes through the euro. Converting itself is
//! the core's [`ExchangeRate`].

use std::collections::BTreeMap;

use sqlx::PgExecutor;
use time::Date;
use tokencontroller_core::{Currency, ExchangeRate, Money, MoneyError};

use super::amount::{AmountError, RateNumber, exchange_rate};

/// How far back a missing day may borrow the last known rate: weekends and
/// bank holidays have none.
const MAX_DAYS_BACK: i64 = 10;

/// Why an amount could not be converted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RateError {
    #[error(
        "there is no exchange rate from {from} to {to} for {day} yet: it arrives with the daily rate refresh, or a controller can enter it"
    )]
    Missing {
        from: Currency,
        to: Currency,
        day: Date,
    },
    #[error(transparent)]
    Amount(#[from] AmountError),
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// The euro reference rate of `currency` on `day`, or on the closest day
/// before it with one.
///
/// # Errors
///
/// A database error.
pub async fn per_euro<'e, E>(
    db: E,
    currency: Currency,
    day: Date,
) -> Result<Option<RateNumber>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    if currency == Currency::EUR {
        return Ok(Some(RateNumber::ONE));
    }
    let earliest = day
        .checked_sub(time::Duration::days(MAX_DAYS_BACK))
        .unwrap_or(day);
    let nanos = sqlx::query_scalar!(
        r#"SELECT per_euro_nanos FROM exchange_rates
           WHERE currency = $1 AND day <= $2 AND day >= $3
           ORDER BY day DESC LIMIT 1"#,
        currency.code(),
        day,
        earliest,
    )
    .fetch_optional(db)
    .await?;
    Ok(nanos.and_then(|nanos| RateNumber::from_nanos(nanos).ok()))
}

/// The rate from `from` to `to` on `day`.
///
/// # Errors
///
/// [`RateError::Missing`] when a rate for that day is not known, or a
/// database error.
pub async fn between(
    db: &sqlx::PgPool,
    from: Currency,
    to: Currency,
    day: Date,
) -> Result<Result<ExchangeRate, RateError>, sqlx::Error> {
    if from == to {
        return Ok(Ok(ExchangeRate::identity(from)));
    }
    let missing = RateError::Missing { from, to, day };
    let Some(from_per_euro) = per_euro(db, from, day).await? else {
        return Ok(Err(missing));
    };
    let Some(to_per_euro) = per_euro(db, to, day).await? else {
        return Ok(Err(missing));
    };
    Ok(from_per_euro
        .inverse()
        .and_then(|euro_per_from| euro_per_from.then(to_per_euro))
        .and_then(|number| exchange_rate(from, to, number))
        .map_err(RateError::from))
}

/// Stores a reference rate. A rate already stored for that day is replaced.
///
/// # Errors
///
/// A database error.
pub async fn store<'e, E>(
    db: E,
    currency: Currency,
    day: Date,
    per_euro: RateNumber,
) -> Result<(), sqlx::Error>
where
    E: PgExecutor<'e>,
{
    if currency == Currency::EUR {
        return Ok(());
    }
    sqlx::query!(
        r#"INSERT INTO exchange_rates (day, currency, per_euro_nanos) VALUES ($1, $2, $3)
           ON CONFLICT (currency, day) DO UPDATE SET per_euro_nanos = EXCLUDED.per_euro_nanos"#,
        day,
        currency.code(),
        per_euro.nanos(),
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Converts amounts of one period. Token entries charged by a provider whose
/// bill is in their currency convert into the reporting currency at that
/// bill's rate, so the cost sheet adds up to the bill; everything else
/// converts at the rate of the period's last day.
#[derive(Debug, Clone)]
pub struct Converter {
    to: Currency,
    day: Date,
    rates: BTreeMap<(Currency, Currency), ExchangeRate>,
    by_bill: BTreeMap<(String, Currency), ExchangeRate>,
}

impl Converter {
    /// A converter into the reporting currency `to`, for figures dated
    /// `day`.
    #[must_use]
    pub const fn new(to: Currency, day: Date) -> Self {
        Self {
            to,
            day,
            rates: BTreeMap::new(),
            by_bill: BTreeMap::new(),
        }
    }

    /// The reporting currency.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.to
    }

    /// The day figures convert at.
    #[must_use]
    pub const fn day(&self) -> Date {
        self.day
    }

    /// Uses `rate` for amounts in its currency.
    pub fn with_rate(&mut self, rate: ExchangeRate) {
        self.rates.insert((rate.from(), rate.to()), rate);
    }

    /// Uses `rate` for amounts in its currency charged by `provider`.
    pub fn with_bill_rate(&mut self, provider: &str, rate: ExchangeRate) {
        if rate.to() == self.to {
            self.by_bill
                .insert((provider.to_owned(), rate.from()), rate);
        }
    }

    /// Every rate of the period's day in use, for keeping with a closed
    /// period.
    #[must_use]
    pub fn rates(&self) -> Vec<ExchangeRate> {
        self.rates.values().copied().collect()
    }

    /// The rate of the period's day from `from` to `to`.
    ///
    /// # Errors
    ///
    /// [`RateError::Missing`] when it is not loaded.
    pub fn rate(&self, from: Currency, to: Currency) -> Result<ExchangeRate, RateError> {
        if from == to {
            return Ok(ExchangeRate::identity(from));
        }
        self.rates
            .get(&(from, to))
            .copied()
            .ok_or(RateError::Missing {
                from,
                to,
                day: self.day,
            })
    }

    /// `amount` in the reporting currency.
    ///
    /// # Errors
    ///
    /// [`RateError::Missing`] when no rate for its currency is known.
    pub fn convert(&self, provider: Option<&str>, amount: Money) -> Result<Money, RateError> {
        let from = amount.currency();
        let rate =
            match provider.and_then(|provider| self.by_bill.get(&(provider.to_owned(), from))) {
                Some(rate) => *rate,
                None => self.rate(from, self.to)?,
            };
        Ok(rate.convert(amount)?)
    }

    /// `amount` in `to` at the period's day.
    ///
    /// # Errors
    ///
    /// [`RateError::Missing`] when the rate is not loaded.
    pub fn convert_to(&self, amount: Money, to: Currency) -> Result<Money, RateError> {
        Ok(self.rate(amount.currency(), to)?.convert(amount)?)
    }

    /// Loads the rate of the converter's day for every pair. Pairs without a
    /// known rate are left out; converting them fails with
    /// [`RateError::Missing`].
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn load_rates(
        &mut self,
        db: &sqlx::PgPool,
        pairs: &[(Currency, Currency)],
    ) -> Result<(), sqlx::Error> {
        for (from, to) in pairs {
            if from == to || self.rates.contains_key(&(*from, *to)) {
                continue;
            }
            if let Ok(rate) = between(db, *from, *to, self.day).await? {
                self.with_rate(rate);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(cents: i64) -> Money {
        Money::from_minor_units(cents, Currency::USD).unwrap()
    }

    fn eur(cents: i64) -> Money {
        Money::from_minor_units(cents, Currency::EUR).unwrap()
    }

    fn rate(from: Currency, to: Currency, number: &str) -> ExchangeRate {
        ExchangeRate::new(from, to, number).unwrap()
    }

    #[test]
    fn the_reporting_currency_needs_no_rate() {
        let converter = Converter::new(Currency::USD, date!(2026 - 09 - 30));
        assert_eq!(converter.convert(None, usd(100)).unwrap(), usd(100));
        assert_eq!(
            converter.convert_to(usd(100), Currency::USD).unwrap(),
            usd(100)
        );
    }

    #[test]
    fn a_bill_rate_wins_for_its_provider() {
        let mut converter = Converter::new(Currency::EUR, date!(2026 - 09 - 30));
        converter.with_rate(rate(Currency::USD, Currency::EUR, "0.9"));
        converter.with_bill_rate("anthropic", rate(Currency::USD, Currency::EUR, "0.8"));
        // A rate into another currency is not a bill rate for the report.
        converter.with_bill_rate(
            "openai",
            rate(Currency::USD, Currency::new("GBP").unwrap(), "0.7"),
        );
        assert_eq!(
            converter.convert(Some("anthropic"), usd(100)).unwrap(),
            eur(80)
        );
        assert_eq!(
            converter.convert(Some("openai"), usd(100)).unwrap(),
            eur(90)
        );
        assert_eq!(converter.convert(None, usd(100)).unwrap(), eur(90));
        assert_eq!(converter.rates().len(), 1);
    }

    #[test]
    fn a_missing_rate_says_which() {
        let converter = Converter::new(Currency::EUR, date!(2026 - 09 - 30));
        assert_eq!(
            converter.convert(None, usd(100)),
            Err(RateError::Missing {
                from: Currency::USD,
                to: Currency::EUR,
                day: date!(2026 - 09 - 30)
            })
        );
        assert!(converter.convert_to(eur(100), Currency::USD).is_err());
    }
}
