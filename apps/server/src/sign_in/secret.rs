//! Random secrets for sign-in links, sessions and API tokens.
//!
//! A secret is 32 random bytes behind a prefix that says what it is, such as
//! `tc_svc_`. The database keeps only an HMAC-SHA256 of it, keyed with
//! `TC_SECRET_KEY`: a copy of the database alone signs nobody in, and a new
//! secret key ends every session and token at once.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;

/// Prefix of a sign-in link's token.
pub const LINK: &str = "tc_link_";
/// Prefix of the secret a local tool holds while it waits for its session.
pub const POLL: &str = "tc_poll_";
/// Prefix of a session token: the browser cookie, or a local tool's bearer
/// token.
pub const SESSION: &str = "tc_ses_";
/// Prefix of a service token, for agents that report token entries.
pub const SERVICE: &str = "tc_svc_";
/// Prefix of a read token, for reports.
pub const READ: &str = "tc_read_";
/// Prefix of the organization token, for telemetry push.
pub const ORGANIZATION: &str = "tc_org_";

/// How many characters of a token stay visible in lists, prefix included.
const SHOWN_RANDOM_CHARS: usize = 4;

/// A new secret: the token to hand out once, and the hash to keep.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub token: String,
    pub hash: Vec<u8>,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<hidden>)")
    }
}

impl Secret {
    /// The start of the token, such as `tc_svc_3f9a`, to tell tokens apart
    /// in a list without showing them.
    #[must_use]
    pub fn shown_prefix(&self) -> String {
        shown_prefix(&self.token)
    }
}

/// Makes a new random secret with `prefix`.
///
/// # Errors
///
/// Fails only if the operating system has no randomness to give.
pub fn generate(prefix: &str, key: &str) -> Result<Secret, getrandom::Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    let token = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes));
    let hash = hash(&token, key);
    Ok(Secret { token, hash })
}

/// The hash of `token` as the database keeps it.
#[must_use]
pub fn hash(token: &str, key: &str) -> Vec<u8> {
    // HMAC accepts keys of any length, so this cannot fail.
    Hmac::<Sha256>::new_from_slice(key.as_bytes()).map_or_else(
        |_| Vec::new(),
        |mut mac| {
            mac.update(token.as_bytes());
            mac.finalize().into_bytes().to_vec()
        },
    )
}

/// A few random characters, such as `4f2a9c1e7b03`, for invite codes.
///
/// # Errors
///
/// Fails only if the operating system has no randomness to give.
pub fn short_code() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 6];
    getrandom::fill(&mut bytes)?;
    Ok(hex::encode(bytes))
}

fn shown_prefix(token: &str) -> String {
    let prefix_len = [SERVICE, READ, ORGANIZATION, SESSION, LINK, POLL]
        .iter()
        .find(|prefix| token.starts_with(**prefix))
        .map_or(0, |prefix| prefix.len());
    token
        .chars()
        .take(prefix_len.saturating_add(SHOWN_RANDOM_CHARS))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn secrets_carry_their_prefix_and_differ() {
        let first = generate(SERVICE, KEY).unwrap();
        let second = generate(SERVICE, KEY).unwrap();
        assert!(first.token.starts_with("tc_svc_"));
        assert_ne!(first.token, second.token);
        assert_eq!(first.token.len(), "tc_svc_".len() + 43);
    }

    #[test]
    fn the_hash_depends_on_the_token_and_the_key() {
        let secret = generate(READ, KEY).unwrap();
        assert_eq!(secret.hash, hash(&secret.token, KEY));
        assert_eq!(secret.hash.len(), 32);
        assert_ne!(
            secret.hash,
            hash(&secret.token, "another key of at least 32 chars")
        );
        assert_ne!(secret.hash, hash("tc_read_other", KEY));
    }

    #[test]
    fn lists_show_the_prefix_and_four_characters() {
        assert_eq!(shown_prefix("tc_svc_abcdefgh"), "tc_svc_abcd");
        assert_eq!(shown_prefix("tc_read_abcdefgh"), "tc_read_abcd");
        assert_eq!(shown_prefix("unknown"), "unkn");
    }

    #[test]
    fn debug_output_hides_the_token() {
        let secret = generate(ORGANIZATION, KEY).unwrap();
        assert!(!format!("{secret:?}").contains(&secret.token));
    }

    #[test]
    fn short_codes_are_twelve_hex_characters() {
        let code = short_code().unwrap();
        assert_eq!(code.len(), 12);
        assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
