//! Configuration from environment variables, exactly as the docs page
//! Self-hosting > Configuration lists them.

use std::fmt;

/// The shortest `TC_SECRET_KEY` the service accepts.
pub const MIN_SECRET_KEY_LEN: usize = 32;

/// Why the service refuses to start.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is not set. See docs.tokencontroller.com/self-hosting/configuration")]
    Missing(&'static str),
    #[error(
        "TC_SECRET_KEY is too short: use a random string of at least {MIN_SECRET_KEY_LEN} characters, for example from `openssl rand -hex 32`"
    )]
    SecretKeyTooShort,
    #[error("TC_PUBLIC_URL must start with https:// or http://, such as https://tc.example.com")]
    PublicUrl,
    #[error("TC_PRICES_REFRESH must be `on` or `off`, not {0:?}")]
    PricesRefresh(String),
    #[error(transparent)]
    CustomerRelations(#[from] crate::customer_relations::SettingsError),
    #[error(
        "TC_STRIPE_SECRET_KEY and TC_STRIPE_WEBHOOK_SECRET go together: set both for Team plan billing, or neither"
    )]
    StripeHalfSet,
}

/// The service's settings.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// `DATABASE_URL`: the Postgres connection string.
    pub database_url: String,
    /// `TC_PUBLIC_URL`: the address people and local tools use, without a
    /// trailing slash.
    pub public_url: String,
    /// `TC_SECRET_KEY`: signs sessions and tokens.
    pub secret_key: String,
    /// `TC_SMTP_URL`: the mail server for magic links and reminders.
    pub smtp_url: Option<String>,
    /// `TC_SMTP_FROM`: the sender of every mail, such as
    /// `Token Controller <mail@tokencontroller.com>`. Without it, mail comes
    /// from `noreply@` the host of `TC_PUBLIC_URL`.
    pub smtp_from: Option<String>,
    /// `TC_PRICES_REFRESH`: the daily price table refresh, on unless `off`.
    pub prices_refresh: bool,
    /// The support inbox, site assistant and newsletter settings, all off
    /// unless set.
    pub customer_relations: crate::customer_relations::Settings,
    /// Team plan billing through Stripe, only on the hosted service. `None`
    /// keeps billing off, as on every self-hosted instance.
    pub stripe: Option<StripeConfig>,
}

/// `TC_STRIPE_SECRET_KEY` and `TC_STRIPE_WEBHOOK_SECRET`.
#[derive(Clone, PartialEq, Eq)]
pub struct StripeConfig {
    /// The API key the service calls Stripe with.
    pub secret_key: String,
    /// The signing secret of the webhook endpoint.
    pub webhook_secret: String,
}

impl Config {
    /// Reads the configuration from the process environment.
    ///
    /// # Errors
    ///
    /// A [`ConfigError`] naming the variable to fix.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the configuration through `lookup`, which returns a variable's
    /// value or `None` when it is not set.
    ///
    /// # Errors
    ///
    /// A [`ConfigError`] naming the variable to fix.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let read = |name: &'static str| lookup(name).filter(|value| !value.trim().is_empty());
        let required = |name: &'static str| read(name).ok_or(ConfigError::Missing(name));

        let database_url = required("DATABASE_URL")?;

        let public_url = required("TC_PUBLIC_URL")?.trim_end_matches('/').to_owned();
        if !(public_url.starts_with("https://") || public_url.starts_with("http://")) {
            return Err(ConfigError::PublicUrl);
        }

        let secret_key = required("TC_SECRET_KEY")?;
        if secret_key.chars().count() < MIN_SECRET_KEY_LEN {
            return Err(ConfigError::SecretKeyTooShort);
        }

        let prices_refresh = match read("TC_PRICES_REFRESH").as_deref().map(str::trim) {
            None | Some("on") => true,
            Some("off") => false,
            Some(other) => return Err(ConfigError::PricesRefresh(other.to_owned())),
        };

        let customer_relations = crate::customer_relations::Settings::from_lookup(&lookup)?;
        let stripe = match (
            read("TC_STRIPE_SECRET_KEY"),
            read("TC_STRIPE_WEBHOOK_SECRET"),
        ) {
            (Some(secret_key), Some(webhook_secret)) => Some(StripeConfig {
                secret_key,
                webhook_secret,
            }),
            (None, None) => None,
            _ => return Err(ConfigError::StripeHalfSet),
        };

