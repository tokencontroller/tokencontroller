//! Versioned settings: price tables and tag maps (ADR 0007).
//!
//! Every change makes a new version and nothing is overwritten, so a closed
//! period can name the versions it was closed with and every figure on its
//! cost sheet can be computed again from them.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The number of one version, counted from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(u32);

impl Version {
    /// The first version.
    pub const FIRST: Self = Self(1);

    /// A version by its number, or `None` for 0.
    #[must_use]
    pub const fn new(number: u32) -> Option<Self> {
        if number == 0 {
            None
        } else {
            Some(Self(number))
        }
    }

    #[must_use]
    pub const fn number(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Every version a setting has had, oldest first. Never empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<T>", into = "Vec<T>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de>"
))]
pub struct History<T> {
    versions: Vec<T>,
    first: T,
}

/// A history read from an empty list.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a history needs at least its first version")]
pub struct EmptyHistory;

impl<T> History<T> {
    /// A history with one version.
    pub fn new(first: T) -> Self {
        Self {
            versions: Vec::new(),
            first,
        }
    }

    /// The version in force now and its number.
    #[must_use]
    pub fn current(&self) -> (Version, &T) {
        let number = self
            .versions
            .len()
            .checked_add(1)
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or(u32::MAX);
        (Version(number), self.versions.last().unwrap_or(&self.first))
    }

    /// One version by its number.
    #[must_use]
    pub fn get(&self, version: Version) -> Option<&T> {
        match version.0.checked_sub(2) {
            None => Some(&self.first),
            Some(index) => usize::try_from(index)
                .ok()
                .and_then(|index| self.versions.get(index)),
        }
    }

    /// Every version, oldest first, with its number.
    pub fn iter(&self) -> impl Iterator<Item = (Version, &T)> {
        std::iter::once(&self.first)
            .chain(&self.versions)
            .zip(1_u32..)
            .map(|(value, number)| (Version(number), value))
    }
}

impl<T: PartialEq> History<T> {
    /// Records a new state. It becomes a new version only if it differs
    /// from the current one. Returns the version in force afterwards.
    pub fn record(&mut self, next: T) -> Version {
        if *self.current().1 != next {
            self.versions.push(next);
        }
        self.current().0
    }
}

impl<T> TryFrom<Vec<T>> for History<T> {
    type Error = EmptyHistory;

    fn try_from(mut versions: Vec<T>) -> Result<Self, Self::Error> {
        if versions.is_empty() {
            return Err(EmptyHistory);
        }
        let first = versions.remove(0);
        Ok(Self { versions, first })
    }
}

impl<T> From<History<T>> for Vec<T> {
    fn from(history: History<T>) -> Self {
        std::iter::once(history.first)
            .chain(history.versions)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_count_from_one() {
        assert_eq!(Version::new(0), None);
        assert_eq!(Version::new(1), Some(Version::FIRST));
        assert_eq!(Version::new(7).unwrap().number(), 7);
        assert_eq!(Version::new(7).unwrap().to_string(), "7");
    }

    #[test]
    fn only_a_change_makes_a_new_version() {
        let mut history = History::new("a");
        assert_eq!(history.current(), (Version::FIRST, &"a"));
        assert_eq!(history.record("a"), Version::FIRST);
        let second = history.record("b");
        assert_eq!(second.number(), 2);
        assert_eq!(history.record("b"), second);
        let third = history.record("a");
        assert_eq!(third.number(), 3);
        assert_eq!(history.current(), (third, &"a"));
    }

    #[test]
    fn old_versions_stay_readable() {
        let mut history = History::new(10);
        history.record(20);
        history.record(30);
        assert_eq!(history.get(Version::FIRST), Some(&10));
        assert_eq!(history.get(Version::new(2).unwrap()), Some(&20));
        assert_eq!(history.get(Version::new(3).unwrap()), Some(&30));
        assert_eq!(history.get(Version::new(4).unwrap()), None);
        let all: Vec<(u32, i32)> = history
            .iter()
            .map(|(version, value)| (version.number(), *value))
            .collect();
        assert_eq!(all, vec![(1, 10), (2, 20), (3, 30)]);
    }

    #[test]
    fn a_history_reads_and_writes_as_a_list() {
        let mut history = History::new("x".to_owned());
        history.record("y".to_owned());
        let json = serde_json::to_string(&history).unwrap();
        assert_eq!(json, r#"["x","y"]"#);
        assert_eq!(
            serde_json::from_str::<History<String>>(&json).unwrap(),
            history
        );
        assert!(serde_json::from_str::<History<String>>("[]").is_err());
        assert_eq!(History::<u8>::try_from(Vec::new()), Err(EmptyHistory));
    }
}
