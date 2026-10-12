//! The one HTTP client every outside call uses: bill pulls, ticket sync and
//! the price refresh. TLS through rustls with the ring provider and the
//! system's certificates.

use std::time::Duration;

/// Sent as `User-Agent` on every outside call.
pub const USER_AGENT: &str = concat!("TokenController/", env!("CARGO_PKG_VERSION"));

/// Builds the HTTP client for outside calls, with a timeout per request.
///
/// # Errors
///
/// Fails only if the TLS setup cannot be built.
pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    // Installing twice is harmless: the second call keeps the first provider.
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_mins(1))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_builds_and_says_who_it_is() {
        assert!(client().is_ok());
        assert!(client().is_ok());
        assert_eq!(USER_AGENT, "TokenController/1.0.0");
    }
}
