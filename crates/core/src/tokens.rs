//! Token counts by kind, as agents report them per message.
//!
//! A token kind is an open name: `input`, `output`, `cache_read`,
//! `cache_write`, `cache_write_5m`, `cache_write_1h` today, and whatever a
//! provider prices next. Only kinds with a price row are summed; an unknown
//! kind is listed and leaves the rest of the message priced (ADR 0028).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The longest token kind name, in characters.
const MAX_KIND_LENGTH: usize = 64;

/// A token kind name that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{0:?} is not a token kind: use up to 64 small letters, digits and _, starting with a letter"
)]
pub struct TokenKindError(pub String);

/// The name of a kind of tokens a provider prices on its own, such as
/// `input` or `cache_read`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TokenKind(Cow<'static, str>);

impl TokenKind {
    pub const INPUT: Self = Self(Cow::Borrowed("input"));
    pub const OUTPUT: Self = Self(Cow::Borrowed("output"));
    pub const CACHE_READ: Self = Self(Cow::Borrowed("cache_read"));
    /// A cache write whose duration the source did not say. Priced as a
    /// 5-minute write.
    pub const CACHE_WRITE: Self = Self(Cow::Borrowed("cache_write"));
    pub const CACHE_WRITE_5M: Self = Self(Cow::Borrowed("cache_write_5m"));
    pub const CACHE_WRITE_1H: Self = Self(Cow::Borrowed("cache_write_1h"));

    /// The kinds every agent reports today, in the order the docs list them.
    pub const COMMON: [Self; 4] = [
        Self::INPUT,
        Self::OUTPUT,
        Self::CACHE_WRITE,
        Self::CACHE_READ,
    ];

