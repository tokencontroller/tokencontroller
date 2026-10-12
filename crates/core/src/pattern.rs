//! Regular expressions: a project's ticket key pattern, and the text tests
//! of rules.

use std::fmt;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The most memory one compiled pattern may take. Patterns are typed by
/// controllers and people; this keeps a careless one from slowing every
/// token entry down. Matching is always linear in the text.
const SIZE_LIMIT: usize = 1 << 20;

/// A pattern that does not compile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{pattern:?} is not a regular expression: {reason}")]
pub struct PatternError {
    pub pattern: String,
    pub reason: String,
}

fn compile(source: &str, original: &str) -> Result<Regex, PatternError> {
    regex::RegexBuilder::new(source)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|error| PatternError {
            pattern: original.to_owned(),
            reason: error.to_string(),
        })
}

/// How a project's tickets are named, such as `^(KD|OPS)-\d+` or
/// `storefront#\d+`.
///
/// Keys are found anywhere in a branch name, a title or a prompt. A leading
/// `^` means the key starts a word, so `^KD-\d+` finds `KD-214` in
/// `feature/KD-214-fix` but not in `XKD-214`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TicketKeyPattern {
    source: String,
    find: Regex,
    whole: Regex,
}

impl TicketKeyPattern {
    /// Compiles a ticket key pattern.
    ///
    /// # Errors
    ///
    /// [`PatternError`] if it is not a regular expression or is too large.
    pub fn new(pattern: &str) -> Result<Self, PatternError> {
        let (word_start, rest) = match pattern.strip_prefix('^') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let find = if word_start {
            format!("(?:^|[^[:alnum:]])(?P<key>{rest})")
        } else {
            format!("(?P<key>{rest})")
        };
        Ok(Self {
            source: pattern.to_owned(),
            find: compile(&find, pattern)?,
            whole: compile(&format!("^(?:{rest})$"), pattern)?,
        })
    }

    /// The pattern as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Every key in `text`, with the byte position it starts at, in order.
    #[must_use]
    pub fn find_all<'t>(&self, text: &'t str) -> Vec<(usize, &'t str)> {
        self.find
            .captures_iter(text)
            .filter_map(|captures| captures.name("key"))
            .filter(|key| !key.is_empty())
            .map(|key| (key.start(), key.as_str()))
            .collect()
    }

    /// Whether `key` is a whole key of this pattern, such as `KD-214`.
    #[must_use]
    pub fn is_key(&self, key: &str) -> bool {
        self.whole.is_match(key)
    }
}

impl PartialEq for TicketKeyPattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for TicketKeyPattern {}

impl fmt::Debug for TicketKeyPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TicketKeyPattern({:?})", self.source)
    }
}

impl TryFrom<String> for TicketKeyPattern {
    type Error = PatternError;

    fn try_from(pattern: String) -> Result<Self, Self::Error> {
        Self::new(&pattern)
    }
}

impl From<TicketKeyPattern> for String {
    fn from(pattern: TicketKeyPattern) -> Self {
        pattern.source
    }
}

/// A regular expression a rule tests a signal with, such as `^review/`.
/// It matches anywhere in the text unless it says otherwise.
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
        Ok(Self {
            source: pattern.to_owned(),
            regex: compile(pattern, pattern)?,
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

    fn keys<'t>(pattern: &str, text: &'t str) -> Vec<&'t str> {
        TicketKeyPattern::new(pattern)
            .unwrap()
            .find_all(text)
            .into_iter()
            .map(|(_, key)| key)
            .collect()
    }

    #[test]
    fn a_leading_caret_means_the_key_starts_a_word() {
        let jira = r"^(KD|OPS)-\d+";
        assert_eq!(keys(jira, "KD-214-fix"), vec!["KD-214"]);
        assert_eq!(keys(jira, "feature/KD-214-fix"), vec!["KD-214"]);
        assert_eq!(
            keys(jira, "fix KD-214, then OPS-7 and KD-260"),
            vec!["KD-214", "OPS-7", "KD-260"]
        );
        assert_eq!(keys(jira, "KD-1,KD-2"), vec!["KD-1", "KD-2"]);
        assert_eq!(keys(jira, "XKD-214 and kd-214"), Vec::<&str>::new());
        assert_eq!(keys(jira, "KD-2140"), vec!["KD-2140"]);
    }

    #[test]
    fn a_pattern_without_a_caret_matches_anywhere() {
        assert_eq!(
            keys(r"storefront#\d+", "fixes kestrel/storefront#88"),
            vec!["storefront#88"]
        );
    }

    #[test]
    fn keys_come_with_their_position() {
        let pattern = TicketKeyPattern::new(r"^KD-\d+").unwrap();
        assert_eq!(
            pattern.find_all("a KD-1 KD-22"),
            vec![(2, "KD-1"), (7, "KD-22")]
        );
        assert_eq!(pattern.find_all("KD-3"), vec![(0, "KD-3")]);
    }

    #[test]
    fn empty_matches_are_not_keys() {
        assert_eq!(keys(r"\d*", "abc"), Vec::<&str>::new());
    }

    #[test]
    fn whole_keys_are_recognized() {
        let pattern = TicketKeyPattern::new(r"^(KD|OPS)-\d+").unwrap();
        assert!(pattern.is_key("KD-214"));
        assert!(pattern.is_key("OPS-1"));
        assert!(!pattern.is_key("KD-214-fix"));
        assert!(!pattern.is_key("xKD-214"));
        assert!(!pattern.is_key("GLOBEX-7"));
        assert_eq!(pattern.as_str(), r"^(KD|OPS)-\d+");
    }

    #[test]
    fn broken_patterns_are_refused() {
        let error = TicketKeyPattern::new("^KD-(").unwrap_err();
        assert_eq!(error.pattern, "^KD-(");
        assert!(TextPattern::new("[").is_err());
        assert!(TicketKeyPattern::new(&"a{1000}".repeat(100)).is_err());
    }

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
    fn patterns_compare_and_print_by_their_text() {
        assert_eq!(
            TicketKeyPattern::new("^A-\\d+").unwrap(),
            TicketKeyPattern::new("^A-\\d+").unwrap()
        );
        assert_ne!(
            TicketKeyPattern::new("^A-\\d+").unwrap(),
            TicketKeyPattern::new("^B-\\d+").unwrap()
        );
        assert_eq!(
            TextPattern::new("a").unwrap(),
            TextPattern::new("a").unwrap()
        );
        assert_ne!(
            TextPattern::new("a").unwrap(),
            TextPattern::new("b").unwrap()
        );
        assert_eq!(
            format!("{:?}", TicketKeyPattern::new("^A").unwrap()),
            r#"TicketKeyPattern("^A")"#
        );
        assert_eq!(
            format!("{:?}", TextPattern::new("a").unwrap()),
            r#"TextPattern("a")"#
        );
    }

    #[test]
    fn patterns_read_and_write_as_json_strings() {
        let pattern: TicketKeyPattern = serde_json::from_str(r#""^KD-\\d+""#).unwrap();
        assert_eq!(serde_json::to_string(&pattern).unwrap(), r#""^KD-\\d+""#);
        assert!(serde_json::from_str::<TicketKeyPattern>(r#""(""#).is_err());
        let text: TextPattern = serde_json::from_str(r#""^review/""#).unwrap();
        assert_eq!(serde_json::to_string(&text).unwrap(), r#""^review/""#);
        assert!(serde_json::from_str::<TextPattern>(r#""(""#).is_err());
    }
}
