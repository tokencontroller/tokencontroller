//! Bills (source record 3) and the check (formulas 5 and 6).
//!
//! A bill has exactly one contract. Metered: `Bill(c, r) = C({contract: c},
//! r) + U`, `U ≥ 0`; `U < 0` means a message is wrong. Flat: `Bill(c, r) =
//! fee + X`, and `C({contract: c}, r) − X` is what the plan covered. Every
//! check also shows the messages of the same provider with no contract and
//! those whose contract came from the fallback, so a check cannot balance
//! by leaving messages out (ADR 0028).

use serde::{Deserialize, Serialize};

use crate::contract::ContractKind;
use crate::event::DateRange;
use crate::money::{Currency, Money, MoneyError};
use crate::tags::TagValue;

/// A provider's bill under one contract over a date range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    /// Who charged, such as `anthropic`. It is the contract's provider.
    pub provider: String,
    /// The contract the bill is for, such as `anthropic-api`. A bill
    /// without one goes to the provider's only contract, else to a
    /// contract named after the provider. A new key creates the contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<TagValue>,
    /// The kind of a contract this bill creates. Metered when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_kind: Option<ContractKind>,
    pub range: DateRange,
    /// What the provider charged under the contract in the range. For a
    /// flat contract, the fee plus extra usage.
    pub amount: Money,
    /// The fee part of a flat contract's bill, when the bill says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee: Option<Money>,
    /// The extra usage part of a flat contract's bill, when the bill says
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_usage: Option<Money>,
}

impl Bill {
    /// A metered bill of a provider with no contract named, the way bills
    /// were sent before contracts.
    #[must_use]
    pub const fn of_provider(provider: String, range: DateRange, amount: Money) -> Self {
        Self {
            provider,
            contract: None,
            contract_kind: None,
            range,
            amount,
            fee: None,
            extra_usage: None,
        }
    }

    /// The currency the provider charges in.
    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.amount.currency()
    }

    /// Whether the range is one whole calendar month, the period a flat
    /// contract's fee is for.
    #[must_use]
    pub fn is_full_month(&self) -> bool {
        let from = self.range.from;
        from.day() == 1
            && self.range.to.month() == from.month()
            && self.range.to.year() == from.year()
            && self.range.to.next_day().is_none_or(|next| next.day() == 1)
    }
}

/// One bill next to the messages with its contract: what they explain, and
/// beside it what a check could otherwise hide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciliation {
    pub contract: TagValue,
    pub kind: ContractKind,
    pub bill: Money,
    /// `C({contract: c}, r)`: the messages with the bill's contract in its
    /// range.
    pub messages: Money,
    /// Metered: `U = Bill − messages`, what no message explains. Flat: what
    /// the bill has besides its fee and extra usage, zero when the parts
    /// add up.
    pub unexplained: Money,
    /// Flat: the fee, from the bill or from the contract for a whole month.
    pub fee: Option<Money>,
    /// Flat: `X`, the extra usage, from the bill or as the bill less the
    /// fee.
    pub extra_usage: Option<Money>,
    /// Flat: `messages − X`, what the plan was worth, when the fee is
    /// known.
    pub covered: Option<Money>,
    /// Messages of the bill's provider in its range with no contract.
    pub same_provider_no_contract: Money,
    /// The part of `messages` whose contract came from the fallback.
    pub contract_from_fallback: Money,
}

impl Reconciliation {
    /// Metered: `messages` explain the bill up to `U`.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] if the messages are priced in
    /// another currency than the bill, or [`MoneyError::Overflow`].
    pub fn metered(
        contract: TagValue,
        bill: Money,
        messages: Money,
        beside: (Money, Money),
    ) -> Result<Self, MoneyError> {
        let unexplained = bill.checked_sub(messages)?;
        Ok(Self {
            contract,
            kind: ContractKind::Metered,
            bill,
            messages,
            unexplained,
            fee: None,
            extra_usage: None,
            covered: None,
            same_provider_no_contract: beside.0,
            contract_from_fallback: beside.1,
        })
    }

