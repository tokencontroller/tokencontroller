//! Finding ticket keys in branch names, titles and prompts with the projects'
//! ticket key patterns. Only the keys found leave the machine, never the text.

use regex::Regex;

/// One project's ticket key pattern, ready to search text with.
#[derive(Debug, Clone)]
pub struct KeyPattern {
    /// The project the pattern belongs to, if any. A pattern set with
    /// `tc config ticket-key-pattern` has none.
    pub project: Option<String>,
    regex: Regex,
}

impl KeyPattern {
    /// Compiles a pattern such as `^(KD|OPS)-\d+`. The pattern describes a
    /// whole key, so a leading `^` and a trailing `$` are dropped, and a key
    /// is found anywhere in a text where it does not touch other letters or
    /// digits on its left.
    pub fn new(project: Option<String>, pattern: &str) -> Option<Self> {
        let inner = pattern.trim();
        let inner = inner.strip_prefix('^').unwrap_or(inner);
        let inner = match inner.strip_suffix('$') {
            Some(rest) if !rest.ends_with('\\') => rest,
            _ => inner,
        };
        if inner.is_empty() {
            return None;
        }
        let regex = Regex::new(&format!(r"(?:^|[^\p{{L}}\p{{N}}_])(?P<key>(?:{inner}))")).ok()?;
        Some(Self { project, regex })
    }

    /// Every key in `text`, in order.
    pub fn find(&self, text: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for captures in self.regex.captures_iter(text) {
            let Some(key) = captures.name("key") else {
                continue;
            };
            // `KD-12` must not be found inside `KD-12a` or `KD-123x`.
            let next = text.get(key.end()..).and_then(|rest| rest.chars().next());
            if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            keys.push(key.as_str().to_owned());
        }
        keys
    }

    pub fn matches_whole(&self, key: &str) -> bool {
        self.find(key).first().is_some_and(|found| found == key)
    }
}

/// Every key any of the patterns finds in `text`, in order of appearance.
pub fn find_all(patterns: &[KeyPattern], text: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for pattern in patterns {
        for key in pattern.find(text) {
            let position = text.find(&key).unwrap_or(usize::MAX);
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

    fn jira() -> KeyPattern {
        KeyPattern::new(Some("storefront".into()), r"^(KD|OPS)-\d+").unwrap()
    }

    #[test]
    fn keys_are_found_in_branches_titles_and_prompts() {
        let pattern = jira();
        assert_eq!(pattern.find("KD-214-fix-checkout"), ["KD-214"]);
        assert_eq!(pattern.find("feature/OPS-7"), ["OPS-7"]);
        assert_eq!(
            pattern.find("Fix KD-214, then KD-231 (and KD-214 again)"),
            ["KD-214", "KD-231", "KD-214"]
        );
        assert_eq!(pattern.find("work on kd-214"), Vec::<String>::new());
    }

    #[test]
    fn keys_inside_other_words_are_not_keys() {
        let pattern = jira();
        assert!(pattern.find("XKD-214").is_empty());
        assert!(pattern.find("KD-214a").is_empty());
        assert!(pattern.find("KD-214_b").is_empty());
        assert!(pattern.find("ÄKD-214").is_empty());
        assert_eq!(pattern.find("(KD-9)"), ["KD-9"]);
    }

    #[test]
    fn github_issue_patterns_work_too() {
        let pattern = KeyPattern::new(None, r"storefront#\d+").unwrap();
        assert_eq!(pattern.find("fixes storefront#42."), ["storefront#42"]);
        assert!(pattern.matches_whole("storefront#42"));
        assert!(!pattern.matches_whole("storefront#42x"));
    }

    #[test]
    fn anchors_and_alternatives_are_handled() {
        let pattern = KeyPattern::new(None, r"^KD-\d+$").unwrap();
        assert_eq!(pattern.find("a KD-1 b"), ["KD-1"]);
        let either = KeyPattern::new(None, r"KD-\d+|OPS-\d+").unwrap();
        assert_eq!(either.find("x OPS-2 y KD-3"), ["OPS-2", "KD-3"]);
        let escaped = KeyPattern::new(None, r"KD-\d+\$").unwrap();
        assert_eq!(escaped.find("KD-1$"), ["KD-1$"]);
        assert!(KeyPattern::new(None, "^").is_none());
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
