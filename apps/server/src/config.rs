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
    #[error("TC_RATES_REFRESH must be `on` or `off`, not {0:?}")]
    RatesRefresh(String),
    #[error(
        "TC_CLASSIFIER_URL must start with https:// or http://, such as https://api.openai.com/v1"
    )]
    ClassifierUrl,
}

/// The System 1 classifier that makes estimates and labels deviations.
/// Without one, people still type estimates.
#[derive(Clone, PartialEq, Eq)]
pub struct ClassifierConfig {
    /// `TC_CLASSIFIER_URL`: the API's base address without a trailing
    /// slash, such as `https://api.openai.com/v1`.
    pub url: String,
    /// `TC_CLASSIFIER_KEY`: sent as a bearer token; a model server of your
    /// own may need none.
    pub key: Option<String>,
    /// `TC_CLASSIFIER_MODEL`: such as `gpt-6-luna`.
    pub model: String,
}

/// Never prints the key.
impl fmt::Debug for ClassifierConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClassifierConfig")
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "<hidden>"))
            .field("model", &self.model)
            .finish()
    }
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
    /// `TC_PRICE_TABLE`: the path of the price table file, a CSV with
    /// provider, model, kind, price per million and valid from. Without it
    /// the instance has no managed price table.
    pub price_table: Option<String>,
    /// `TC_RATES_REFRESH`: the daily exchange rate refresh, on unless `off`.
    pub rates_refresh: bool,
    /// `TC_CLASSIFIER_URL`, `TC_CLASSIFIER_KEY` and `TC_CLASSIFIER_MODEL`:
    /// the estimator, if any.
    pub classifier: Option<ClassifierConfig>,
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

        let rates_refresh = match read("TC_RATES_REFRESH").as_deref().map(str::trim) {
            None | Some("on") => true,
            Some("off") => false,
            Some(other) => return Err(ConfigError::RatesRefresh(other.to_owned())),
        };

        let classifier = match (read("TC_CLASSIFIER_URL"), read("TC_CLASSIFIER_MODEL")) {
            (None, None) if read("TC_CLASSIFIER_KEY").is_none() => None,
            (Some(url), Some(model)) => {
                let url = url.trim().trim_end_matches('/').to_owned();
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err(ConfigError::ClassifierUrl);
                }
                Some(ClassifierConfig {
                    url,
                    key: read("TC_CLASSIFIER_KEY"),
                    model: model.trim().to_owned(),
                })
            }
            (Some(_), None) => return Err(ConfigError::Missing("TC_CLASSIFIER_MODEL")),
            (None, _) => return Err(ConfigError::Missing("TC_CLASSIFIER_URL")),
        };

        Ok(Self {
            database_url,
            public_url,
            secret_key,
            smtp_url: read("TC_SMTP_URL"),
            smtp_from: read("TC_SMTP_FROM"),
            price_table: read("TC_PRICE_TABLE").map(|path| path.trim().to_owned()),
            rates_refresh,
            classifier,
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
            .field("price_table", &self.price_table)
            .field("rates_refresh", &self.rates_refresh)
            .field("classifier", &self.classifier)
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
        assert_eq!(config.price_table, None);
        assert!(config.rates_refresh);
        assert_eq!(config.classifier, None);
    }

    #[test]
    fn a_classifier_needs_its_url_and_model() {
        let mut pairs = minimal();
        pairs.push(("TC_CLASSIFIER_URL", "https://api.openai.com/v1/"));
        pairs.push(("TC_CLASSIFIER_KEY", "sk-test-0123"));
        pairs.push(("TC_CLASSIFIER_MODEL", "gpt-6-luna"));
        let classifier = config(&pairs).unwrap().classifier.unwrap();
        assert_eq!(classifier.url, "https://api.openai.com/v1");
        assert_eq!(classifier.key.as_deref(), Some("sk-test-0123"));
        assert_eq!(classifier.model, "gpt-6-luna");
        assert!(!format!("{classifier:?}").contains("sk-test"));

        let mut only_url = minimal();
        only_url.push(("TC_CLASSIFIER_URL", "https://api.openai.com/v1"));
        assert_eq!(
            config(&only_url).unwrap_err(),
            ConfigError::Missing("TC_CLASSIFIER_MODEL")
        );
        let mut only_model = minimal();
        only_model.push(("TC_CLASSIFIER_MODEL", "gpt-6-luna"));
        assert_eq!(
            config(&only_model).unwrap_err(),
            ConfigError::Missing("TC_CLASSIFIER_URL")
        );
        let mut bad_url = minimal();
        bad_url.push(("TC_CLASSIFIER_URL", "api.openai.com"));
        bad_url.push(("TC_CLASSIFIER_MODEL", "gpt-6-luna"));
        assert_eq!(config(&bad_url).unwrap_err(), ConfigError::ClassifierUrl);
    }

    #[test]
    fn the_price_table_file_and_the_rates_switch_are_read() {
        let mut pairs = minimal();
        pairs.push(("TC_PRICE_TABLE", " /etc/tokencontroller/prices.csv "));
        pairs.push(("TC_RATES_REFRESH", "off"));
        let config = config(&pairs).unwrap();
        assert_eq!(
            config.price_table.as_deref(),
            Some("/etc/tokencontroller/prices.csv")
        );
        assert!(!config.rates_refresh);
        let mut bad = minimal();
        bad.push(("TC_RATES_REFRESH", "maybe"));
        assert_eq!(
            super::Config::from_lookup(|name| bad
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned()))
            .unwrap_err(),
            ConfigError::RatesRefresh("maybe".to_owned())
        );
    }

    #[test]
    fn missing_or_wrong_required_variables_are_named() {
        let mut no_db = minimal();
        no_db.remove(0);
        assert_eq!(
            config(&no_db).unwrap_err(),
            ConfigError::Missing("DATABASE_URL")
        );
        let mut short = minimal();
        short.pop();
        short.push(("TC_SECRET_KEY", "short"));
        assert_eq!(config(&short).unwrap_err(), ConfigError::SecretKeyTooShort);
        let mut bad_url = minimal();
        bad_url.remove(1);
        bad_url.push(("TC_PUBLIC_URL", "tc.example.com"));
        assert_eq!(config(&bad_url).unwrap_err(), ConfigError::PublicUrl);
        let blank = [
            ("DATABASE_URL", "postgres://tc:secret@db/tc"),
            ("TC_PUBLIC_URL", "  "),
            ("TC_SECRET_KEY", KEY),
        ];
        assert_eq!(
            config(&blank).unwrap_err(),
            ConfigError::Missing("TC_PUBLIC_URL")
        );
    }

    #[test]
    fn the_debug_output_hides_secrets() {
        let mut pairs = minimal();
        pairs.push(("TC_SMTP_URL", "smtps://user:pw@mail.example.com"));
        let shown = format!("{:?}", config(&pairs).unwrap());
        assert!(!shown.contains(KEY));
        assert!(!shown.contains("pw@"));
        assert!(shown.contains("tc.example.com"));
    }
}