    /// Flat: the bill is fee plus extra usage. Either part known gives the
    /// other as the rest of the bill; with neither known, nothing is
    /// guessed and nothing is covered.
    ///
    /// # Errors
    ///
    /// [`MoneyError::CurrencyMismatch`] or [`MoneyError::Overflow`].
    pub fn flat(
        contract: TagValue,
        bill: Money,
        messages: Money,
        parts: (Option<Money>, Option<Money>),
        beside: (Money, Money),
    ) -> Result<Self, MoneyError> {
        let (fee, extra_usage) = match parts {
            (Some(fee), Some(extra)) => (Some(fee), Some(extra)),
            (Some(fee), None) => (Some(fee), Some(bill.checked_sub(fee)?)),
            (None, Some(extra)) => (Some(bill.checked_sub(extra)?), Some(extra)),
            (None, None) => (None, None),
        };
        let unexplained = match (fee, extra_usage) {
            (Some(fee), Some(extra)) => bill.checked_sub(fee)?.checked_sub(extra)?,
            _ => Money::zero(bill.currency()),
        };
        let covered = extra_usage
            .map(|extra| messages.checked_sub(extra))
            .transpose()?;
        Ok(Self {
            contract,
            kind: ContractKind::Flat,
            bill,
            messages,
            unexplained,
            fee,
            extra_usage,
            covered,
            same_provider_no_contract: beside.0,
            contract_from_fallback: beside.1,
        })
    }

