//! Token counts by kind, as the session files report them.

use tokencontroller_core::{TokenCounts, TokenKind};

/// Tokens of one model call or a sum of them. Cache writes are kept by cache
/// lifetime because the files report them apart.
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

    /// The counts by kind for the token entry schema and the core. A call
    /// with 1-hour cache writes sends `cache_write_5m` and `cache_write_1h`
    /// apart, so each is priced at its own row; otherwise the writes go as
    /// `cache_write`, priced as 5 minutes.
    pub fn kinds(self) -> TokenCounts {
        if self.cache_write_1h > 0 {
            TokenCounts::common(self.input, self.output, 0, self.cache_read)
                .with(TokenKind::CACHE_WRITE_5M, self.cache_write_5m)
                .with(TokenKind::CACHE_WRITE_1H, self.cache_write_1h)
        } else {
            TokenCounts::common(
                self.input,
                self.output,
                self.cache_write_5m,
                self.cache_read,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hour_cache_writes_go_apart() {
        let a = Tokens {
            input: 1,
            output: 2,
            cache_write_5m: 3,
            cache_write_1h: 4,
            cache_read: 5,
        };
        let kinds = a.kinds();
        assert_eq!(kinds.get(&TokenKind::CACHE_WRITE), 0);
        assert_eq!(kinds.get(&TokenKind::CACHE_WRITE_5M), 3);
        assert_eq!(kinds.get(&TokenKind::CACHE_WRITE_1H), 4);
        let five = Tokens {
            cache_write_5m: 3,
            ..Tokens::default()
        };
        assert_eq!(five.kinds().get(&TokenKind::CACHE_WRITE), 3);
        assert_eq!(five.kinds().get(&TokenKind::CACHE_WRITE_5M), 0);
        assert_eq!(kinds.get(&tokencontroller_core::TokenKind::CACHE_READ), 5);
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
        assert_eq!(
            hour.kinds()
                .get(&tokencontroller_core::TokenKind::CACHE_WRITE_1H),
            1
        );
        let kinds = Tokens {
            input: 10,
            output: 20,
            ..Tokens::default()
        }
        .kinds();
        assert_eq!(
            (
                kinds.get(&tokencontroller_core::TokenKind::INPUT),
                kinds.get(&tokencontroller_core::TokenKind::OUTPUT),
                kinds.get(&tokencontroller_core::TokenKind::CACHE_READ)
            ),
            (10, 20, 0)
        );
    }
}
