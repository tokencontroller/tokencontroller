//! What the API writes and reads: money as decimal strings, tags as JSON
//! objects, filters as `client:acme,team:web`, and the OpenAPI shapes of
//! the core types.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Event, Filter, Money, TagName, TagValue, Tags};
use utoipa::ToSchema;

use crate::error::ApiError;

/// Tags as the OpenAPI document shows the core's [`Tags`]: a JSON object
/// of tag name to tag value.
#[derive(Debug, Clone, Default, ToSchema)]
#[schema(as = Tags, value_type = Object, example = json!({"feature": "KD-214", "project": "shop"}))]
pub struct TagsSchema(pub BTreeMap<String, String>);

/// An amount of money as the API writes it: a decimal string with the
/// currency's minor units, such as `139.50`, never a float.
#[must_use]
pub fn money(amount: Money) -> String {
    amount
        .currency()
        .format_minor_units(amount.round_to_minor_units())
}

/// An amount in full, for figures finer than a cent such as a price per
/// million tokens: `12.5`, `0.000031`.
#[must_use]
pub fn money_exact(amount: Money) -> String {
    Decimal::new(amount.nanos(), 9).normalize().to_string()
}

/// Reads a decimal string, or a JSON number, as money in `currency`.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for anything that is not a plain decimal.
pub fn parse_money(text: &str, currency: Currency) -> Result<Money, ApiError> {
    Money::parse(text.trim(), currency).map_err(|error| {
        ApiError::BadRequest(format!("{text:?} is not an amount in {currency}: {error}"))
    })
}

/// A decimal amount as a JSON number (`0.031`) or a decimal string
/// (`"0.031"`), read without floats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    Number(serde_json::Number),
    Text(String),
}

impl utoipa::PartialSchema for Amount {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::SchemaType::Type(
                utoipa::openapi::schema::Type::String,
            ))
            .description(Some("A decimal amount, as a string or a number"))
            .examples(["0.031"])
            .into()
    }
}

impl ToSchema for Amount {}

impl Amount {
    /// The amount as money in `currency`.
    ///
    /// # Errors
    ///
    /// [`ApiError::BadRequest`] for a number that is not a plain decimal.
    pub fn money(&self, currency: Currency) -> Result<Money, ApiError> {
        match self {
            Self::Number(number) => parse_money(&number.to_string(), currency),
            Self::Text(text) => parse_money(text, currency),
        }
    }

    /// The amount as a decimal, such as a fraction.
    ///
    /// # Errors
    ///
    /// [`ApiError::BadRequest`] for anything that is not a decimal.
    pub fn decimal(&self) -> Result<Decimal, ApiError> {
        let text = match self {
            Self::Number(number) => number.to_string(),
            Self::Text(text) => text.trim().to_owned(),
        };
        text.parse::<Decimal>()
            .or_else(|_| Decimal::from_scientific(&text))
            .map_err(|_| ApiError::BadRequest(format!("{text:?} is not a number")))
    }
}

/// Reads tags from a JSON object, checking names and values.
///
/// # Errors
///
/// [`ApiError::BadRequest`] naming the first tag that breaks the rules.
pub fn tags(pairs: &BTreeMap<String, String>) -> Result<Tags, ApiError> {
    let mut tags = Tags::new();
    for (name, value) in pairs {
        tags.insert(tag_name(name)?, tag_value(name, value)?);
    }
    Ok(tags)
}

/// Reads a tag name.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it breaks the rules.
pub fn tag_name(name: &str) -> Result<TagName, ApiError> {
    TagName::new(name.trim()).map_err(|error| ApiError::BadRequest(error.to_string()))
}

/// Reads a tag value for `name`.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when it breaks the rules.
pub fn tag_value(name: &str, value: &str) -> Result<TagValue, ApiError> {
    TagValue::new(value).map_err(|error| ApiError::BadRequest(format!("{name}: {error}")))
}