    /// Whether a message is wrong. Metered: `U < 0`, the messages come to
    /// more than the bill. Flat: the extra usage comes to more than every
    /// message under the plan, so a message is missing or wrong.
    #[must_use]
    pub fn a_message_is_wrong(&self) -> bool {
        match self.kind {
            ContractKind::Metered => self.unexplained.nanos().is_negative(),
            ContractKind::Flat => self
                .covered
                .is_some_and(|covered| covered.nanos().is_negative()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn march() -> DateRange {
        DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap()
    }

    fn key() -> TagValue {
        TagValue::new("anthropic-api").unwrap()
    }

    fn zero() -> (Money, Money) {
        (usd("0"), usd("0"))
    }

    #[test]
    fn messages_plus_unexplained_equal_a_metered_bill() {
        let check = Reconciliation::metered(key(), usd("1000"), usd("950"), zero()).unwrap();
        assert_eq!(check.bill, usd("1000"));
        assert_eq!(check.messages, usd("950"));
        assert_eq!(check.unexplained, usd("50"));
        assert_eq!(check.kind, ContractKind::Metered);
        assert_eq!(check.covered, None);
        assert!(!check.a_message_is_wrong());
    }

    #[test]
    fn messages_above_a_metered_bill_mean_a_message_is_wrong() {
        let over = Reconciliation::metered(key(), usd("1000"), usd("1000.01"), zero()).unwrap();
        assert_eq!(over.unexplained, usd("-0.01"));
        assert!(over.a_message_is_wrong());
        let exact = Reconciliation::metered(key(), usd("1000"), usd("1000"), zero()).unwrap();
        assert_eq!(exact.unexplained, usd("0"));
        assert!(!exact.a_message_is_wrong());
        assert!(matches!(
            Reconciliation::metered(key(), usd("1"), Money::zero(Currency::EUR), zero()),
            Err(MoneyError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn a_flat_bill_is_fee_plus_extra_usage_and_the_rest_is_covered() {
        let beside = (usd("70"), usd("5"));
        let both = Reconciliation::flat(
            key(),
            usd("250"),
            usd("1900"),
            (Some(usd("200")), Some(usd("50"))),
            beside,
        )
        .unwrap();
        assert_eq!(both.covered, Some(usd("1850")));
        assert_eq!(both.unexplained, usd("0"));
        assert_eq!(both.same_provider_no_contract, usd("70"));
        assert_eq!(both.contract_from_fallback, usd("5"));
        assert!(!both.a_message_is_wrong());
        let fee_only = Reconciliation::flat(
            key(),
            usd("250"),
            usd("1900"),
            (Some(usd("200")), None),
            beside,
        )
        .unwrap();
        assert_eq!(fee_only.extra_usage, Some(usd("50")));
        assert_eq!(fee_only.covered, Some(usd("1850")));
        let extra_only = Reconciliation::flat(
            key(),
            usd("250"),
            usd("1900"),
            (None, Some(usd("50"))),
            beside,
        )
        .unwrap();
        assert_eq!(extra_only.fee, Some(usd("200")));
        let neither =
            Reconciliation::flat(key(), usd("250"), usd("1900"), (None, None), beside).unwrap();
        assert_eq!(neither.fee, None);
        assert_eq!(neither.covered, None);
        assert_eq!(neither.unexplained, usd("0"));
        assert!(!neither.a_message_is_wrong());
        // Parts that do not add up to the bill leave the rest unexplained.
        let off = Reconciliation::flat(
            key(),
            usd("260"),
            usd("1900"),
            (Some(usd("200")), Some(usd("50"))),
            beside,
        )
        .unwrap();
        assert_eq!(off.unexplained, usd("10"));
        // Extra usage above every message under the plan: one is missing.
        let short = Reconciliation::flat(
            key(),
            usd("250"),
            usd("49.99"),
            (Some(usd("200")), None),
            beside,
        )
        .unwrap();
        assert_eq!(short.covered, Some(usd("-0.01")));
        assert!(short.a_message_is_wrong());
        let even = Reconciliation::flat(
            key(),
            usd("250"),
            usd("50"),
            (Some(usd("200")), None),
            beside,
        )
        .unwrap();
        assert!(!even.a_message_is_wrong());
    }

    #[test]
    fn a_bill_knows_a_whole_month() {
        let bill = Bill::of_provider("anthropic".to_owned(), march(), usd("1000"));
        assert!(bill.is_full_month());
        assert_eq!(bill.currency(), Currency::USD);
        let part = |from, to| {
            Bill::of_provider(
                "anthropic".to_owned(),
                DateRange::new(from, to).unwrap(),
                usd("1"),
            )
            .is_full_month()
        };
        assert!(!part(date!(2026 - 03 - 02), date!(2026 - 03 - 31)));
        assert!(!part(date!(2026 - 03 - 01), date!(2026 - 03 - 30)));
        assert!(!part(date!(2026 - 03 - 01), date!(2026 - 04 - 30)));
        assert!(!part(date!(2025 - 03 - 01), date!(2026 - 03 - 31)));
        assert!(part(date!(2026 - 02 - 01), date!(2026 - 02 - 28)));
        assert!(part(date!(2026 - 12 - 01), date!(2026 - 12 - 31)));
    }

    #[test]
    fn bills_read_from_json_without_a_contract() {
        let json = r#"{"provider": "anthropic", "range": {"from": "2026-03-01", "to": "2026-03-31"},
            "amount": {"nanos": 1000000000000, "currency": "USD"}}"#;
        let bill: Bill = serde_json::from_str(json).unwrap();
        assert_eq!(
            bill,
            Bill::of_provider("anthropic".to_owned(), march(), usd("1000"))
        );
        let again: Bill = serde_json::from_str(&serde_json::to_string(&bill).unwrap()).unwrap();
        assert_eq!(again, bill);
        let flat = Bill {
            contract: Some(key()),
            contract_kind: Some(ContractKind::Flat),
            fee: Some(usd("200")),
            extra_usage: Some(usd("50")),
            ..bill
        };
        let again: Bill = serde_json::from_str(&serde_json::to_string(&flat).unwrap()).unwrap();
        assert_eq!(again, flat);
    }
}
