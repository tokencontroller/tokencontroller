//! Finding feature keys in branch names, titles and prompts with a feature
//! key pattern, such as `^(KD|OPS)-\d+`. Only the keys found leave the
//! machine, never the text. The same pattern becomes the core's rule
//! pattern, so the machine finds the keys the rules do.

use regex::Regex;
use tokencontroller_core::TextPattern;

/// The most memory one compiled pattern may take, as in the core.
const SIZE_LIMIT: usize = 1 << 20;

/// A feature key pattern, ready to search text with.
#[derive(Debug, Clone)]
pub struct KeyPattern {
    source: String,
    /// The pattern as the core's rules take it: a group named `key` holds
    /// the key, and a leading `^` in the source means the key starts a word.
    rule_source: String,
    regex: Regex,
}

impl KeyPattern {
    /// Compiles a pattern such as `^(KD|OPS)-\d+`. A leading `^` means the
    /// key starts a word, so `XKD-1` is not `KD-1`. `None` for a pattern that
    /// is empty or is not a regular expression.
    pub fn new(pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        let (word_start, body) = match pattern.strip_prefix('^') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        if body.is_empty() {
            return None;
        }
        let rule_source = if body.contains("(?P<key>") {
            // Already a rule pattern, such as the organization's.
            pattern.to_owned()
        } else if word_start {
            format!(r"(?:^|[^A-Za-z0-9_])(?P<key>{body})")
        } else {
            format!(r"(?P<key>{body})")
        };
        let regex = regex::RegexBuilder::new(&rule_source)
            .size_limit(SIZE_LIMIT)
            .build()
            .ok()?;
        Some(Self {
            source: pattern.to_owned(),
            rule_source,
            regex,
        })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// The pattern as a rule in the core reads it.
    pub fn rule_pattern(&self) -> Option<TextPattern> {
        TextPattern::new(&self.rule_source).ok()
    }

    /// Every key in `text`, with the byte position it starts at, in order.
    pub fn find_at(&self, text: &str) -> Vec<(usize, String)> {
        self.regex
            .captures_iter(text)
            .filter_map(|captures| captures.name("key"))
            .filter(|found| !found.as_str().is_empty())
            .map(|found| (found.start(), found.as_str().to_owned()))
            .collect()
    }

    /// Every key in `text`, in order, each once.
    pub fn find_all(&self, text: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for (_, key) in self.find_at(text) {
            if !found.contains(&key) {
                found.push(key);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jira() -> KeyPattern {
        KeyPattern::new(r"^(KD|OPS)-\d+").unwrap()
    }

    #[test]
    fn keys_are_found_in_branches_titles_and_prompts() {
        let pattern = jira();
        assert_eq!(pattern.find_all("KD-214-fix-checkout"), ["KD-214"]);
        assert_eq!(pattern.find_all("feature/OPS-7"), ["OPS-7"]);
        assert_eq!(
            pattern.find_all("Fix KD-214, then KD-231 (and KD-214 again)"),
            ["KD-214", "KD-231"]
        );
        assert_eq!(
            pattern.find_at("a KD-1").first().map(|(at, _)| *at),
            Some(2)
        );
        assert_eq!(pattern.find_all("work on kd-214"), Vec::<String>::new());
        assert_eq!(pattern.as_str(), r"^(KD|OPS)-\d+");
    }

    #[test]
    fn a_leading_caret_means_the_key_starts_a_word() {
        let pattern = jira();
        assert!(pattern.find_all("XKD-214").is_empty());
        assert!(pattern.find_all("2KD-214").is_empty());
        assert_eq!(pattern.find_all("(KD-9)"), ["KD-9"]);
        let anywhere = KeyPattern::new(r"KD-\d+").unwrap();
        assert_eq!(anywhere.find_all("XKD-214"), ["KD-214"]);
    }

    #[test]
    fn the_rule_pattern_finds_the_same_keys() {
        let rule = jira().rule_pattern().unwrap();
        assert_eq!(rule.key_in("feature/KD-12-login"), Some("KD-12"));
        assert_eq!(rule.key_in("XKD-12"), None);
        assert_eq!(rule.keys_in("KD-1 and OPS-2"), ["KD-1", "OPS-2"]);
    }

    #[test]
    fn empty_and_broken_patterns_are_refused() {
        let either = KeyPattern::new(r"KD-\d+|OPS-\d+").unwrap();
        assert_eq!(either.find_all("x OPS-2 y KD-3"), ["OPS-2", "KD-3"]);
        assert!(KeyPattern::new("^").is_none());
        assert!(KeyPattern::new(" ").is_none());
        assert!(KeyPattern::new("(").is_none());
    }

    #[test]
    fn a_pattern_that_already_names_its_key_is_taken_as_it_is() {
        let organizations = KeyPattern::new(r"(?:^|[^A-Za-z0-9_])(?P<key>(KD|OPS)-\d+)").unwrap();
        assert_eq!(
            organizations.find_all("feature/KD-12 and OPS-3"),
            ["KD-12", "OPS-3"]
        );
        assert!(organizations.find_all("XKD-12").is_empty());
        let rule = organizations.rule_pattern().unwrap();
        assert_eq!(rule.key_in("KD-12-login"), Some("KD-12"));
        assert!(KeyPattern::new(r"(?P<key>a)(?P<key>b)").is_none());
    }
}
