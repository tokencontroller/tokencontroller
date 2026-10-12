//! Keys for outside systems (bill sources and ticket systems), sealed with a
//! key derived from `TC_SECRET_KEY` before they are stored, so a copy of the
//! database alone does not give them away.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use chacha20poly1305::aead::{Aead as _, AeadCore as _, KeyInit as _, OsRng};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest as _, Sha256};

/// The prefix of every sealed value, so a later change of scheme can tell
/// old values apart.
const VERSION: &str = "v1:";

/// Bytes of the nonce stored in front of the sealed bytes.
const NONCE_LEN: usize = 24;

/// Why a sealed value cannot be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("the stored key was sealed in an unknown format")]
    Format,
    #[error("the stored key cannot be opened: was TC_SECRET_KEY changed?")]
    WrongKey,
}

fn cipher(secret_key: &str) -> XChaCha20Poly1305 {
    let mut hasher = Sha256::new();
    hasher.update(b"tokencontroller.integrations.v1\0");
    hasher.update(secret_key.as_bytes());
    let key: [u8; 32] = hasher.finalize().into();
    XChaCha20Poly1305::new(&key.into())
}

/// Seals `plain` with a key derived from `secret_key`. Every call uses a new
/// random nonce, so sealing the same value twice gives different text.
///
/// # Errors
///
/// [`SecretError::Format`] only if the cipher refuses the input, which does
/// not happen for keys of normal length.
pub fn seal(secret_key: &str, plain: &str) -> Result<String, SecretError> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = cipher(secret_key)
        .encrypt(&nonce, plain.as_bytes())
        .map_err(|_| SecretError::Format)?;
    let mut bytes = nonce.to_vec();
    bytes.extend_from_slice(&sealed);
    Ok(format!("{VERSION}{}", STANDARD_NO_PAD.encode(bytes)))
}

/// Opens a value [`seal`] made with the same `secret_key`.
///
/// # Errors
///
/// [`SecretError`] when the value is malformed or the key differs.
pub fn open(secret_key: &str, sealed: &str) -> Result<String, SecretError> {
    let encoded = sealed.strip_prefix(VERSION).ok_or(SecretError::Format)?;
    let bytes = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| SecretError::Format)?;
    let (nonce, body) = bytes
        .split_at_checked(NONCE_LEN)
        .ok_or(SecretError::Format)?;
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SecretError::Format)?;
    let plain = cipher(secret_key)
        .decrypt(&XNonce::from(nonce), body)
        .map_err(|_| SecretError::WrongKey)?;
    String::from_utf8(plain).map_err(|_| SecretError::Format)
}

/// The last four characters of a key, for showing which key is stored
/// without showing the key.
#[must_use]
pub fn hint(plain: &str) -> String {
    let chars: Vec<char> = plain.chars().collect();
    let tail: String = chars.iter().skip(chars.len().saturating_sub(4)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_sealed_key_opens_with_the_same_secret_only() {
        let sealed = seal(KEY, "sk-ant-admin01-abc").unwrap();
        assert!(sealed.starts_with("v1:"));
        assert!(!sealed.contains("sk-ant"));
        assert_eq!(open(KEY, &sealed).unwrap(), "sk-ant-admin01-abc");
        assert_eq!(
            open("another secret key of enough length!", &sealed),
            Err(SecretError::WrongKey)
        );
    }

    #[test]
    fn sealing_twice_gives_different_text() {
        assert_ne!(seal(KEY, "same").unwrap(), seal(KEY, "same").unwrap());
    }

    #[test]
    fn malformed_values_are_refused() {
        assert_eq!(open(KEY, "plain"), Err(SecretError::Format));
        assert_eq!(open(KEY, "v1:!!!"), Err(SecretError::Format));
        assert_eq!(open(KEY, "v1:AAAA"), Err(SecretError::Format));
    }

    #[test]
    fn hints_show_only_the_end() {
        assert_eq!(hint("sk-ant-admin01-abcd"), "…abcd");
        assert_eq!(hint("ab"), "…ab");
    }
}
