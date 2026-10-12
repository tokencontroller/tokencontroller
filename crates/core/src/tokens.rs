//! Token counts by kind, as agents report them per message.

use serde::{Deserialize, Serialize};

/// The kinds of tokens a provider prices separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Input,
    Output,
    CacheWrite,
    CacheRead,
}

impl TokenKind {
    /// Every kind, in the order the docs list them.
    pub const ALL: [Self; 4] = [Self::Input, Self::Output, Self::CacheWrite, Self::CacheRead];
}

/// Token counts by kind: the `tokens` field of a token entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenCounts {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub cache_read: u64,
}

impl TokenCounts {
    /// The count of one kind.
    #[must_use]
    pub const fn get(self, kind: TokenKind) -> u64 {
        match kind {
            TokenKind::Input => self.input,
            TokenKind::Output => self.output,
            TokenKind::CacheWrite => self.cache_write,
            TokenKind::CacheRead => self.cache_read,
        }
    }

    /// Adds two counts kind by kind, or `None` if a count would overflow.
    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_add(other.input)?,
            output: self.output.checked_add(other.output)?,
            cache_write: self.cache_write.checked_add(other.cache_write)?,
            cache_read: self.cache_read.checked_add(other.cache_read)?,
        })
    }

    /// All kinds together, or `None` if the total would overflow.
    #[must_use]
    pub fn total(self) -> Option<u64> {
        TokenKind::ALL
            .iter()
            .try_fold(0_u64, |sum, kind| sum.checked_add(self.get(*kind)))
    }

    /// Whether every count is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_write == 0 && self.cache_read == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS: TokenCounts = TokenCounts {
        input: 1,
        output: 2,
        cache_write: 3,
        cache_read: 4,
    };

    #[test]
    fn counts_are_read_by_kind() {
        let read: Vec<u64> = TokenKind::ALL
            .iter()
            .map(|kind| COUNTS.get(*kind))
            .collect();
        assert_eq!(read, vec![1, 2, 3, 4]);
    }

    #[test]
    fn counts_add_kind_by_kind_and_never_wrap() {
        let doubled = COUNTS.checked_add(COUNTS).unwrap();
        assert_eq!(
            doubled,
            TokenCounts {
                input: 2,
                output: 4,
                cache_write: 6,
                cache_read: 8,
            }
        );
        assert_eq!(doubled.total(), Some(20));
        for kind in TokenKind::ALL {
            let mut big = TokenCounts::default();
            match kind {
                TokenKind::Input => big.input = u64::MAX,
                TokenKind::Output => big.output = u64::MAX,
                TokenKind::CacheWrite => big.cache_write = u64::MAX,
                TokenKind::CacheRead => big.cache_read = u64::MAX,
            }
            assert_eq!(big.checked_add(COUNTS), None, "{kind:?}");
            assert_eq!(big.total(), Some(u64::MAX));
            assert_eq!(big.checked_add(TokenCounts::default()), Some(big));
        }
        let all_big = TokenCounts {
            input: u64::MAX,
            output: 1,
            ..TokenCounts::default()
        };
        assert_eq!(all_big.total(), None);
    }

    #[test]
    fn zero_means_every_kind_is_zero() {
        assert!(TokenCounts::default().is_zero());
        for kind in TokenKind::ALL {
            let mut one = TokenCounts::default();
            match kind {
                TokenKind::Input => one.input = 1,
                TokenKind::Output => one.output = 1,
                TokenKind::CacheWrite => one.cache_write = 1,
                TokenKind::CacheRead => one.cache_read = 1,
            }
            assert!(!one.is_zero(), "{kind:?}");
        }
    }

    #[test]
    fn counts_read_from_json_with_missing_kinds_as_zero() {
        let counts: TokenCounts = serde_json::from_str(r#"{"input": 5, "output": 7}"#).unwrap();
        assert_eq!(counts.input, 5);
        assert_eq!(counts.output, 7);
        assert_eq!(counts.cache_read, 0);
        assert!(serde_json::from_str::<TokenCounts>(r#"{"thinking": 1}"#).is_err());
    }
}
