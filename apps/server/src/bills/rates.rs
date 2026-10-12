//! Exchange rates. An organization computes in one reporting currency. A bill
//! in another currency keeps its own amounts and the rate of its date, and a
//! report can be shown in another currency at the rate of the period's last
//! day (docs: Account > Bills, glossary: Reporting currency).
//!
//! Rates are reference rates per euro, as the European Central Bank
//! publishes them, kept per day in `exchange_rates`. A rate between two other
//! currencies goes through the euro.

use std::collections::HashMap;

use sqlx::PgExecutor;
use time::Date;
use tokencontroller_core::{Currency, Money};

use super::amount::{AmountError, ExchangeRate};

/// How far back a missing day may borrow the last known rate: weekends and
/// bank holidays have none.
const MAX_DAYS_BACK: i64 = 10;

/// Why an amount could not be converted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RateError {
    #[error(
        "there is no exchange rate from {from} to {to} for {day} yet: it arrives with the daily rate refresh, or a controller can enter it"
    )]
    Missing { from: Currency, to: Currency, day: Date },
    #[error(transparent)]
    Amount(#[from] AmountError),
}

/// The euro reference rate of `currency` on `day`, or on the closest day
/// before it with one.
///
/// # Errors
///
/// A database error.
pub async fn per_euro<'e, E>(db: E, currency: Currency, day: Date) -> Result<Option<ExchangeRate>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    if currency == Currency::EUR {
        return Ok(Some(ExchangeRate::ONE));
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
    Ok(nanos.and_then(|nanos| ExchangeRate::from_nanos(nanos).ok()))
}

/// The rate from `from` to `to` on `day`: how many units of `to` one unit of
/// `from` is worth.
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
        return Ok(Ok(ExchangeRate::ONE));
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
        .map_err(RateError::from))
}

/// Stores a reference rate. A rate already stored for that day is replaced.
///
/// # Errors
///
/// A database error.
pub async fn store<'e, E>(db: E, currency: Currency, day: Date, per_euro: ExchangeRate) -> Result<(), sqlx::Error>
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

/// Converts amounts of one period into the reporting currency. Token entries
/// charged by a provider whose bill is in their currency convert at that
/// bill's rate, so the cost sheet adds up to the bill; everything else
/// converts at the rate of the period's last day.
#[derive(Debug, Clone)]
pub struct Converter {
    to: Currency,
    day: Date,
    by_currency: HashMap<Currency, ExchangeRate>,
    by_bill: HashMap<(String, Currency), ExchangeRate>,
}

impl Converter {
    /// A converter into `to`, for figures dated `day`.
    #[must_use]
    pub fn new(to: Currency, day: Date) -> Self {
        Self {
            to,
            day,
            by_currency: HashMap::new(),
            by_bill: HashMap::new(),
        }
    }

    /// The currency everything converts into.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.to
    }

    /// Uses `rate` for amounts in `from`.
    pub fn with_rate(&mut self, from: Currency, rate: ExchangeRate) {
        self.by_currency.insert(from, rate);
    }

    /// Uses `rate` for amounts in `from` charged by `provider`.
    pub fn with_bill_rate(&mut self, provider: &str, from: Currency, rate: ExchangeRate) {
        self.by_bill.insert((provider.to_owned(), from), rate);
    }

    /// The rates in use per currency, for keeping with a closed period.
    #[must_use]
    pub fn rates(&self) -> Vec<(Currency, ExchangeRate)> {
        let mut rates: Vec<_> = self.by_currency.iter().map(|(c, r)| (*c, *r)).collect();
        rates.sort_by_key(|(currency, _)| *currency);
        rates
    }

    /// `amount` in the reporting currency.
    ///
    /// # Errors
    ///
    /// [`RateError::Missing`] when no rate for its currency is known.
    pub fn convert(&self, provider: Option<&str>, amount: Money) -> Result<Money, RateError> {
        let from = amount.currency();
        if from == self.to {
            return Ok(amount);
        }
        let rate = provider
            .and_then(|provider| self.by_bill.get(&(provider.to_owned(), from)))
            .or_else(|| self.by_currency.get(&from))
            .ok_or(RateError::Missing {
                from,
                to: self.to,
                day: self.day,
            })?;
        Ok(rate.convert(amount, self.to)?)
    }

    /// Loads the rate of every currency in `currencies` for the converter's
    /// day. Currencies without a known rate are left out; converting them
    /// fails with [`RateError::Missing`].
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn load_rates(&mut self, db: &sqlx::PgPool, currencies: &[Currency]) -> Result<(), sqlx::Error> {
        for currency in currencies {
            if *currency == self.to || self.by_currency.contains_key(currency) {
                continue;
            }
            if let Ok(rate) = between(db, *currency, self.to, self.day).await? {
                self.by_currency.insert(*currency, rate);
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

    #[test]
    fn the_reporting_currency_needs_no_rate() {
        let converter = Converter::new(Currency::USD, date!(2026 - 09 - 30));
        assert_eq!(converter.convert(None, usd(100)).unwrap(), usd(100));
    }

    #[test]
    fn a_bill_rate_wins_for_its_provider() {
        let mut converter = Converter::new(Currency::EUR, date!(2026 - 09 - 30));
        converter.with_rate(Currency::USD, ExchangeRate::parse("0.9").unwrap());
        converter.with_bill_rate("anthropic", Currency::USD, ExchangeRate::parse("0.8").unwrap());
        let eur = |cents| Money::from_minor_units(cents, Currency::EUR).unwrap();
        assert_eq!(converter.convert(Some("anthropic"), usd(100)).unwrap(), eur(80));
        assert_eq!(converter.convert(Some("openai"), usd(100)).unwrap(), eur(90));
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
    }
}
