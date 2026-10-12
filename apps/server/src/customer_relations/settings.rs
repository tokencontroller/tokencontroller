//! The customer relations settings. Every one is optional and each feature is
//! off until its settings are set, so a self-hosted instance runs none of it.

use std::fmt;

/// The Claude model that writes support answers and site assistant answers.
pub const CLAUDE_MODEL: &str = "claude-opus-5-5";

/// Where the docs are read from, as one text file (`/llms-full.txt`).
pub const DOCS_TEXT_URL: &str = "https://docs.tokencontroller.com/llms-full.txt";

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com";
const RESEND_API_URL: &str = "https://api.resend.com";

/// Why the service refuses a customer relations setting.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("{0} must be set together with {1}")]
    Together(&'static str, &'static str),
    #[error("{0} must be a mail address such as `Name <name@example.com>`, not {1:?}")]
    Address(&'static str, String),
    #[error("TC_SITE_URL must start with https:// or http://, such as https://example.com")]
    SiteUrl,
    #[error("TC_ONBOARDING_EMAILS must be `on` or `off`, not {0:?}")]
    OnboardingEmails(String),
}

/// The customer relations settings.
#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    /// `TC_OPERATOR_EMAILS`: the people who run this instance. They read the
    /// support inbox and feedback reports and get notices about them.
    pub operator_emails: Vec<String>,
    /// `TC_INBOUND_MAIL_SECRET`: the secret the mail worker signs handed-over
    /// mail with. Turns on the support inbox.
    pub inbound_mail_secret: Option<String>,
    /// `TC_SUPPORT_FROM`: the sender of support answers.
    pub support_from: Option<String>,
    /// `TC_ANTHROPIC_API_KEY`: turns on answers from the docs, by mail and on
    /// the website.
    pub anthropic_api_key: Option<String>,
    /// `TC_RESEND_API_KEY`: turns on the newsletter, sent through Resend.
    pub resend_api_key: Option<String>,
    /// `TC_NEWSLETTER_FROM`: the sender of the newsletter.
    pub newsletter_from: Option<String>,
    /// `TC_SITE_URL`: the public website whose pages may use the site
    /// assistant, the contact form and the newsletter sign-up.
    pub site_url: Option<String>,
    /// `TC_ONBOARDING_EMAILS`: `on` sends a new organization's controllers one
    /// email for each first step it has not taken after a few days.
    pub onboarding_emails: bool,
    /// Where the Claude API is; changed only by tests.
    pub anthropic_api_url: String,
    /// Where the Resend API is; changed only by tests.
    pub resend_api_url: String,
    /// Where the docs text is; changed only by tests.
    pub docs_text_url: String,
}

impl Settings {
    /// Reads the settings through `lookup`, which returns a variable's value or
    /// `None` when it is not set.
    ///
    /// # Errors
    ///
    /// A [`SettingsError`] naming the variable to fix.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, SettingsError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let read = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };

