//! Tags and attributes: a name and a value, on a split or on a person.
//!
//! A set of tags has at most one value per tag name, so cost partitioned by
//! one tag name always adds up to the total (formula 5). Attributes of
//! people (team, department) use the same names and values, so one filter
//! can hold tags and attributes together. A filter is a set of tags and
//! attributes a report or a grant is limited to.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The longest tag name, in characters.
const MAX_NAME_LENGTH: usize = 40;
/// The longest tag value, in characters.
const MAX_VALUE_LENGTH: usize = 200;

/// A tag name or value that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    #[error(
        "{0:?} is not a tag name: use up to 40 small letters, digits, - and _, starting with a letter"
    )]
    InvalidName(String),
    #[error("{0:?} is not a tag value: use up to 200 characters, not only spaces")]
    InvalidValue(String),
    #[error("{0:?} is not a tag: write it as name=value, such as team=web")]
    InvalidTag(String),
}

/// The left half of a tag, such as `team`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TagName(String);

impl TagName {
    /// Reads a tag name: small ASCII letters, digits, `-` and `_`, starting
    /// with a letter, at most 40 characters.
    ///
    /// # Errors
    ///
    /// [`TagError::InvalidName`] for anything else.
    pub fn new(name: &str) -> Result<Self, TagError> {
        let starts_with_letter = name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        let allowed = name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if starts_with_letter && allowed && name.len() <= MAX_NAME_LENGTH {
            Ok(Self(name.to_owned()))
        } else {
            Err(TagError::InvalidName(name.to_owned()))
        }
    }

    /// The name `person`: the person whose message a split belongs to. It
    /// is on every closed tag set and is never a tag value record.
    #[must_use]
    pub fn person() -> Self {
        Self("person".to_owned())
    }

    /// The built-in tag `project`.
    #[must_use]
    pub fn project() -> Self {
        Self("project".to_owned())
    }

    /// The built-in tag `feature`: a ticket, story, epic or task.
    #[must_use]
    pub fn feature() -> Self {
        Self("feature".to_owned())
    }

    /// The built-in tag `client`.
    #[must_use]
    pub fn client() -> Self {
        Self("client".to_owned())
    }

    /// Whether this is one of the built-in tag names `project`, `feature`
    /// and `client`. An organization adds its own.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        matches!(self.0.as_str(), "project" | "feature" | "client")
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TagName {
    type Error = TagError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(&name)
    }
}

