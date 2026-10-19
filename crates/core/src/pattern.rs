//! Regular expressions: the pattern a rule tests a signal with.

use std::fmt;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The most memory one compiled pattern may take. Patterns are typed by
/// people; this keeps a careless one from slowing every message down.
/// Matching is always linear in the text.
const SIZE_LIMIT: usize = 1 << 20;

/// A pattern that does not compile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{pattern:?} is not a regular expression: {reason}")]
pub struct PatternError {
    pub pattern: String,
    pub reason: String,
}

/// A regular expression a rule tests a signal with, such as `^review/` or
/// `(?P<key>KD-\d+)`. It matches anywhere in the text unless it says
/// otherwise. A group named `key` picks the part of the match that is a tag
/// value's key; without one, the whole match is the key.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TextPattern {
    source: String,
    regex: Regex,
}

impl TextPattern {
    /// Compiles a pattern.
    ///
    /// # Errors
    ///
    /// [`PatternError`] if it is not a regular expression or is too large.
    pub fn new(pattern: &str) -> Result<Self, PatternError> {
        let regex = regex::RegexBuilder::new(pattern)
            .size_limit(SIZE_LIMIT)
            .build()
            .map_err(|error| PatternError {
                pattern: pattern.to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Self {
            source: pattern.to_owned(),
            regex,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Whether the pattern matches anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// The key the first match names: the group `key` if the pattern has
    /// one, else the whole match. `None` when nothing matches or the key is
    /// empty.
    #[must_use]
    pub fn key_in<'t>(&self, text: &'t str) -> Option<&'t str> {
        let captures = self.regex.captures(text)?;
        let found = captures
            .name("key")
            .or_else(|| captures.get(0))
            .map(|found| found.as_str())?;
        (!found.is_empty()).then_some(found)
    }

    /// Every key the pattern finds in `text`, in order, once per mention.
    #[must_use]
    pub fn keys_in<'t>(&self, text: &'t str) -> Vec<&'t str> {
        self.regex
            .captures_iter(text)
            .filter_map(|captures| {
                captures
                    .name("key")
                    .or_else(|| captures.get(0))
                    .map(|found| found.as_str())
            })
            .filter(|found| !found.is_empty())
            .collect()
    }
}

impl PartialEq for TextPattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for TextPattern {}

impl fmt::Debug for TextPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TextPattern({:?})", self.source)
    }
}

impl TryFrom<String> for TextPattern {
    type Error = PatternError;

    fn try_from(pattern: String) -> Result<Self, Self::Error> {
        Self::new(&pattern)
    }
}

impl From<TextPattern> for String {
    fn from(pattern: TextPattern) -> Self {
        pattern.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_patterns_match_anywhere_unless_anchored() {
        let review = TextPattern::new("^review/").unwrap();
        assert!(review.is_match("review/kd-214"));
        assert!(!review.is_match("feature/review/x"));
        assert!(
            TextPattern::new("deps")
                .unwrap()
                .is_match("chore/deps-update")
        );
        assert_eq!(review.as_str(), "^review/");
    }

    #[test]
    fn the_whole_match_is_the_key_without_a_named_group() {
        let jira = TextPattern::new(r"KD-\d+").unwrap();
        assert_eq!(jira.key_in("feature/KD-214-fix"), Some("KD-214"));
        assert_eq!(jira.key_in("main"), None);
        assert_eq!(
            jira.keys_in("fix KD-214, then KD-7 and KD-214"),
            vec!["KD-214", "KD-7", "KD-214"]
        );
        assert!(jira.keys_in("nothing").is_empty());
    }

    #[test]
    fn a_group_named_key_picks_the_key_out_of_the_match() {
        let branch = TextPattern::new(r"^(?:feature|fix)/(?P<key>[A-Z]+-\d+)").unwrap();
        assert_eq!(branch.key_in("feature/KD-214-login"), Some("KD-214"));
        assert_eq!(branch.key_in("fix/OPS-7"), Some("OPS-7"));
        assert_eq!(branch.key_in("KD-214-login"), None);
        assert_eq!(branch.keys_in("fix/OPS-7"), vec!["OPS-7"]);
    }

    #[test]
    fn empty_matches_are_not_keys() {
        let any = TextPattern::new(r"\d*").unwrap();
        assert_eq!(any.key_in("abc"), None);
        assert!(any.keys_in("abc").is_empty());
        let empty_group = TextPattern::new(r"x(?P<key>\d*)").unwrap();
        assert_eq!(empty_group.key_in("xa"), None);
        assert_eq!(empty_group.key_in("x12"), Some("12"));
    }

    #[test]
    fn broken_patterns_are_refused() {
        let error = TextPattern::new("[").unwrap_err();
        assert_eq!(error.pattern, "[");
        assert!(!error.reason.is_empty());
        assert!(TextPattern::new(&"a{1000}".repeat(100)).is_err());
    }

    #[test]
    fn patterns_compare_and_print_by_their_text() {
        assert_eq!(
            TextPattern::new("a").unwrap(),
            TextPattern::new("a").unwrap()
        );
        assert_ne!(
            TextPattern::new("a").unwrap(),
            TextPattern::new("b").unwrap()
        );
        assert_eq!(
            format!("{:?}", TextPattern::new("a").unwrap()),
            r#"TextPattern("a")"#
        );
    }

    #[test]
    fn patterns_read_and_write_as_json_strings() {
        let text: TextPattern = serde_json::from_str(r#""^review/""#).unwrap();
        assert_eq!(serde_json::to_string(&text).unwrap(), r#""^review/""#);
        assert!(serde_json::from_str::<TextPattern>(r#""(""#).is_err());
        assert_eq!(String::from(text), "^review/");
    }
}
