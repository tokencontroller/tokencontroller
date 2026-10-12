//! Finding ticket keys in branch names, titles and prompts with the projects'
//! ticket key patterns. Only the keys found leave the machine, never the text.
//! The patterns are the core's, so the machine finds the same keys the
//! service does (ADR 0010).

use tokencontroller_core::pattern::TicketKeyPattern;

/// One project's ticket key pattern, ready to search text with.
#[derive(Debug, Clone)]
pub struct KeyPattern {
    /// The project the pattern belongs to, if any. A pattern set with
    /// `tc config ticket-key-pattern` has none.
    pub project: Option<String>,
    pattern: TicketKeyPattern,
}

impl KeyPattern {
    /// Compiles a pattern such as `^(KD|OPS)-\d+`. A leading `^` means the
    /// key starts a word. `None` for a pattern that is empty or is not a
    /// regular expression.
    pub fn new(project: Option<String>, pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        if pattern.strip_prefix('^').unwrap_or(pattern).is_empty() {
            return None;
        }
        let pattern = TicketKeyPattern::new(pattern).ok()?;
        Some(Self { project, pattern })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        self.pattern.as_str()
    }

    /// Every key in `text`, with the byte position it starts at, in order.
    pub fn find_at(&self, text: &str) -> Vec<(usize, String)> {
        self.pattern
            .find_all(text)
            .into_iter()
            .map(|(position, key)| (position, key.to_owned()))
            .collect()
    }

    pub fn matches_whole(&self, key: &str) -> bool {
        self.pattern.is_key(key)
    }
}

/// Every key any of the patterns finds in `text`, in order of appearance,
/// each once.
pub fn find_all(patterns: &[KeyPattern], text: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for pattern in patterns {
        for (position, key) in pattern.find_at(text) {
            if !found.iter().any(|(_, existing)| *existing == key) {
                found.push((position, key));
            }
        }
    }
    found.sort_by_key(|(position, _)| *position);
    found.into_iter().map(|(_, key)| key).collect()
}

/// The project whose pattern a key belongs to.
pub fn project_of(patterns: &[KeyPattern], key: &str) -> Option<String> {
    patterns
        .iter()
        .find(|pattern| pattern.matches_whole(key))
        .and_then(|pattern| pattern.project.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pattern: &KeyPattern, text: &str) -> Vec<String> {
        pattern
            .find_at(text)
            .into_iter()
            .map(|(_, key)| key)
            .collect()
    }

    fn jira() -> KeyPattern {
        KeyPattern::new(Some("storefront".into()), r"^(KD|OPS)-\d+").unwrap()
    }

    #[test]
    fn keys_are_found_in_branches_titles_and_prompts() {
        let pattern = jira();
        assert_eq!(keys(&pattern, "KD-214-fix-checkout"), ["KD-214"]);
        assert_eq!(keys(&pattern, "feature/OPS-7"), ["OPS-7"]);
        assert_eq!(
            keys(&pattern, "Fix KD-214, then KD-231 (and KD-214 again)"),
            ["KD-214", "KD-231", "KD-214"]
        );
        assert_eq!(keys(&pattern, "work on kd-214"), Vec::<String>::new());
    }

    #[test]
    fn a_leading_caret_means_the_key_starts_a_word() {
        let pattern = jira();
        assert!(keys(&pattern, "XKD-214").is_empty());
        assert!(keys(&pattern, "2KD-214").is_empty());
        assert_eq!(keys(&pattern, "(KD-9)"), ["KD-9"]);
        let anywhere = KeyPattern::new(None, r"KD-\d+").unwrap();
        assert_eq!(keys(&anywhere, "XKD-214"), ["KD-214"]);
    }

    #[test]
    fn github_issue_patterns_work_too() {
        let pattern = KeyPattern::new(None, r"storefront#\d+").unwrap();
        assert_eq!(keys(&pattern, "fixes storefront#42."), ["storefront#42"]);
        assert!(pattern.matches_whole("storefront#42"));
        assert!(!pattern.matches_whole("storefront#42x"));
        assert_eq!(pattern.as_str(), r"storefront#\d+");
    }

    #[test]
    fn empty_and_broken_patterns_are_refused() {
        let either = KeyPattern::new(None, r"KD-\d+|OPS-\d+").unwrap();
        assert_eq!(keys(&either, "x OPS-2 y KD-3"), ["OPS-2", "KD-3"]);
        assert!(KeyPattern::new(None, "^").is_none());
        assert!(KeyPattern::new(None, " ").is_none());
        assert!(KeyPattern::new(None, "(").is_none());
    }

    #[test]
    fn several_patterns_give_keys_in_text_order_once() {
        let patterns = [
            jira(),
            KeyPattern::new(Some("app".into()), r"APP-\d+").unwrap(),
        ];
        assert_eq!(
            find_all(&patterns, "APP-1 then KD-2 then APP-1"),
            ["APP-1", "KD-2"]
        );
        assert_eq!(project_of(&patterns, "APP-1").as_deref(), Some("app"));
        assert_eq!(project_of(&patterns, "KD-2").as_deref(), Some("storefront"));
        assert_eq!(project_of(&patterns, "ZZ-2"), None);
    }
}