impl From<TagName> for String {
    fn from(name: TagName) -> Self {
        name.0
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The right half of a tag, such as `web`. Values are kept exactly as given
/// apart from spaces at either end: `acme` and `Acme` are two values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TagValue(String);

impl TagValue {
    /// Reads a tag value: any text up to 200 characters without control
    /// characters, with spaces at either end removed.
    ///
    /// # Errors
    ///
    /// [`TagError::InvalidValue`] if nothing is left after removing the
    /// spaces, the value is too long, or it has a control character.
    pub fn new(value: &str) -> Result<Self, TagError> {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > MAX_VALUE_LENGTH
            || trimmed.chars().any(char::is_control)
        {
            Err(TagError::InvalidValue(value.to_owned()))
        } else {
            Ok(Self(trimmed.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TagValue {
    type Error = TagError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<TagValue> for String {
    fn from(value: TagValue) -> Self {
        value.0
    }
}

impl fmt::Display for TagValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A set of tags with at most one value per tag name, ordered by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tags(BTreeMap<TagName, TagValue>);

impl Tags {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads tags written as `name=value` pairs, the way the CLI takes them,
    /// such as `["team=web", "client=acme"]`. A later value for the same
    /// name replaces an earlier one.
    ///
    /// # Errors
    ///
    /// [`TagError`] for a pair without `=` or with an invalid name or value.
    pub fn parse<'a, I>(pairs: I) -> Result<Self, TagError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut tags = Self::new();
        for pair in pairs {
            let (name, value) = pair
                .split_once('=')
                .ok_or_else(|| TagError::InvalidTag(pair.to_owned()))?;
            tags.insert(TagName::new(name.trim())?, TagValue::new(value)?);
        }
        Ok(tags)
    }

    /// Sets a tag, replacing any value it had: one value per tag name.
    /// Returns the value it replaced.
    pub fn insert(&mut self, name: TagName, value: TagValue) -> Option<TagValue> {
        self.0.insert(name, value)
    }

    /// Removes a tag and returns its value.
    pub fn remove(&mut self, name: &TagName) -> Option<TagValue> {
        self.0.remove(name)
    }

    /// The value of a tag name, if the set has one.
    #[must_use]
    pub fn get(&self, name: &TagName) -> Option<&TagValue> {
        self.0.get(name)
    }

    /// The value of a tag name given as text.
    #[must_use]
    pub fn get_str(&self, name: &str) -> Option<&TagValue> {
        TagName::new(name).ok().and_then(|name| self.0.get(&name))
    }

    #[must_use]
    pub fn contains(&self, name: &TagName) -> bool {
        self.0.contains_key(name)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The tags in order of their names.
    pub fn iter(&self) -> impl Iterator<Item = (&TagName, &TagValue)> {
        self.0.iter()
    }

    /// These tags on top of `lower`: where both have a value for a tag name,
    /// this set's value wins.
    #[must_use]
    pub fn over(&self, lower: &Self) -> Self {
        let mut merged = lower.clone();
        for (name, value) in &self.0 {
            merged.insert(name.clone(), value.clone());
        }
        merged
    }

    /// Only the tags whose name passes `keep`.
    #[must_use]
    pub fn retain(&self, mut keep: impl FnMut(&TagName) -> bool) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(name, _)| keep(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        )
    }
}

impl FromIterator<(TagName, TagValue)> for Tags {
    fn from_iter<I: IntoIterator<Item = (TagName, TagValue)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl fmt::Display for Tags {
    /// Shows tags the way reports do: `feature KD-1 · team web`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, (name, value)) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str(" · ")?;
            }
            write!(f, "{name} {value}")?;
        }
        Ok(())
    }
}

/// A set of tags and attributes a report or a grant is limited to, such as
/// `client: acme` or `team: web`. The filter `F` of the formulas. A split
/// matches when its closed tag set has every tag in the filter
/// (`F ⊆ T*(s)`); an empty filter matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Filter(pub Tags);

impl Filter {
    /// Whether `tags` has every tag of the filter, with the same value.
    #[must_use]
    pub fn matches(&self, tags: &Tags) -> bool {
        self.0
            .iter()
            .all(|(name, value)| tags.get(name) == Some(value))
    }

    /// Whether every tag of this filter is in `other`: this filter is at
    /// least as wide. A grant on `self` covers a question about `other`.
    #[must_use]
    pub fn covers(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }

    /// Whether the filter limits nothing.
    #[must_use]
    pub fn is_everything(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    #[test]
    fn tag_names_are_short_lowercase_words() {
        for good in ["team", "cost-center", "client_2", "a"] {
            assert_eq!(TagName::new(good).unwrap().as_str(), good);
        }
        let longest = "a".repeat(40);
        assert!(TagName::new(&longest).is_ok());
        for bad in [
            "",
            "Team",
            "2team",
            "-team",
            "team web",
            "tëam",
            "team:",
            &"a".repeat(41),
        ] {
            assert_eq!(
                TagName::new(bad),
                Err(TagError::InvalidName(bad.to_owned()))
            );
        }
    }

    #[test]
    fn built_in_tag_names_are_project_feature_and_client() {
        assert!(TagName::project().is_built_in());
        assert!(TagName::feature().is_built_in());
        assert!(TagName::client().is_built_in());
        assert!(!TagName::person().is_built_in());
        assert!(!TagName::new("team").unwrap().is_built_in());
        assert_eq!(TagName::person(), TagName::new("person").unwrap());
        assert_eq!(TagName::project(), TagName::new("project").unwrap());
        assert_eq!(TagName::feature(), TagName::new("feature").unwrap());
        assert_eq!(TagName::client(), TagName::new("client").unwrap());
    }

    #[test]
    fn tag_values_keep_their_case_and_lose_outer_spaces() {
        assert_eq!(TagValue::new(" Acme ").unwrap().as_str(), "Acme");
        assert_ne!(
            TagValue::new("acme").unwrap(),
            TagValue::new("Acme").unwrap()
        );
        assert_eq!(
            TagValue::new("Ö".repeat(200).as_str())
                .unwrap()
                .as_str()
                .chars()
                .count(),
            200
        );
        for bad in ["", "   ", "a\nb", "tab\there"] {
            assert!(TagValue::new(bad).is_err(), "{bad:?}");
        }
        assert!(TagValue::new(&"Ö".repeat(201)).is_err());
    }

    #[test]
    fn a_set_has_one_value_per_tag_name() {
        let mut set = tags(&["team=web", "team=data"]);
        assert_eq!(set.len(), 1);
        assert_eq!(set.get_str("team").unwrap().as_str(), "data");
        let old = set.insert(
            TagName::new("team").unwrap(),
            TagValue::new("mobile").unwrap(),
        );
        assert_eq!(old.unwrap().as_str(), "data");
        assert_eq!(
            set.remove(&TagName::new("team").unwrap()).unwrap().as_str(),
            "mobile"
        );
        assert!(set.is_empty());
        assert_eq!(set.get_str("Not A Name"), None);
    }

    #[test]
    fn a_set_counts_its_tags() {
        let set = tags(&["team=web", "client=acme"]);
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        let collected: Tags = set
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        assert_eq!(collected, set);
    }

    #[test]
    fn tags_read_from_name_value_pairs() {
        let set = tags(&["team = web", "client=acme corp"]);
        assert_eq!(set.to_string(), "client acme corp · team web");
        assert!(set.contains(&TagName::new("client").unwrap()));
        assert_eq!(
            Tags::parse(["team"]),
            Err(TagError::InvalidTag("team".to_owned()))
        );
        assert!(Tags::parse(["Team=web"]).is_err());
        assert!(Tags::parse(["team="]).is_err());
        assert_eq!(Tags::new().to_string(), "");
    }

    #[test]
    fn upper_tags_win_over_lower_ones() {
        let upper = tags(&["team=web", "purpose=learning"]);
        let lower = tags(&["team=data", "client=acme"]);
        assert_eq!(
            upper.over(&lower),
            tags(&["team=web", "purpose=learning", "client=acme"])
        );
        let only_team = upper.retain(|name| name.as_str() == "team");
        assert_eq!(only_team, tags(&["team=web"]));
    }

    #[test]
    fn a_filter_matches_tags_that_have_all_of_it() {
        let entry = tags(&["team=web", "client=acme", "project=acme-shop"]);
        assert!(Filter::default().matches(&entry));
        assert!(Filter(tags(&["client=acme"])).matches(&entry));
        assert!(Filter(tags(&["team=web", "project=acme-shop"])).matches(&entry));
        assert!(!Filter(tags(&["team=web", "project=globex-app"])).matches(&entry));
        assert!(!Filter(tags(&["purpose=learning"])).matches(&entry));
        assert!(!Filter(tags(&["client=Acme"])).matches(&entry));
    }

    #[test]
    fn a_wider_filter_covers_a_narrower_one() {
        let acme = Filter(tags(&["client=acme"]));
        let acme_web = Filter(tags(&["client=acme", "team=web"]));
        assert!(Filter::default().covers(&acme));
        assert!(acme.covers(&acme_web));
        assert!(acme.covers(&acme));
        assert!(!acme_web.covers(&acme));
        assert!(!Filter(tags(&["client=globex"])).covers(&acme));
        assert!(Filter::default().is_everything());
        assert!(!acme.is_everything());
    }

    #[test]
    fn tags_read_and_write_as_a_json_object() {
        let set = tags(&["team=web"]);
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"{"team":"web"}"#);
        assert_eq!(serde_json::from_str::<Tags>(&json).unwrap(), set);
        assert!(serde_json::from_str::<Tags>(r#"{"Team":"web"}"#).is_err());
        assert!(serde_json::from_str::<Tags>(r#"{"team":" "}"#).is_err());
        let names: Vec<String> = serde_json::from_str::<Tags>(r#"{"b":"1","a":"2"}"#)
            .unwrap()
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
