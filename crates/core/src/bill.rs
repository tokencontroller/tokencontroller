//! Bills (source record 2) and reconciliation (formula 4).
//!
//! `Bill(provider, r) = C({provider}, r) + R(provider, r)`, with `R ≥ 0`.
//! Reconciliation is on the bill's own range. `R < 0` means a message is
//! wrong. `R` is a line on the bill, not a concept.

use serde::{Deserialize, Serialize};

use crate::event::DateRange;
use crate::money::{Currency, Money, MoneyError};

/// A provider's bill for metered usage over a date range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    pub provider: String,
    pub range: DateRange,
    pub amount: Money,
}

impl Bill {
    /// The currency the provider charges in.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.amount.currency()
    }

    /// Reconciles the bill against what the provider's messages in its
    /// range cost.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the messages are priced in
    /// another currency than the bill, or [`MoneyError::Overflow`].
    pub fn reconcile(&self, messages: Money) -> Result<Reconciliation, MoneyError> {
        let unexplained = self.amount.checked_sub(messages)?;
        Ok(Reconciliation {
            bill: self.amount,
            messages,
            unexplained,
        })
    }
}

/// One bill next to its messages: what the messages explain and what the
/// bill has that no message explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciliation {
    pub bill: Money,
    /// `C({provider}, r)`.
    pub messages: Money,
    /// `R`: what the bill has that no message explains.
    pub unexplained: Money,
}

impl Reconciliation {
    /// Whether `R < 0`: the messages come to more than the bill, so a
    /// message is wrong.
    #[must_use]
    pub fn a_message_is_wrong(&self) -> bool {
        self.unexplained.nanos().is_negative()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn bill() -> Bill {
        Bill {
            provider: "anthropic".to_owned(),
            range: DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap(),
            amount: usd("1000"),
        }
    }

    #[test]
    fn messages_plus_unexplained_equal_the_bill() {
        let reconciliation = bill().reconcile(usd("950")).unwrap();
        assert_eq!(reconciliation.bill, usd("1000"));
        assert_eq!(reconciliation.messages, usd("950"));
        assert_eq!(reconciliation.unexplained, usd("50"));
        assert!(!reconciliation.a_message_is_wrong());
        assert_eq!(bill().currency(), Currency::USD);
    }

    #[test]
    fn messages_above_the_bill_mean_a_message_is_wrong() {
        let over = bill().reconcile(usd("1000.01")).unwrap();
        assert_eq!(over.unexplained, usd("-0.01"));
        assert!(over.a_message_is_wrong());
        let exact = bill().reconcile(usd("1000")).unwrap();
        assert_eq!(exact.unexplained, usd("0"));
        assert!(!exact.a_message_is_wrong());
        let none = bill().reconcile(usd("0")).unwrap();
        assert_eq!(none.unexplained, usd("1000"));
    }

    #[test]
    fn a_bill_in_another_currency_than_the_prices_is_refused() {
        assert!(matches!(
            bill().reconcile(Money::zero(Currency::EUR)),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
        let json = serde_json::to_string(&bill()).unwrap();
        assert_eq!(serde_json::from_str::<Bill>(&json).unwrap(), bill());
    }
}