        let operator_emails = read("TC_OPERATOR_EMAILS")
            .map(|list| {
                list.split(',')
                    .map(|email| email.trim().to_lowercase())
                    .filter(|email| !email.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let inbound_mail_secret = read("TC_INBOUND_MAIL_SECRET");
        let support_from = read("TC_SUPPORT_FROM");
        if inbound_mail_secret.is_some() && support_from.is_none() {
            return Err(SettingsError::Together(
                "TC_SUPPORT_FROM",
                "TC_INBOUND_MAIL_SECRET",
            ));
        }
        if let Some(from) = &support_from {
            check_address("TC_SUPPORT_FROM", from)?;
        }

        let resend_api_key = read("TC_RESEND_API_KEY");
        let newsletter_from = read("TC_NEWSLETTER_FROM");
        match (&resend_api_key, &newsletter_from) {
            (Some(_), None) => {
                return Err(SettingsError::Together(
                    "TC_NEWSLETTER_FROM",
                    "TC_RESEND_API_KEY",
                ));
            }
            (None, Some(_)) => {
                return Err(SettingsError::Together(
                    "TC_RESEND_API_KEY",
                    "TC_NEWSLETTER_FROM",
                ));
            }
            _ => {}
        }
        if let Some(from) = &newsletter_from {
            check_address("TC_NEWSLETTER_FROM", from)?;
        }

        let site_url = read("TC_SITE_URL").map(|url| url.trim_end_matches('/').to_owned());
        if let Some(url) = &site_url
            && !(url.starts_with("https://") || url.starts_with("http://"))
        {
            return Err(SettingsError::SiteUrl);
        }

        let onboarding_emails = match read("TC_ONBOARDING_EMAILS").as_deref() {
            None | Some("off") => false,
            Some("on") => true,
            Some(other) => return Err(SettingsError::OnboardingEmails(other.to_owned())),
        };

        Ok(Self {
            operator_emails,
            inbound_mail_secret,
            support_from,
            anthropic_api_key: read("TC_ANTHROPIC_API_KEY"),
            resend_api_key,
            newsletter_from,
            site_url,
            onboarding_emails,
            anthropic_api_url: ANTHROPIC_API_URL.to_owned(),
            resend_api_url: RESEND_API_URL.to_owned(),
            docs_text_url: DOCS_TEXT_URL.to_owned(),
        })
    }

    /// Whether `email` belongs to a person who runs this instance.
    #[must_use]
    pub fn is_operator(&self, email: &str) -> bool {
        let email = email.trim().to_lowercase();
        self.operator_emails.contains(&email)
    }
}

impl Default for Settings {
    /// Everything off.
    fn default() -> Self {
        Self {
            operator_emails: Vec::new(),
            inbound_mail_secret: None,
            support_from: None,
            anthropic_api_key: None,
            resend_api_key: None,
            newsletter_from: None,
            site_url: None,
            onboarding_emails: false,
            anthropic_api_url: ANTHROPIC_API_URL.to_owned(),
            resend_api_url: RESEND_API_URL.to_owned(),
            docs_text_url: DOCS_TEXT_URL.to_owned(),
        }
    }
}

fn check_address(name: &'static str, value: &str) -> Result<(), SettingsError> {
    value
        .parse::<lettre::message::Mailbox>()
        .map(|_| ())
        .map_err(|_| SettingsError::Address(name, value.to_owned()))
}

/// Never prints secrets, so the settings can be logged.
impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hidden = |value: &Option<String>| value.as_ref().map(|_| "<hidden>");
        f.debug_struct("Settings")
            .field("operator_emails", &self.operator_emails)
            .field("inbound_mail_secret", &hidden(&self.inbound_mail_secret))
            .field("support_from", &self.support_from)
            .field("anthropic_api_key", &hidden(&self.anthropic_api_key))
            .field("resend_api_key", &hidden(&self.resend_api_key))
            .field("newsletter_from", &self.newsletter_from)
            .field("site_url", &self.site_url)
            .field("onboarding_emails", &self.onboarding_emails)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn settings(pairs: &[(&str, &str)]) -> Result<Settings, SettingsError> {
        let vars: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        Settings::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn everything_is_off_by_default() {
        let settings = settings(&[]).unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.operator_emails.is_empty());
        assert!(!settings.is_operator("someone@example.com"));
    }

    #[test]
    fn operators_are_matched_without_case() {
        let settings =
            settings(&[("TC_OPERATOR_EMAILS", " Ops@Example.com, , b@example.com")]).unwrap();
        assert_eq!(
            settings.operator_emails,
            ["ops@example.com", "b@example.com"]
        );
        assert!(settings.is_operator("OPS@example.com "));
        assert!(!settings.is_operator("c@example.com"));
    }

    #[test]
    fn onboarding_emails_are_on_or_off() {
        assert!(
            settings(&[("TC_ONBOARDING_EMAILS", "on")])
                .unwrap()
                .onboarding_emails
        );
        assert!(
            !settings(&[("TC_ONBOARDING_EMAILS", "off")])
                .unwrap()
                .onboarding_emails
        );
        assert_eq!(
            settings(&[("TC_ONBOARDING_EMAILS", "yes")]),
            Err(SettingsError::OnboardingEmails("yes".to_owned()))
        );
    }

    #[test]
    fn the_support_inbox_needs_a_sender() {
        assert_eq!(
            settings(&[("TC_INBOUND_MAIL_SECRET", "secret")]),
            Err(SettingsError::Together(
                "TC_SUPPORT_FROM",
                "TC_INBOUND_MAIL_SECRET"
            ))
        );
        assert_eq!(
            settings(&[
                ("TC_INBOUND_MAIL_SECRET", "secret"),
                ("TC_SUPPORT_FROM", "not an address"),
            ]),
            Err(SettingsError::Address(
                "TC_SUPPORT_FROM",
                "not an address".to_owned()
            ))
        );
        let ok = settings(&[
            ("TC_INBOUND_MAIL_SECRET", "secret"),
            ("TC_SUPPORT_FROM", "Support <support@example.com>"),
        ])
        .unwrap();
        assert_eq!(
            ok.support_from.as_deref(),
            Some("Support <support@example.com>")
        );
    }

    #[test]
    fn the_newsletter_needs_both_settings() {
        assert_eq!(
            settings(&[("TC_RESEND_API_KEY", "re_x")]),
            Err(SettingsError::Together(
                "TC_NEWSLETTER_FROM",
                "TC_RESEND_API_KEY"
            ))
        );
        assert_eq!(
            settings(&[("TC_NEWSLETTER_FROM", "News <news@example.com>")]),
            Err(SettingsError::Together(
                "TC_RESEND_API_KEY",
                "TC_NEWSLETTER_FROM"
            ))
        );
        assert!(
            settings(&[
                ("TC_RESEND_API_KEY", "re_x"),
                ("TC_NEWSLETTER_FROM", "News <news@example.com>"),
            ])
            .is_ok()
        );
    }

    #[test]
    fn the_site_url_needs_a_scheme_and_loses_its_slash() {
        assert_eq!(
            settings(&[("TC_SITE_URL", "example.com")]),
            Err(SettingsError::SiteUrl)
        );
        let ok = settings(&[("TC_SITE_URL", "https://example.com/")]).unwrap();
        assert_eq!(ok.site_url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn debug_output_hides_secrets() {
        let shown = format!(
            "{:?}",
            settings(&[
                ("TC_ANTHROPIC_API_KEY", "sk-ant-secret"),
                ("TC_INBOUND_MAIL_SECRET", "inbound-secret"),
                ("TC_SUPPORT_FROM", "Support <support@example.com>"),
                ("TC_RESEND_API_KEY", "re_secret"),
                ("TC_NEWSLETTER_FROM", "News <news@example.com>"),
            ])
            .unwrap()
        );
        assert!(!shown.contains("sk-ant-secret"));
        assert!(!shown.contains("inbound-secret"));
        assert!(!shown.contains("re_secret"));
    }
}
