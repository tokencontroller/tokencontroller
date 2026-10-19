//! Token counts by kind, as the session files report them.

use tokencontroller_core::TokenCounts;

/// Tokens of one model call or a sum of them. Cache writes are kept by cache
/// lifetime because the files report them apart; everywhere outside the
/// readers they are one kind, `cache_write`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
}

impl Tokens {
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    /// The four kinds of the token entry schema and the core.
    pub fn kinds(self) -> TokenCounts {
        TokenCounts {
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
        let kinds = a.kinds();
        assert_eq!(kinds.cache_write, 7);
        assert_eq!(kinds.cache_read, 5);
        assert!(Tokens::default().is_zero());
        assert!(!a.is_zero());
    }

    #[test]
    fn one_cache_write_of_either_kind_is_not_zero() {
        let hour = Tokens {
            cache_write_1h: 1,
            ..Tokens::default()
        };
        assert!(!hour.is_zero());
        assert_eq!(hour.kinds().cache_write, 1);
        let kinds = Tokens {
            input: 10,
            output: 20,
            ..Tokens::default()
        }
        .kinds();
        assert_eq!((kinds.input, kinds.output, kinds.cache_read), (10, 20, 0));
    }
}