        Ok(Self {
            database_url,
            public_url,
            secret_key,
            smtp_url: read("TC_SMTP_URL"),
            smtp_from: read("TC_SMTP_FROM"),
            prices_refresh,
            customer_relations,
            stripe,
        })
    }
}

/// Never prints secrets, so the configuration can be logged.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<hidden>")
            .field("public_url", &self.public_url)
            .field("secret_key", &"<hidden>")
            .field("smtp_url", &self.smtp_url.as_ref().map(|_| "<hidden>"))
            .field("smtp_from", &self.smtp_from)
            .field("prices_refresh", &self.prices_refresh)
            .field("customer_relations", &self.customer_relations)
            .field("stripe", &self.stripe.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    fn minimal() -> Vec<(&'static str, &'static str)> {
        vec![
            ("DATABASE_URL", "postgres://tc:secret@db/tc"),
            ("TC_PUBLIC_URL", "https://tc.example.com/"),
            ("TC_SECRET_KEY", KEY),
        ]
    }

    #[test]
    fn the_three_required_variables_are_enough() {
        let config = config(&minimal()).unwrap();
        assert_eq!(config.public_url, "https://tc.example.com");
        assert_eq!(config.smtp_url, None);
        assert_eq!(config.smtp_from, None);
        assert!(config.prices_refresh);
        assert!(config.stripe.is_none());
    }

    #[test]
    fn stripe_needs_both_settings() {
        let mut pairs = minimal();
        pairs.push(("TC_STRIPE_SECRET_KEY", "sk_test_123"));
        assert_eq!(config(&pairs), Err(ConfigError::StripeHalfSet));

        pairs.push(("TC_STRIPE_WEBHOOK_SECRET", "whsec_456"));
        let stripe = config(&pairs).unwrap().stripe.unwrap();
        assert_eq!(stripe.secret_key, "sk_test_123");
        assert_eq!(stripe.webhook_secret, "whsec_456");
    }

    #[test]
    fn each_required_variable_is_named_when_missing() {
        for name in ["DATABASE_URL", "TC_PUBLIC_URL", "TC_SECRET_KEY"] {
            let pairs: Vec<_> = minimal().into_iter().filter(|(n, _)| *n != name).collect();
            assert_eq!(config(&pairs), Err(ConfigError::Missing(name)));
        }
    }

    #[test]
    fn a_short_secret_key_is_refused() {
        let mut pairs = minimal();
        pairs.retain(|(n, _)| *n != "TC_SECRET_KEY");
        pairs.push(("TC_SECRET_KEY", "change-me"));
        assert_eq!(config(&pairs), Err(ConfigError::SecretKeyTooShort));
    }

    #[test]
    fn the_price_refresh_can_be_switched_off() {
        let mut pairs = minimal();
        pairs.push(("TC_PRICES_REFRESH", "off"));
        assert!(!config(&pairs).unwrap().prices_refresh);

        let mut pairs = minimal();
        pairs.push(("TC_PRICES_REFRESH", "maybe"));
        assert_eq!(
            config(&pairs),
            Err(ConfigError::PricesRefresh("maybe".to_owned()))
        );
    }

    #[test]
    fn the_public_url_needs_a_scheme() {
        let mut pairs = minimal();
        pairs.retain(|(n, _)| *n != "TC_PUBLIC_URL");
        pairs.push(("TC_PUBLIC_URL", "tc.example.com"));
        assert_eq!(config(&pairs), Err(ConfigError::PublicUrl));
    }

    #[test]
    fn debug_output_hides_secrets() {
        let mut pairs = minimal();
        pairs.push(("TC_SMTP_URL", "smtp://user:pass@mail.example.com:587"));
        pairs.push(("TC_STRIPE_SECRET_KEY", "sk_test_hidden"));
        pairs.push(("TC_STRIPE_WEBHOOK_SECRET", "whsec_hidden"));
        let shown = format!("{:?}", config(&pairs).unwrap());
        assert!(!shown.contains(KEY));
        assert!(!shown.contains("pass@"));
        assert!(!shown.contains("secret@"));
        assert!(!shown.contains("sk_test_hidden"));
        assert!(!shown.contains("whsec_hidden"));
    }
}
