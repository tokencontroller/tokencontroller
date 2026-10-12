//! The attributes of one event or span, merged with its resource's, read by
//! name. Each read takes the attribute, so what is left over becomes the
//! message's attributes, the raw material for tags.

use std::collections::BTreeMap;

use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokencontroller_core::{Currency, Money};

use crate::integrations::decimal::money_from_decimal;

/// Nano-units of money in one micro-unit.
const NANOS_PER_MICRO: i64 = 1_000;

/// A count attribute that is present but no whole, non-negative number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unreadable;

pub(super) struct Fields<'a> {
    values: BTreeMap<&'a str, &'a Value>,
}

impl<'a> Fields<'a> {
    /// The resource's attributes, then the record's: the record wins when
    /// both name the same attribute.
    pub(super) fn new(resource: &'a [KeyValue], record: &'a [KeyValue]) -> Self {
        let values = resource
            .iter()
            .chain(record)
            .filter_map(|attribute| {
                let value = attribute.value.as_ref()?.value.as_ref()?;
                Some((attribute.key.as_str(), value))
            })
            .collect();
        Self { values }
    }

    /// The text of an attribute, without taking it.
    pub(super) fn peek_text(&self, key: &str) -> Option<&'a str> {
        self.values.get(key).copied().and_then(text_of)
    }

    /// The first of `keys` that holds text (a string, or a whole number as
    /// some harnesses send IDs), taken.
    pub(super) fn take_text(&mut self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|key| {
            let text = self
                .values
                .get(*key)
                .copied()
                .and_then(|value| match value {
                    Value::IntValue(number) => Some(number.to_string()),
                    Value::StringValue(_) => text_of(value).map(str::to_owned),
                    _ => None,
                })?;
            self.values.remove(*key);
            Some(text)
        })
    }

    /// A token count, taken: `None` when absent. Harnesses send counts as
    /// integers, as decimal strings (`Claude Code`, Codex CLI) or as whole
    /// doubles.
    pub(super) fn take_count(&mut self, key: &str) -> Result<Option<u64>, Unreadable> {
        let Some(value) = self.values.remove(key) else {
            return Ok(None);
        };
        let count = match value {
            Value::IntValue(number) => u64::try_from(*number).ok(),
            Value::StringValue(text) => text.trim().parse::<u64>().ok(),
            Value::DoubleValue(number) => format!("{number}").parse::<u64>().ok(),
            _ => None,
        };
        count.map(Some).ok_or(Unreadable)
    }

    /// The first of `keys` that holds a count, taken, or zero when none is
    /// present.
    pub(super) fn take_count_or_zero(&mut self, keys: &[&str]) -> Result<u64, Unreadable> {
        for key in keys {
            if let Some(count) = self.take_count(key)? {
                return Ok(count);
            }
        }
        Ok(0)
    }

    /// An amount in US dollars, taken. A double is read through its shortest
    /// decimal text, so no float arithmetic touches it.
    pub(super) fn take_dollars(&mut self, key: &str) -> Option<Money> {
        let text = match self.values.get(key).copied()? {
            Value::StringValue(text) => text.clone(),
            Value::DoubleValue(number) => format!("{number}"),
            Value::IntValue(number) => number.to_string(),
            _ => return None,
        };
        let money = money_from_decimal(&text, Currency::USD)
            .ok()
            .filter(|money| !money.nanos().is_negative())?;
        self.values.remove(key);
        Some(money)
    }

    /// An amount in millionths of a US dollar, taken.
    pub(super) fn take_micro_dollars(&mut self, key: &str) -> Option<Money> {
        let micros = match self.values.get(key).copied()? {
            Value::IntValue(number) => *number,
            Value::StringValue(text) => text.trim().parse::<i64>().ok()?,
            _ => return None,
        };
        let nanos = micros
            .checked_mul(NANOS_PER_MICRO)
            .filter(|nanos| !nanos.is_negative())?;
        self.values.remove(key);
        Some(Money::from_nanos(nanos, Currency::USD))
    }

    /// An RFC 3339 timestamp, such as `event.timestamp`, taken.
    pub(super) fn take_timestamp(&mut self, key: &str) -> Option<OffsetDateTime> {
        let text = self.peek_text(key)?;
        let at = OffsetDateTime::parse(text, &Rfc3339).ok()?;
        self.values.remove(key);
        Some(at)
    }

    /// Takes attributes that were read by other means or mean nothing as tags.
    pub(super) fn discard(&mut self, keys: &[&str]) {
        for key in keys {
            self.values.remove(*key);
        }
    }

    /// What is left, as text: strings that are not blank, whole numbers,
    /// booleans and doubles.
    pub(super) fn into_attributes(self) -> BTreeMap<String, String> {
        self.values
            .into_iter()
            .filter_map(|(key, value)| {
                let text = match value {
                    Value::StringValue(_) => text_of(value)?.to_owned(),
                    Value::IntValue(number) => number.to_string(),
                    Value::BoolValue(flag) => flag.to_string(),
                    Value::DoubleValue(number) => format!("{number}"),
                    _ => return None,
                };
                Some((key.to_owned(), text))
            })
            .collect()
    }
}

/// A string value that is not blank.
fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::StringValue(text) if !text.trim().is_empty() => Some(text.trim()),
        _ => None,
    }
}
