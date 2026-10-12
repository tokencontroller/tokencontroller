//! Filters: a set of tags a view is limited to, such as `client: acme`, or
//! `team: web` and `project: acme-shop`. A split matches when it has every
//! tag in the filter. Reports, token budgets and people managers' access
//! are all filters.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Tag names and values: one value per tag name.
pub type Tags = BTreeMap<String, String>;

/// A set of tags. The empty filter matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(transparent)]
#[schema(value_type = BTreeMap<String, String>, example = json!({"client": "acme"}))]
pub struct Filter(pub Tags);

/// Why a filter could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilterError {
    #[error("{0:?} is not a tag: write it as name:value, such as client:acme")]
    NotATag(String),
    #[error("the filter names {0:?} twice; a filter has one value per tag name")]
    Twice(String),
}

impl Filter {
    /// Reads the filter of a URL, such as `client:acme,team:web`. An empty
    /// text is the empty filter.
    ///
    /// # Errors
    ///
    /// [`FilterError`] for a part that is not `name:value` or a tag name
    /// given twice.
    pub fn parse(text: &str) -> Result<Self, FilterError> {
        let mut tags = Tags::new();
        for part in text.split(',').map(str::trim).filter(|part| !part.is_empty()) {
            let (name, value) = part
                .split_once(':')
                .map(|(name, value)| (name.trim(), value.trim()))
                .filter(|(name, value)| !name.is_empty() && !value.is_empty())
                .ok_or_else(|| FilterError::NotATag(part.to_owned()))?;
            if tags.insert(name.to_owned(), value.to_owned()).is_some() {
                return Err(FilterError::Twice(name.to_owned()));
            }
        }
        Ok(Self(tags))
    }

    /// Whether a split with these tags matches.
    #[must_use]
    pub fn matches(&self, tags: &Tags) -> bool {
        self.0
            .iter()
            .all(|(name, value)| tags.get(name) == Some(value))
    }

    /// Whether every split this filter matches is also matched by `wider`:
    /// `team: web, client: acme` lies within `team: web`.
    #[must_use]
    pub fn lies_within(&self, wider: &Self) -> bool {
        wider.matches(&self.0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The filter as JSON, as the database stores it.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.0
                .iter()
                .map(|(name, value)| (name.clone(), serde_json::Value::String(value.clone())))
                .collect(),
        )
    }

    /// The filter the database stored. Anything but text values is ignored.
    #[must_use]
    pub fn from_json(value: &serde_json::Value) -> Self {
        let tags = value
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(name, value)| value.as_str().map(|v| (name.clone(), v.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        Self(tags)
    }
}

impl fmt::Display for Filter {
    /// The URL form, such as `client:acme,team:web`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .0
            .iter()
            .map(|(name, value)| format!("{name}:{value}"))
            .collect();
        f.write_str(&parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn reads_the_url_form() {
        let filter = Filter::parse("client:acme, team:web").unwrap();
        assert_eq!(filter.0, tags(&[("client", "acme"), ("team", "web")]));
        assert_eq!(filter.to_string(), "client:acme,team:web");
        assert!(Filter::parse("").unwrap().is_empty());
        assert_eq!(
            Filter::parse("ticket:KD-214").unwrap().0,
            tags(&[("ticket", "KD-214")])
        );
    }

    #[test]
    fn refuses_what_is_not_a_tag() {
        assert_eq!(
            Filter::parse("client"),
            Err(FilterError::NotATag("client".to_owned()))
        );
        assert!(Filter::parse("client:").is_err());
        assert!(Filter::parse(":acme").is_err());
        assert_eq!(
            Filter::parse("team:web,team:data"),
            Err(FilterError::Twice("team".to_owned()))
        );
    }

    #[test]
    fn matches_when_every_tag_is_there() {
        let filter = Filter::parse("team:web,client:acme").unwrap();
        assert!(filter.matches(&tags(&[("team", "web"), ("client", "acme"), ("ticket", "KD-1")])));
        assert!(!filter.matches(&tags(&[("team", "web")])));
        assert!(!filter.matches(&tags(&[("team", "web"), ("client", "globex")])));
        assert!(Filter::default().matches(&tags(&[])));
    }

    #[test]
    fn a_narrower_filter_lies_within_a_wider_one() {
        let wide = Filter::parse("team:web").unwrap();
        let narrow = Filter::parse("team:web,client:acme").unwrap();
        assert!(narrow.lies_within(&wide));
        assert!(!wide.lies_within(&narrow));
        assert!(wide.lies_within(&Filter::default()));
    }

    #[test]
    fn round_trips_through_json() {
        let filter = Filter::parse("team:web").unwrap();
        assert_eq!(Filter::from_json(&filter.to_json()), filter);
        assert!(Filter::from_json(&serde_json::json!(["x"])).is_empty());
        assert_eq!(
            Filter::from_json(&serde_json::json!({"team": "web", "n": 1})),
            filter
        );
    }
}
