//! Token counts by kind.

use serde::Serialize;

/// Tokens of one model call or a sum of them. Cache writes are kept by cache
/// lifetime because a one-hour write costs more; everywhere outside this
/// machine they are one kind, `cache_write`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
}

/// The four kinds the token entry schema uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TokenKinds {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Tokens {
    /// Adds two counts, saturating: no session comes near the limit.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_write_5m: self.cache_write_5m.saturating_add(other.cache_write_5m),
            cache_write_1h: self.cache_write_1h.saturating_add(other.cache_write_1h),
            cache_read: self.cache_read.saturating_add(other.cache_read),
        }
    }

    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    pub fn kinds(self) -> TokenKinds {
        TokenKinds {
            input: self.input,
            output: self.output,
            cache_write: self.cache_write_5m.saturating_add(self.cache_write_1h),
            cache_read: self.cache_read,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_add_up_and_merge_cache_writes() {
        let a = Tokens {
            input: 1,
            output: 2,
            cache_write_5m: 3,
            cache_write_1h: 4,
            cache_read: 5,
        };
        let sum = a.plus(a);
        assert_eq!(sum.input, 2);
        assert_eq!(sum.cache_write_1h, 8);
        let kinds = sum.kinds();
        assert_eq!(kinds.cache_write, 14);
        assert_eq!(kinds.cache_read, 10);
        assert!(Tokens::default().is_zero());
        assert!(!a.is_zero());
        let max = Tokens {
            input: u64::MAX,
            ..Tokens::default()
        };
        assert_eq!(max.plus(a).input, u64::MAX);
    }
}