/// Tags as a plain JSON object.
#[must_use]
pub fn tags_map(tags: &Tags) -> BTreeMap<String, String> {
    tags.iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// Reads a filter as the query string carries it: `client:acme,team:web`.
/// Empty is the whole organization.
///
/// # Errors
///
/// [`ApiError::BadRequest`] for a part without `name:value`.
pub fn filter(text: Option<&str>) -> Result<Filter, ApiError> {
    let mut tags = Tags::new();
    for part in text
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (name, value) = part.split_once([':', '=']).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "{part:?} is not a filter part; write name:value, such as client:acme"
            ))
        })?;
        tags.insert(tag_name(name)?, tag_value(name, value.trim())?);
    }
    Ok(Filter(tags))
}

/// A filter as the query string carries it.
#[must_use]
pub fn filter_text(filter: &Filter) -> String {
    filter
        .0
        .iter()
        .map(|(name, value)| format!("{name}:{value}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// One event as `GET /events` and the raw export write it: the core's
/// own JSON, so a reader folds it back to the same figures.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventView {
    pub id: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
    /// The person who recorded it, by email, where a person did.
    pub recorded_by: Option<String>,
    /// The older event this one replaces, if any.
    pub supersedes: Option<String>,
    /// The kind in the words of the docs: `message`, `bill`, `tag value`, …
    pub kind: &'static str,
    /// The record, tagged with its kind, as the core crate writes it.
    #[schema(value_type = Object)]
    pub record: serde_json::Value,
}

impl EventView {
    /// The view of one event.
    #[must_use]
    pub fn of(event: &Event) -> Self {
        Self {
            id: event.id.to_string(),
            recorded_at: event.recorded_at,
            recorded_by: event.recorded_by.as_ref().map(ToString::to_string),
            supersedes: event.supersedes.as_ref().map(ToString::to_string),
            kind: event.record.kind(),
            record: serde_json::to_value(&event.record).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_is_written_with_the_minor_units_of_its_currency() {
        let usd = |text: &str| Money::parse(text, Currency::USD).unwrap();
        assert_eq!(money(usd("139.5")), "139.50");
        assert_eq!(money(usd("0.000031")), "0.00");
        assert_eq!(money_exact(usd("0.000031")), "0.000031");
        assert_eq!(money_exact(usd("12.50")), "12.5");
        assert_eq!(parse_money(" 7.25 ", Currency::USD).unwrap(), usd("7.25"));
        assert!(parse_money("seven", Currency::USD).is_err());
    }

    #[test]
    fn amounts_read_as_numbers_or_strings() {
        let number: Amount = serde_json::from_str("0.031").unwrap();
        let text: Amount = serde_json::from_str("\"0.031\"").unwrap();
        assert_eq!(
            number.money(Currency::USD).unwrap(),
            text.money(Currency::USD).unwrap()
        );
        assert_eq!(text.decimal().unwrap().to_string(), "0.031");
        assert!(Amount::Text("x".to_owned()).decimal().is_err());
    }

    #[test]
    fn tags_and_filters_read_from_the_wire() {
        let pairs: BTreeMap<String, String> = [("feature", "KD-1"), ("team", "web")]
            .into_iter()
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect();
        let read = tags(&pairs).unwrap();
        assert_eq!(tags_map(&read), pairs);
        let mut bad = pairs.clone();
        bad.insert("Team".to_owned(), "web".to_owned());
        assert!(tags(&bad).is_err());
        let filter = filter(Some(" client:acme , team=web")).unwrap();
        assert_eq!(filter_text(&filter), "client:acme,team:web");
        assert!(super::filter(None).unwrap().is_everything());
        assert!(super::filter(Some("acme")).is_err());
    }

    #[test]
    fn an_event_view_carries_the_cores_json() {
        let event = Event::new(
            "cut_1",
            time::macros::datetime!(2026-04-01 00:00 UTC),
            tokencontroller_core::Record::Cutoff(time::macros::date!(2026 - 03 - 31)),
        )
        .unwrap()
        .by(TagValue::new("nadia@kestrel.example").unwrap());
        let view = EventView::of(&event);
        assert_eq!(view.kind, "cutoff");
        assert_eq!(view.recorded_by.as_deref(), Some("nadia@kestrel.example"));
        assert_eq!(view.record, serde_json::json!({"cutoff": "2026-03-31"}));
    }
}
