//! The European Central Bank's euro reference rates (ADR 0019): fetched
//! once a day and stored per day, so bills and reports convert between
//! currencies without anyone typing a rate. `TC_PRICES_REFRESH=off` stops
//! the refresh on instances without internet access; rates can still be
//! given on each bill.

use std::time::Duration;

use sqlx::PgPool;
use time::Date;
use time::macros::format_description;
use tokencontroller_core::Currency;

use super::amount::RateNumber;
use super::rates;
use crate::AppState;

/// The last 90 days of reference rates. Fetching more than one day fills
/// gaps after the service was down.
pub const RATES_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml";

/// The job's name in the job runner.
pub const JOB: &str = "exchange-rates";

/// How often the rates are fetched. The bank publishes once a working day,
/// around 16:00 Frankfurt time.
pub const JOB_EVERY: Duration = Duration::from_hours(6);

/// One day's rate of one currency, in units per euro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyRate {
    pub day: Date,
    pub currency: Currency,
    pub per_euro: RateNumber,
}

/// The value of `name="…"` or `name='…'` in a tag's text.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}="))?;
    let rest = tag.get(at.checked_add(name.len())?.checked_add(1)?..)?;
    let quote = rest
        .chars()
        .next()
        .filter(|quote| *quote == '"' || *quote == '\'')?;
    let value = rest.get(1..)?;
    value.split(quote).next()
}

/// Reads the bank's XML: `<Cube time="2026-10-09">` holding
/// `<Cube currency="USD" rate="1.0832"/>`. Anything it cannot read is
/// skipped.
#[must_use]
pub fn parse(xml: &str) -> Vec<DailyRate> {
    let day_format = format_description!("[year]-[month]-[day]");
    let mut day: Option<Date> = None;
    let mut found = Vec::new();
    for chunk in xml.split("<Cube").skip(1) {
        let Some(tag) = chunk.split('>').next() else {
            continue;
        };
        if let Some(time) = attribute(tag, "time") {
            day = Date::parse(time, &day_format).ok();
            continue;
        }
        let (Some(day), Some(code), Some(rate)) =
            (day, attribute(tag, "currency"), attribute(tag, "rate"))
        else {
            continue;
        };
        if let (Ok(currency), Ok(per_euro)) = (Currency::new(code), RateNumber::parse(rate)) {
            found.push(DailyRate {
                day,
                currency,
                per_euro,
            });
        }
    }
    found
}

/// Fetches the rates and stores them. Returns how many it stored.
///
/// # Errors
///
/// When the bank cannot be reached or the database fails.
pub async fn refresh(db: &PgPool, client: &reqwest::Client) -> anyhow::Result<usize> {
    let xml = client
        .get(RATES_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let found = parse(&xml);
    let mut tx = db.begin().await?;
    for rate in &found {
        rates::store(&mut *tx, rate.currency, rate.day, rate.per_euro).await?;
    }
    tx.commit().await?;
    Ok(found.len())
}

/// The `exchange-rates` job: [`refresh`] from the bank. It runs in the job
/// runner, so one replica fetches at a time.
///
/// # Errors
///
/// As [`refresh`].
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let client = crate::integrations::http::client()?;
    let stored = refresh(&state.db, &client).await?;
    Ok(format!("stored {stored} exchange rates"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn reads_both_of_the_banks_quote_styles() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01">
  <Cube>
    <Cube time="2026-10-09">
      <Cube currency="USD" rate="1.0832"/>
      <Cube currency="JPY" rate="161.42"/>
    </Cube>
    <Cube time='2026-10-08'>
      <Cube currency='USD' rate='1.0791'/>
      <Cube currency='usd' rate='1'/>
      <Cube currency='GBP' rate='zero'/>
    </Cube>
  </Cube>
</gesmes:Envelope>"#;
        let usd = Currency::new("USD").unwrap();
        let rates = parse(xml);
        assert_eq!(rates.len(), 3);
        assert_eq!(
            rates.first(),
            Some(&DailyRate {
                day: date!(2026 - 10 - 09),
                currency: usd,
                per_euro: RateNumber::parse("1.0832").unwrap(),
            })
        );
        assert_eq!(
            rates.get(2).map(|rate| (rate.day, rate.currency)),
            Some((date!(2026 - 10 - 08), usd))
        );
    }

    #[test]
    fn a_rate_before_any_day_is_skipped() {
        assert!(parse(r#"<Cube currency="USD" rate="1.08"/>"#).is_empty());
        assert!(parse("").is_empty());
    }
}
