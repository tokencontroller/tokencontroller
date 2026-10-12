//! Bills and reconciliation: token entries plus residual equal the bill.

use serde::{Deserialize, Serialize};

use crate::money::{Currency, ExchangeRate, Money, MoneyError};
use crate::period::Period;
use crate::status::CostSource;

/// A bill that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BillError {
    #[error(
        "bill {bill}: gross {gross} minus discount {discount} minus credit {credit} is not net {net}"
    )]
    DoesNotAddUp {
        bill: String,
        gross: Money,
        discount: Money,
        credit: Money,
        net: Money,
    },
    #[error("bill {bill} is in {bill_currency}; it needs an exchange rate to {reporting_currency}")]
    NoExchangeRate {
        bill: String,
        bill_currency: Currency,
        reporting_currency: Currency,
    },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// A provider's figure for one period's metered usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    /// How the organization refers to this bill, such as
    /// `anthropic-console-2026-10`.
    pub id: String,
    pub provider: String,
    pub cost_source: CostSource,
    pub period: Period,
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
    /// To the organization's reporting currency, at the bill's date. Needed
    /// only when the bill is in another currency.
    #[serde(default)]
    pub exchange_rate: Option<ExchangeRate>,
    /// Counts the provider's restatements, from 1.
    #[serde(default = "first_revision")]
    pub revision: u32,
}

const fn first_revision() -> u32 {
    1
}

/// The check of one bill: what token entries explain, and the residual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciliation {
    /// What token entries explain, at list value.
    pub explained: Money,
    /// The part of the bill no token entry explains. Negative when token
    /// entries come to more than the bill.
    pub residual: Money,
}

impl Bill {
    /// The currency the provider charges in.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.gross.currency()
    }

    /// Checks that the amounts share one currency and that net is gross
    /// minus discount minus credit.
    ///
    /// # Errors
    ///
    /// [`BillError::DoesNotAddUp`], or [`BillError::Money`] for amounts in
    /// different currencies.
    pub fn check(&self) -> Result<(), BillError> {
        let computed = self
            .gross
            .checked_sub(self.discount)?
            .checked_sub(self.credit)?;
        if computed == self.net {
            Ok(())
        } else {
            Err(BillError::DoesNotAddUp {
                bill: self.id.clone(),
                gross: self.gross,
                discount: self.discount,
                credit: self.credit,
                net: self.net,
            })
        }
    }

    /// Reconciles the bill against the list values of the token entries
    /// charged on it. Token entries plus residual always equal gross; the
    /// discount and credit are their own lines.
    ///
    /// # Errors
    ///
    /// As [`Bill::check`], or [`BillError::Money`] if a list value is in
    /// another currency.
    pub fn reconcile<I>(&self, list_values: I) -> Result<Reconciliation, BillError>
    where
        I: IntoIterator<Item = Money>,
    {
        self.check()?;
        let explained = Money::sum(list_values, self.currency())?;
        Ok(Reconciliation {
            explained,
            residual: self.gross.checked_sub(explained)?,
        })
    }

    /// The rate that turns the bill's currency into the reporting currency.
    ///
    /// # Errors
    ///
    /// [`BillError::NoExchangeRate`] if the bill is in another currency and
    /// has no rate, or [`BillError::Money`] if its rate is between other
    /// currencies.
    pub fn rate_to(&self, reporting_currency: Currency) -> Result<ExchangeRate, BillError> {
        if self.currency() == reporting_currency {
            return Ok(ExchangeRate::identity(reporting_currency));
        }
        let rate = self
            .exchange_rate
            .ok_or_else(|| BillError::NoExchangeRate {
                bill: self.id.clone(),
                bill_currency: self.currency(),
                reporting_currency,
            })?;
        if rate.from() == self.currency() && rate.to() == reporting_currency {
            Ok(rate)
        } else {
            Err(BillError::Money(MoneyError::CurrencyMismatch {
                left: self.currency(),
                right: rate.from(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn bill() -> Bill {
        Bill {
            id: "anthropic-2026-10".to_owned(),
            provider: "anthropic".to_owned(),
            cost_source: CostSource::UsageAtApiRates,
            period: Period::parse("2026-10").unwrap(),
            gross: usd("2000"),
            discount: usd("200"),
            credit: usd("50"),
            net: usd("1750"),
            exchange_rate: None,
            revision: 1,
        }
    }

    #[test]
    fn token_entries_plus_residual_equal_the_bill() {
        let reconciliation = bill().reconcile([usd("1500"), usd("380")]).unwrap();
        assert_eq!(reconciliation.explained, usd("1880"));
        assert_eq!(reconciliation.residual, usd("120"));
        let over = bill().reconcile([usd("2100")]).unwrap();
        assert_eq!(over.residual, usd("-100"));
        let none = bill().reconcile([]).unwrap();
        assert_eq!(none.residual, usd("2000"));
    }

    #[test]
    fn net_must_be_gross_minus_discount_minus_credit() {
        assert_eq!(bill().check(), Ok(()));
        let mut wrong = bill();
        wrong.net = usd("1800");
        assert!(matches!(wrong.check(), Err(BillError::DoesNotAddUp { .. })));
        assert!(wrong.reconcile([]).is_err());
        let mut mixed = bill();
        mixed.credit = Money::zero(Currency::EUR);
        assert!(matches!(mixed.check(), Err(BillError::Money(_))));
        let mut euro_entry = bill();
        euro_entry.revision = 2;
        assert!(matches!(
            euro_entry.reconcile([Money::zero(Currency::EUR)]),
            Err(BillError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }

    #[test]
    fn a_bill_in_another_currency_needs_its_rate() {
        let mut bill = bill();
        assert_eq!(
            bill.rate_to(Currency::USD).unwrap(),
            ExchangeRate::identity(Currency::USD)
        );
        assert!(matches!(
            bill.rate_to(Currency::EUR),
            Err(BillError::NoExchangeRate { .. })
        ));
        let rate = ExchangeRate::new(Currency::USD, Currency::EUR, "0.9").unwrap();
        bill.exchange_rate = Some(rate);
        assert_eq!(bill.rate_to(Currency::EUR).unwrap(), rate);
        let backwards = ExchangeRate::new(Currency::EUR, Currency::USD, "1.1").unwrap();
        bill.exchange_rate = Some(backwards);
        assert!(bill.rate_to(Currency::EUR).is_err());
        let other_target =
            ExchangeRate::new(Currency::USD, Currency::new("GBP").unwrap(), "0.8").unwrap();
        bill.exchange_rate = Some(other_target);
        assert!(bill.rate_to(Currency::EUR).is_err());
    }

    #[test]
    fn bills_read_from_json_with_revision_one_by_default() {
        let json = serde_json::to_string(&bill()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("revision");
        let read: Bill = serde_json::from_value(value).unwrap();
        assert_eq!(read, bill());
    }
}