    /// Reads a kind name: small ASCII letters, digits and `_`, starting
    /// with a letter, at most 64 characters.
    ///
    /// # Errors
    ///
    /// [`TokenKindError`] for anything else.
    pub fn new(name: &str) -> Result<Self, TokenKindError> {
        let starts_with_letter = name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        let allowed = name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if starts_with_letter && allowed && name.len() <= MAX_KIND_LENGTH {
            Ok(Self(Cow::Owned(name.to_owned())))
        } else {
            Err(TokenKindError(name.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The kind priced the same way when this kind has no price row of its
    /// own: a cache write of unknown duration is priced as 5 minutes, and a
    /// 5-minute write takes the price of a plain cache write.
    #[must_use]
    pub fn priced_as(&self) -> Option<Self> {
        if *self == Self::CACHE_WRITE {
            Some(Self::CACHE_WRITE_5M)
        } else if *self == Self::CACHE_WRITE_5M {
            Some(Self::CACHE_WRITE)
        } else {
            None
        }
    }
}

impl TryFrom<String> for TokenKind {
    type Error = TokenKindError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(&name)
    }
}

impl From<TokenKind> for String {
    fn from(kind: TokenKind) -> Self {
        kind.0.into_owned()
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Token counts by kind: the `tokens` field of a message. A kind with no
/// tokens is not kept, so two counts with the same tokens are equal. As
/// JSON `input`, `output`, `cache_read` and `cache_write` are always
/// written, zero or not, so a message with zero tokens never reads as one with no counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "BTreeMap<TokenKind, u64>", into = "BTreeMap<TokenKind, u64>")]
pub struct TokenCounts(BTreeMap<TokenKind, u64>);

impl From<BTreeMap<TokenKind, u64>> for TokenCounts {
    fn from(counts: BTreeMap<TokenKind, u64>) -> Self {
        counts.into_iter().collect()
    }
}

impl From<TokenCounts> for BTreeMap<TokenKind, u64> {
    fn from(counts: TokenCounts) -> Self {
        let mut all = counts.0;
        for kind in TokenKind::COMMON {
            all.entry(kind).or_insert(0);
        }
        all
    }
}

impl FromIterator<(TokenKind, u64)> for TokenCounts {
    fn from_iter<I: IntoIterator<Item = (TokenKind, u64)>>(iter: I) -> Self {
        let mut counts = Self::default();
        for (kind, count) in iter {
            counts.set(kind, count);
        }
        counts
    }
}

impl TokenCounts {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts of `input`, `output`, `cache_write` and `cache_read`; add other
    /// kinds with [`Self::with`].
    #[must_use]
    pub fn common(input: u64, output: u64, cache_write: u64, cache_read: u64) -> Self {
        Self::new()
            .with(TokenKind::INPUT, input)
            .with(TokenKind::OUTPUT, output)
            .with(TokenKind::CACHE_WRITE, cache_write)
            .with(TokenKind::CACHE_READ, cache_read)
    }

    /// These counts with one kind set, such as
    /// `TokenCounts::new().with(TokenKind::INPUT, 10)`.
    #[must_use]
    pub fn with(mut self, kind: TokenKind, count: u64) -> Self {
        self.set(kind, count);
        self
    }

    /// Sets the count of one kind. Zero removes the kind.
    pub fn set(&mut self, kind: TokenKind, count: u64) {
        if count == 0 {
            self.0.remove(&kind);
        } else {
            self.0.insert(kind, count);
        }
    }

    /// The count of one kind, zero when the kind is not there.
    #[must_use]
    pub fn get(&self, kind: &TokenKind) -> u64 {
        self.0.get(kind).copied().unwrap_or(0)
    }

    /// Every kind with tokens and its count, by kind name.
    pub fn iter(&self) -> impl Iterator<Item = (&TokenKind, u64)> {
        self.0.iter().map(|(kind, count)| (kind, *count))
    }

    /// The kinds that are summed into the cost: every kind with tokens,
    /// except a plain cache write when the same call also gives its
    /// 5-minute or 1-hour counts, since those are the same tokens told
    /// finer.
    pub fn summed(&self) -> impl Iterator<Item = (&TokenKind, u64)> {
        let finer =
            self.get(&TokenKind::CACHE_WRITE_5M) > 0 || self.get(&TokenKind::CACHE_WRITE_1H) > 0;
        self.iter()
            .filter(move |(kind, _)| !(finer && **kind == TokenKind::CACHE_WRITE))
    }

    /// The counts as two sources of the same call compare them: cache
    /// writes of every duration added into one plain cache write.
    #[must_use]
    pub fn compared(&self) -> Self {
        let mut compared = Self::default();
        let mut writes = 0_u64;
        for (kind, count) in self.summed() {
            if *kind == TokenKind::CACHE_WRITE
                || *kind == TokenKind::CACHE_WRITE_5M
                || *kind == TokenKind::CACHE_WRITE_1H
            {
                writes = writes.saturating_add(count);
            } else {
                compared.set(kind.clone(), count);
            }
        }
        compared.set(TokenKind::CACHE_WRITE, writes);
        compared
    }

    /// Adds two counts kind by kind, or `None` if a count would overflow.
    #[must_use]
    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        let mut sum = self.clone();
        for (kind, count) in other.iter() {
            let added = sum.get(kind).checked_add(count)?;
            sum.set(kind.clone(), added);
        }
        Some(sum)
    }

    /// All kinds together, or `None` if the total would overflow.
    #[must_use]
    pub fn total(&self) -> Option<u64> {
        self.0
            .values()
            .try_fold(0_u64, |sum, count| sum.checked_add(*count))
    }

    /// Whether every count is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts() -> TokenCounts {
        TokenCounts::common(1, 2, 3, 4)
    }

    #[test]
    fn kinds_are_open_lowercase_names() {
        for good in ["input", "reasoning", "cache_write_1h", "a"] {
            assert_eq!(TokenKind::new(good).unwrap().as_str(), good);
        }
        assert!(TokenKind::new(&"a".repeat(64)).is_ok());
        for bad in ["", "Input", "1h", "cache-write", " input", &"a".repeat(65)] {
            assert_eq!(TokenKind::new(bad), Err(TokenKindError(bad.to_owned())));
        }
        assert_eq!(TokenKind::new("input").unwrap(), TokenKind::INPUT);
        assert_eq!(TokenKind::CACHE_WRITE_1H.to_string(), "cache_write_1h");
        assert_eq!(
            serde_json::to_string(&TokenKind::CACHE_READ).unwrap(),
            r#""cache_read""#
        );
        assert!(serde_json::from_str::<TokenKind>(r#""Bad""#).is_err());
        assert_eq!(String::from(TokenKind::OUTPUT), "output");
    }

    #[test]
    fn a_cache_write_of_unknown_duration_is_priced_as_five_minutes() {
        assert_eq!(
            TokenKind::CACHE_WRITE.priced_as(),
            Some(TokenKind::CACHE_WRITE_5M)
        );
        assert_eq!(
            TokenKind::CACHE_WRITE_5M.priced_as(),
            Some(TokenKind::CACHE_WRITE)
        );
        assert_eq!(TokenKind::CACHE_WRITE_1H.priced_as(), None);
        assert_eq!(TokenKind::INPUT.priced_as(), None);
    }

    #[test]
    fn counts_are_read_by_kind_and_zero_is_not_kept() {
        let read: Vec<u64> = TokenKind::COMMON
            .iter()
            .map(|kind| counts().get(kind))
            .collect();
        assert_eq!(read, vec![1, 2, 3, 4]);
        assert_eq!(counts().get(&TokenKind::new("reasoning").unwrap()), 0);
        let mut zeroed = counts();
        zeroed.set(TokenKind::INPUT, 0);
        assert_eq!(zeroed.get(&TokenKind::INPUT), 0);
        assert_eq!(zeroed.iter().count(), 3);
        assert_eq!(
            TokenCounts::new().with(TokenKind::INPUT, 0),
            TokenCounts::new()
        );
    }

    #[test]
    fn counts_add_kind_by_kind_and_never_wrap() {
        let doubled = counts().checked_add(&counts()).unwrap();
        assert_eq!(
            doubled,
            TokenCounts::new()
                .with(TokenKind::INPUT, 2)
                .with(TokenKind::OUTPUT, 4)
                .with(TokenKind::CACHE_WRITE, 6)
                .with(TokenKind::CACHE_READ, 8)
        );
        assert_eq!(doubled.total(), Some(20));
        let big = TokenCounts::new().with(TokenKind::INPUT, u64::MAX);
        assert_eq!(big.checked_add(&counts()), None);
        assert_eq!(big.total(), Some(u64::MAX));
        assert_eq!(big.checked_add(&TokenCounts::new()), Some(big.clone()));
        let too_big = big.with(TokenKind::OUTPUT, 1);
        assert_eq!(too_big.total(), None);
    }

    #[test]
    fn zero_means_no_kind_has_tokens() {
        assert!(TokenCounts::new().is_zero());
        assert!(!counts().is_zero());
        assert!(!TokenCounts::new().with(TokenKind::CACHE_READ, 1).is_zero());
    }

    #[test]
    fn finer_cache_writes_win_over_the_plain_one() {
        let plain = TokenCounts::new()
            .with(TokenKind::INPUT, 5)
            .with(TokenKind::CACHE_WRITE, 10);
        let summed: Vec<&str> = plain.summed().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(summed, vec!["cache_write", "input"]);
        for finer in [TokenKind::CACHE_WRITE_5M, TokenKind::CACHE_WRITE_1H] {
            let both = plain.clone().with(finer.clone(), 4);
            let summed: Vec<&str> = both.summed().map(|(kind, _)| kind.as_str()).collect();
            assert_eq!(summed, vec![finer.as_str(), "input"]);
        }
        // Two sources of one call compare equal however finely they tell
        // the cache writes.
        let file = TokenCounts::new()
            .with(TokenKind::INPUT, 5)
            .with(TokenKind::CACHE_WRITE_5M, 6)
            .with(TokenKind::CACHE_WRITE_1H, 4);
        assert_eq!(file.compared(), plain.compared());
        assert_eq!(plain.compared().get(&TokenKind::CACHE_WRITE), 10);
        assert_ne!(
            file.compared(),
            plain.clone().with(TokenKind::INPUT, 6).compared()
        );
    }

    #[test]
    fn counts_read_from_json_with_any_kind() {
        let counts: TokenCounts =
            serde_json::from_str(r#"{"input": 5, "output": 7, "cache_read": 0, "reasoning": 3}"#)
                .unwrap();
        assert_eq!(counts.get(&TokenKind::INPUT), 5);
        assert_eq!(counts.get(&TokenKind::OUTPUT), 7);
        assert_eq!(counts.get(&TokenKind::CACHE_READ), 0);
        assert_eq!(counts.get(&TokenKind::new("reasoning").unwrap()), 3);
        assert_eq!(
            serde_json::to_string(&counts).unwrap(),
            r#"{"cache_read":0,"cache_write":0,"input":5,"output":7,"reasoning":3}"#
        );
        assert!(serde_json::from_str::<TokenCounts>(r#"{"Thinking": 1}"#).is_err());
    }
}
